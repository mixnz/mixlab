//! Backing up and restoring a PostgreSQL database with `pg_dump` and `psql`.
//!
//! The child process, its progress and its cancellation are [`super::super::dump`]'s. The
//! credentials go through the password file the tools already read, and the database's name
//! through the environment, never the command line.

use super::super::dump::{
    create_file, open_file, run, share, DumpMode, Fed, Invocation, Lead, Progress, Tick, Tracker,
    Watch,
};
use crate::error::AppError;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// A PostgreSQL password file holding the credentials, deleted when this goes out of scope.
///
/// The same reasoning as `OptionFile` in `mysql/dump.rs`: `pg_dump` and `psql` have no `--password`
/// to pass on the command line at all, and the alternative is `PGPASSWORD` in the environment — which is
/// inherited by anything the tool starts and, on some systems, readable from outside the process.
/// A password file is what these tools are built to read, and pointing `PGPASSFILE` at one is how
/// they are told to read this one rather than the user's own.
struct PgPassFile {
    path: PathBuf,
}

impl PgPassFile {
    fn new(host: &str, port: u16, user: &str, password: &str) -> Result<Self, AppError> {
        let path = std::env::temp_dir().join(format!("mixlab-{}.pgpass", uuid::Uuid::new_v4()));
        let mut file = File::create(&path)
            .map_err(|e| err!("error.cannotWriteFile", path = path.display(), message = e))?;
        // The format is `host:port:database:user:password`, one entry per line, with `:` and `\`
        // inside a field escaped by a backslash — otherwise a password holding a colon would read
        // as the end of the field and the beginning of another.
        let escaped = |value: &str| value.replace('\\', "\\\\").replace(':', "\\:");
        // `*` for the database: this file exists for one connection, and which of that server's
        // databases the tool is pointed at is the caller's business rather than the password's.
        let body = format!(
            "{}:{port}:*:{}:{}\n",
            escaped(host),
            escaped(user),
            escaped(password)
        );
        file.write_all(body.as_bytes())
            .map_err(|e| err!("error.cannotWriteFile", path = path.display(), message = e))?;
        // Not merely tidy: libpq refuses to read a password file that anyone else can, and ignores
        // it silently — which would leave the tool prompting for a password that never comes.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| err!("error.cannotWriteFile", path = path.display(), message = e))?;
        }
        Ok(Self { path })
    }

    /// The environment the tool is run with: where to find this file, how long to wait for a
    /// server that never answers, and which database to open. Not asking for a password
    /// interactively is `--no-password`, which goes on the command line beside it.
    ///
    /// `PGDATABASE` rather than `--dbname=`, which is where the database used to go: libpq reads a
    /// `dbname` holding an `=`, or starting `postgresql://`, as a whole connection string and
    /// expands it. So a database named `dbname=other` is not opened — it is *parsed*, and the tool
    /// connects wherever the name says, which for a restore means writing a file into a database
    /// nobody chose. `PGDATABASE` is only ever a name; libpq fills it in as a default, after the
    /// point where that expansion happens.
    fn env(&self, database: &str) -> Vec<(&'static str, String)> {
        vec![
            ("PGPASSFILE", self.path.display().to_string()),
            ("PGCONNECT_TIMEOUT", "10".to_string()),
            ("PGDATABASE", database.to_string()),
        ]
    }
}

impl Drop for PgPassFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// What `pg_dump --verbose` writes as it reaches each table, and the whole of the signal the
/// progress below is built on.
const PG_TABLE_LINE: &str = "dumping contents of table ";

/// The table `pg_dump` has just reached, out of one line of its commentary — named the way the rest
/// of the app names one, so that it matches the weights it is looked up in.
///
/// Three spellings are recognised, because the wording has changed across the versions this app can
/// meet: `"public.users"` (12 and later), `"public"."users"` (older), and a bare `users` (older
/// still, and only for the schema on the search path). Everything before the phrase is skipped
/// rather than matched, since the line is prefixed with the program's name on some versions and not
/// on others.
///
/// The most brittle thing here, as on MySQL: a future `pg_dump` that words this differently says
/// nothing this recognises, and the dump then runs with a bar that moves without a number.
fn pg_reached_table(line: &str) -> Option<String> {
    let rest = line.split_once(PG_TABLE_LINE)?.1.trim();
    let unquoted = rest.trim_matches('"');
    let (schema, table) = match unquoted.split_once("\".\"") {
        Some(pair) => pair,
        // A dot inside the one pair of quotes. Split at the first, since a schema's name may not be
        // written unquoted with a dot in it while a table reached this way could have been.
        None => match unquoted.split_once('.') {
            Some(pair) => pair,
            None => (super::DEFAULT_SCHEMA, unquoted),
        },
    };
    Some(super::qualify(schema, table))
}

