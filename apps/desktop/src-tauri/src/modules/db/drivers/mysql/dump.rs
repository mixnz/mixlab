//! Backing up and restoring a MySQL or MariaDB database with `mysqldump` and `mysql`.
//!
//! The child process, its progress and its cancellation are [`super::super::dump`]'s; this is
//! what is asked of the two tools. The credentials go through a temporary option file that only
//! this user can read, never the command line, where any other process could read them.

use super::super::dump::{
    create_file, open_file, run, share, DumpMode, Fed, Invocation, Progress, Tick, Tracker, Watch,
};
use crate::error::AppError;
use crate::platform::hide_console;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// A MySQL option file holding the credentials, deleted when this goes out of scope.
///
/// The alternative is `--password=` on the command line, which every other process on the machine
/// can read out of the process list for as long as the dump runs.
///
/// Handed to the tools as `--defaults-file`, never `--defaults-extra-file`: the extra file is read
/// *before* the user's own `~/.my.cnf`, so a `[client]` there holding `password=` — which a local
/// install commonly leaves behind — wins over the credentials in here, and the tool connects with
/// no password at all. `--defaults-file` reads this file alone. (`~/.mylogin.cnf` is still read;
/// MySQL makes no exception for it.)
struct OptionFile {
    path: PathBuf,
}

impl OptionFile {
    fn new(host: &str, port: u16, user: &str, password: &str) -> Result<Self, AppError> {
        let path = std::env::temp_dir().join(format!("mixlab-{}.cnf", uuid::Uuid::new_v4()));
        let mut file = File::create(&path)
            .map_err(|e| err!("error.cannotWriteFile", path = path.display(), message = e))?;
        // Values are double-quoted, which is the one form of an option file value that may hold
        // `#`, spaces or a leading digit — with `\` and `"` escaped so the quoting holds.
        let quoted =
            |value: &str| format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""));
        let body = format!(
            "[client]\nhost={}\nport={port}\nuser={}\npassword={}\n",
            quoted(host),
            quoted(user),
            quoted(password)
        );
        file.write_all(body.as_bytes())
            .map_err(|e| err!("error.cannotWriteFile", path = path.display(), message = e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(Self { path })
    }
}

impl Drop for OptionFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// The line `mysqldump --verbose` writes as it reaches each table, and the whole of the signal the
/// count below is built on.
const TABLE_LINE: &str = "-- Retrieving table structure for table ";

/// The table mysqldump has just reached, out of one line of its commentary — and `None` for every
/// other thing it has to say, which is most of them.
///
/// The most brittle thing here: a future mysqldump that words this differently says nothing this
/// recognises, and the dump then runs with a bar that moves without a number.
fn reached_table(line: &str) -> Option<&str> {
    line.strip_prefix(TABLE_LINE)?.strip_suffix("...")
}

/// Whether the dump tool at `tool` is MariaDB's own rather than MySQL's.
///
/// Not the same question as which server is being dumped, and it has to be asked separately: a
/// machine with MariaDB installed has a `mysqldump` on it too — MariaDB ships the old name pointing
/// at `mariadb-dump` — so the name this was found under says nothing. Two of the flags below exist
/// only in MySQL's tool, and MariaDB's does not ignore what it does not know:
///
/// ```text
/// $ mariadb-dump ... --set-gtid-purged=OFF --column-statistics=0 demo
/// mariadb-dump: unknown variable 'set-gtid-purged=OFF'
/// mariadb-dump: unknown variable 'column-statistics=0'
/// $ echo $?
/// 7
/// ```
///
/// Neither has anything to answer for on MariaDB: it writes no `GTID_PURGED` to be turned off, and
/// has no histograms to be skipped. So the two are dropped rather than translated. A tool that will
/// not say what it is keeps them, which is how every dump ran before this asked.
fn is_mariadb_tool(tool: &Path) -> bool {
    let mut command = Command::new(tool);
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    hide_console(&mut command);
    command.output().is_ok_and(|out| {
        String::from_utf8_lossy(&out.stdout)
            .to_ascii_lowercase()
            .contains("mariadb")
    })
}