/// The command line a pg_dump run is given. The database is not on it — see [`PgPassFile::env`].
fn pg_dump_args(host: &str, port: u16, user: &str, mode: DumpMode) -> Vec<String> {
    let mut args = vec![
        format!("--host={host}"),
        format!("--port={port}"),
        format!("--username={user}"),
        // Never prompt. Without it a tool that cannot authenticate would sit waiting on a console
        // that is not there, and the dump would hang rather than fail.
        "--no-password".to_string(),
        // Names each table on standard error as it reaches it, which is the only account pg_dump
        // gives of how far along it is. It goes nowhere near the dump itself, which is standard
        // output.
        "--verbose".to_string(),
        // Plain SQL, so that `psql` can replay it and so that the file is readable — the custom
        // format would need `pg_restore` and would not be a file anyone could open.
        "--format=plain".to_string(),
        // Both left out of the file: see above.
        "--no-owner".to_string(),
        "--no-privileges".to_string(),
        // Written as UTF-8 whatever the database's own encoding, which is what the restore below
        // reads it back as and what makes a dump portable between servers.
        "--encoding=UTF8".to_string(),
        // Nothing here asks for a consistent snapshot, because pg_dump already takes one: it runs
        // in a repeatable-read transaction of its own accord. `--serializable-deferrable` would go
        // further and is deliberately not used — it can sit waiting for a snapshot with no
        // serialization anomalies in it, which on a busy server is a dump that never starts.
        //
        // The database itself is not here: it goes in the environment. See [`PgPassFile::env`].
    ];
    match mode {
        // Sequences, functions and triggers all belong to the structure and come with it; there is
        // no separate switch for them as there is on MySQL.
        DumpMode::Structure => args.push("--schema-only".to_string()),
        DumpMode::Data => args.push("--data-only".to_string()),
        DumpMode::All => {}
    }
    args
}

/// Writes `database` to `path` as SQL.
///
/// The dump carries no `CREATE DATABASE` — that would be `--create` — so it restores into whichever
/// database it is pointed at, as the MySQL one does. Ownership and privileges are left out for the
/// same reason: a dump that names roles restores only onto a server that has them, and the roles of
/// the server it came from are not part of what the user asked to back up.
///
/// `tables` is every table of the database against what its rows weigh on the server, which is what
/// the progress reported through `watch` is worked out from; see [`Tracker`]. Empty when the server
/// would not say, which costs the dump nothing but its percentage.
#[allow(clippy::too_many_arguments)]
pub fn dump(
    tool: &Path,
    host: &str,
    port: u16,
    user: &str,
    password: &str,
    database: &str,
    mode: DumpMode,
    path: &str,
    tables: &[(String, u64)],
    watch: &Watch<'_>,
) -> Result<(), AppError> {
    let pgpass = PgPassFile::new(host, port, user, password)?;
    let args = pg_dump_args(host, port, user, mode);

    let out = create_file(path)?;
    let mut tracker = Tracker::new(tables, path, mode != DumpMode::Structure);
    run(
        Invocation {
            tool,
            args: &args,
            env: &pgpass.env(database),
            stdin: None,
            stdout: Some(out),
            name: "pg_dump",
        },
        watch,
        |tick| {
            if let Tick::Line(line) = tick {
                // Everything else it says — connecting, reading the schema, saving the search path —
                // leaves the reckoning where it was.
                let Some(table) = pg_reached_table(line) else {
                    return;
                };
                tracker.reached(&table);
            }
            (watch.report)(tracker.progress());
        },
    )
}

/// How much of the start of a dump is read looking for its preamble — a dozen short lines under a
/// comment, so this is many times over what it takes.
const PG_PREAMBLE_LIMIT: u64 = 8 * 1024;

/// What replaces `SET transaction_timeout = 0;`: the same instruction, said in a way a server that
/// has never heard of the parameter can refuse without the restore stopping.
const PG_TIMEOUT_GUARD: &str = "DO $$ BEGIN PERFORM pg_catalog.set_config('transaction_timeout', \
                                '0', false); EXCEPTION WHEN undefined_object THEN END $$;\n";

/// Whether the line is `pg_dump`'s own `SET transaction_timeout = 0;`, spacing aside.
///
/// Only that one value is recognised: zero is what `pg_dump` writes and what {@link
/// PG_TIMEOUT_GUARD} puts back, and a line saying anything else was written by hand and is not
/// this module's to rewrite.
fn pg_disables_transaction_timeout(statement: &str) -> bool {
    let Some(assignment) = statement
        .strip_prefix("SET ")
        .and_then(|rest| rest.strip_suffix(';'))
    else {
        return false;
    };
    match assignment.split_once('=') {
        Some((name, value)) => name.trim() == "transaction_timeout" && value.trim() == "0",
        None => false,
    }
}

/// Whether the line is one of a dump's opening lines — the comment block, psql's own `\restrict`,
/// and the `SET`s that follow them. The rewrite walks no further than these, so that a row of data
/// which happens to read like a `SET` cannot be taken for one.
fn pg_preamble_line(statement: &str) -> bool {
    statement.is_empty()
        || statement.starts_with("--")
        || statement.starts_with('\\')
        || (statement.ends_with(';')
            && (statement.starts_with("SET ")
                || statement.starts_with("SELECT pg_catalog.set_config(")))
}

/// Rewrites the opening lines of a dump so that a server which has never heard of
/// `transaction_timeout` can still be restored into — and `None` for a dump that needs nothing
/// doing to it, which is most of them.
///
/// `pg_dump` opens every dump by setting the timeouts to zero, so that a server configured to cut
/// long statements short cannot cut the restore short. `transaction_timeout` is the newest of them
/// — PostgreSQL 17 — and `pg_dump` writes the line whatever server it is talking to, because the
/// preamble is the client's and not the server's. Restoring such a dump into anything older stops
/// on that line with `unrecognized configuration parameter`, and since the app pins the newest
/// `pg_dump` there is (it refuses to dump a server newer than itself, see `tools::PG_VERSION`)
/// that is *every* restore into a server older than 17 — including a dump this app took from that
/// same server minutes earlier.
///
/// The line is replaced rather than dropped because a server that does know the parameter should
/// still have it set. A block that swallows `undefined_object` does both without this side having
/// to ask the server its version first.
fn pg_rewrite_preamble(head: &[u8]) -> Option<Lead> {
    let mut bytes = Vec::new();
    let mut replaced = 0u64;
    for line in head.split_inclusive(|byte| *byte == b'\n') {
        // A last line with no ending is one the read cut in half, and what the rest of it says
        // cannot be known from here. Nor can a line that is not UTF-8, which no preamble line is.
        if !line.ends_with(b"\n") {
            break;
        }
        let Ok(text) = std::str::from_utf8(line) else {
            break;
        };
        let statement = text.trim();
        if pg_disables_transaction_timeout(statement) {
            bytes.extend_from_slice(PG_TIMEOUT_GUARD.as_bytes());
            return Some(Lead {
                bytes,
                replaced: replaced + line.len() as u64,
            });
        }
        if !pg_preamble_line(statement) {
            break;
        }
        bytes.extend_from_slice(line);
        replaced += line.len() as u64;
    }
    None
}

/// Reads the start of a dump, works out what of it has to be rewritten, and leaves the file
/// positioned at the first byte the tool is to be given as it stands.
fn pg_preamble(file: &mut File, path: &str) -> Result<Option<Lead>, AppError> {
    let mut head = Vec::new();
    (&mut *file)
        .take(PG_PREAMBLE_LIMIT)
        .read_to_end(&mut head)
        .map_err(|e| err!("error.cannotReadFile", path = path, message = e))?;
    let lead = pg_rewrite_preamble(&head);
    // Back to the very start for a dump with nothing to rewrite, since the read above moved past
    // it — the whole file is poured over as it stands.
    let start = lead.as_ref().map_or(0, |lead| lead.replaced);
    file.seek(SeekFrom::Start(start))
        .map_err(|e| err!("error.cannotReadFile", path = path, message = e))?;
    Ok(lead)
}

/// The command line the `psql` client is given. As in [`pg_dump_args`], the database is not on it.
fn psql_args(host: &str, port: u16, user: &str) -> Vec<String> {
    vec![
        format!("--host={host}"),
        format!("--port={port}"),
        format!("--username={user}"),
        "--no-password".to_string(),
        // The user's own `~/.psqlrc` is not read: it may set anything at all — a pager, a format,
        // `ON_ERROR_ROLLBACK` — and none of it belongs in the middle of a restore.
        "--no-psqlrc".to_string(),
        "--quiet".to_string(),
        "--set=ON_ERROR_STOP=1".to_string(),
    ]
}

/// Replays a SQL file through `psql`, into `database`.
///
/// `ON_ERROR_STOP` is what makes a failure a failure: without it psql reports the statement it
/// could not run and carries straight on, leaving a half-restored database behind and an exit
/// status of zero to say it went well.
///
/// Progress is the same count as the MySQL restore's — the share of the file handed over — and
/// stops short of 100 for the same reason: the last statements are still running when the last
/// bytes have gone.
#[allow(clippy::too_many_arguments)]
pub fn restore(
    tool: &Path,
    host: &str,
    port: u16,
    user: &str,
    password: &str,
    database: &str,
    path: &str,
    watch: &Watch<'_>,
) -> Result<(), AppError> {
    let pgpass = PgPassFile::new(host, port, user, password)?;
    let args = psql_args(host, port, user);

    let mut file = open_file(path)?;
    // A dump written by a newer pg_dump than the server it is going into needs a word changing
    // before psql sees it; see [`pg_rewrite_preamble`].
    let lead = pg_preamble(&mut file, path)?;
    // A file whose size cannot be read is still restored, just without a percentage to go with it.
    let total = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    let sent = Arc::new(AtomicU64::new(0));
    let counted = Arc::clone(&sent);
    let fed = Fed {
        file,
        path: path.to_string(),
        sent,
        lead,
    };

    run(
        Invocation {
            tool,
            args: &args,
            env: &pgpass.env(database),
            stdin: Some(fed),
            stdout: None,
            name: "psql",
        },
        watch,
        |_| {
            (watch.report)(Progress {
                percent: share(counted.load(Ordering::Relaxed), total),
                ..Progress::default()
            });
        },
    )
}

#[cfg(test)]
mod tests;