/// The command line a mysqldump run is given, with the database name last.
///
/// Its own function so that what goes on that line can be read — and tested — without a server to
/// point it at: everything here is a decision, and several of them are conditional.
fn mysqldump_args(
    defaults_file: &Path,
    charset: &str,
    mariadb: bool,
    column_statistics: bool,
    mode: DumpMode,
    database: &str,
) -> Vec<String> {
    let mut args = vec![
        // Must come first: mysqldump reads its option files before anything else on the line. The
        // only one, not an extra one — see [`OptionFile`].
        format!("--defaults-file={}", defaults_file.display()),
        format!("--default-character-set={charset}"),
        // Names each table on standard error as it reaches it, which is the only account mysqldump
        // gives of how far along it is. It goes nowhere near the dump itself, which is standard
        // output.
        "--verbose".to_string(),
        // Timestamps come out as they are stored rather than converted to UTC, so a restore onto
        // a server in another time zone still reads back the same wall-clock values.
        "--skip-tz-utc".to_string(),
        // The usual bundle: drop-and-create, extended inserts, quick, disable-keys, set-charset.
        "--opt".to_string(),
        "--no-autocommit".to_string(),
        // Takes the whole dump from one consistent snapshot instead of locking the tables.
        "--single-transaction".to_string(),
        "--no-tablespaces".to_string(),
        // Binary columns as hex rather than as escaped text: no character set can touch them, so
        // they come back byte for byte.
        "--hex-blob".to_string(),
        "--force".to_string(),
    ];
    // Both are MySQL's tool's alone, and MariaDB's refuses the whole run over either — see
    // [`is_mariadb_tool`].
    if !mariadb {
        // Otherwise an 8.0 mysqldump writes a `SET @@GLOBAL.GTID_PURGED` the restoring server
        // usually refuses, and which has nothing to do with the data being carried across.
        args.push("--set-gtid-purged=OFF".to_string());
        if !column_statistics {
            args.push("--column-statistics=0".to_string());
        }
    }
    match mode {
        DumpMode::Structure => {
            args.push("--no-data".to_string());
            args.push("--routines".to_string());
            args.push("--events".to_string());
        }
        DumpMode::Data => {
            args.push("--no-create-info".to_string());
            // Both belong to the structure, and mysqldump writes them beside the tables they are
            // attached to — which a data-only dump has none of.
            args.push("--skip-triggers".to_string());
            args.push("--skip-routines".to_string());
        }
        DumpMode::All => {
            args.push("--routines".to_string());
            args.push("--events".to_string());
        }
    }
    /* End of options. Without it a database whose name begins with `-` is read as one — and
    mysqldump has short options that take no argument, so `-x` is not rejected, it silently
    turns on `--lock-all-tables` and then dumps nothing at all. Both MySQL's `my_getopt` and
    MariaDB's stop at a bare `--`. */
    args.push("--".to_string());
    // The bare name rather than `--databases`, which is what would put `CREATE DATABASE` and a
    // `USE` at the head of the file. Without them the dump names no database at all, so it
    // restores into whichever one it is pointed at — at the cost of the database's own default
    // character set, which each table carries its own copy of anyway.
    args.push(database.to_string());
    args
}

/// Writes `database` to `path` as SQL.
///
/// `charset` is the character set the dump is transferred in — see `mysql::structure::dump_charset`
/// for how it is chosen. It matters more than it looks: mysqldump converts every string on the way
/// out to this character set and writes a matching `SET NAMES` into the file, so a wrong one is a
/// restore where the text comes back mangled.
///
/// `column_statistics` says whether the server has the `information_schema.COLUMN_STATISTICS`
/// table that an 8.0 mysqldump reads histograms from. A 5.x server has not, and asking it for one
/// is an error that stops the dump — so against those the feature is turned off.
///
/// `tables` is every base table of the database against what its rows weigh on the server, which
/// is what the progress reported through `watch` is worked out from; see [`Tracker`]. Empty when
/// the server would not say, which costs the dump nothing but its percentage.
#[allow(clippy::too_many_arguments)]
pub fn dump(
    tool: &Path,
    host: &str,
    port: u16,
    user: &str,
    password: &str,
    database: &str,
    charset: &str,
    mode: DumpMode,
    column_statistics: bool,
    path: &str,
    tables: &[(String, u64)],
    watch: &Watch<'_>,
) -> Result<(), AppError> {
    let options = OptionFile::new(host, port, user, password)?;
    let args = mysqldump_args(
        &options.path,
        charset,
        is_mariadb_tool(tool),
        column_statistics,
        mode,
        database,
    );
    let out = create_file(path)?;
    let mut tracker = Tracker::new(tables, path, mode != DumpMode::Structure);
    run(
        Invocation {
            tool,
            args: &args,
            env: &[],
            stdin: None,
            stdout: Some(out),
            name: "mysqldump",
        },
        watch,
        |tick| {
            if let Tick::Line(line) = tick {
                // Everything else it says — connecting, savepoints, the rows of a table already
                // counted — leaves the reckoning where it was, and is not worth a reading of its own.
                let Some(table) = reached_table(line) else {
                    return;
                };
                tracker.reached(table);
            }
            (watch.report)(tracker.progress());
        },
    )
}

/// The command line the `mysql` client is given, with the database to restore into last.
fn mysql_restore_args(defaults_file: &Path, database: &str) -> Vec<String> {
    vec![
        format!("--defaults-file={}", defaults_file.display()),
        // Only the initial setting: a dump carries its own `SET NAMES`, which takes over from here.
        "--default-character-set=utf8mb4".to_string(),
        // Stop at the first statement the server rejects instead of carrying on and leaving a
        // half-restored database behind with the reason scrolled off.
        "--batch".to_string(),
        // See [`mysqldump_args`]: the same `--` for the same reason, and here a database named
        // `-e` would not merely stop the restore, it would be read as a statement to run.
        "--".to_string(),
        database.to_string(),
    ]
}

/// Replays a SQL file through the `mysql` client.
///
/// `database` is where the file's statements land, since a dump written by this app names no
/// database of its own. A file from elsewhere that does carry a `USE` still overrides this — the
/// client cannot refuse a statement in the file it is given.
///
/// Progress is the plainest of any of these: the file is fed to the client by this side rather
/// than handed over, so what is reported through `watch` is the share of it actually sent. It runs
/// a little ahead of the truth at the end — the last statements are still being executed when the
/// last bytes have gone — which is why the figure stops short of 100.
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
    let options = OptionFile::new(host, port, user, password)?;
    let args = mysql_restore_args(&options.path, database);

    let file = open_file(path)?;
    // A file whose size cannot be read is still restored, just without a percentage to go with it.
    let total = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    let sent = Arc::new(AtomicU64::new(0));
    let counted = Arc::clone(&sent);
    let fed = Fed {
        file,
        path: path.to_string(),
        sent,
        lead: None,
    };

    run(
        Invocation {
            tool,
            args: &args,
            env: &[],
            stdin: Some(fed),
            stdout: None,
            name: "mysql",
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
