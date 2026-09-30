//! Backing up and restoring a whole database, by driving the vendors' own command-line tools:
//! `mysqldump`/`mysql`, `pg_dump`/`psql` and `mongodump`/`mongorestore`. Where those tools come
//! from is {@link super::tools}' business; this module is what runs them.
//!
//! Writing a dump by hand would mean reimplementing every corner of the three servers' output —
//! definers, triggers, routines, sequences, extensions, BSON types — and being wrong about one of
//! them is a restore that silently differs from the original.
//!
//! Nothing in here interpolates a password into a command line: on most systems the arguments of a
//! running process are readable by any other process of the same user, so MySQL's credentials go
//! through a temporary option file that only this user can read, and PostgreSQL's through the
//! password file its tools already know how to read. MongoDB's have nowhere else to go — its tools
//! take a URI and nothing else — which is a limitation of those tools.

use crate::error::AppError;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::platform::hide_console;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

/// What of a MySQL database is to be written out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DumpMode {
    /// The table definitions, routines and triggers, with no rows.
    Structure,
    /// The rows alone, to be loaded into a database that already has the tables.
    Data,
    /// Both, which is what makes the dump able to rebuild the database on its own.
    All,
}

impl DumpMode {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "structure" => Ok(Self::Structure),
            "data" => Ok(Self::Data),
            "all" => Ok(Self::All),
            other => Err(err!("error.unknownDumpMode", mode = other)),
        }
    }
}

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

/// A PostgreSQL password file holding the credentials, deleted when this goes out of scope.
///
/// The same reasoning as {@link OptionFile}: `pg_dump` and `psql` have no `--password` to pass on
/// the command line at all, and the alternative is `PGPASSWORD` in the environment — which is
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

/// What the caller of [`run`] is shown while the tool runs: every line the tool writes to its
/// standard error as it arrives, and nothing at all every quarter second — so a caller watching
/// something other than the tool's own words, such as the size of the file it is writing, still has
/// somewhere to look from.
enum Tick<'a> {
    Line(&'a str),
    Idle,
}

/// A file to be poured into a tool's standard input, counting what has gone in.
///
/// The count is the whole of a restore's progress: the mysql client has no idea how far through
/// the file it is, but the file is of a known size and every byte of it has to be handed over.
struct Fed {
    file: File,
    /// Where the file came from, for the error a file that cannot be read through has to raise.
    path: String,
    /// How much of it the tool has been given, read by the caller's ticks from another thread.
    sent: Arc<AtomicU64>,
    /// What goes in ahead of the file, in place of the start of it that `file` is already
    /// positioned past. Only the PostgreSQL restore ever has one; see [`pg_rewrite_preamble`].
    lead: Option<Lead>,
}

/// A rewritten start of a dump: `bytes` are handed to the tool in place of the file's first
/// `replaced` bytes.
struct Lead {
    bytes: Vec<u8>,
    replaced: u64,
}

impl Fed {
    fn pour_into(self, mut sink: std::process::ChildStdin) -> Result<(), AppError> {
        let Self {
            mut file,
            path,
            sent,
            lead,
        } = self;
        if let Some(Lead { bytes, replaced }) = lead {
            // Returning drops `sink`, which is what the end of the loop below does by hand.
            if sink.write_all(&bytes).is_err() {
                return Ok(());
            }
            // Progress is the share of the *file* that has gone over, so what counts here is the
            // stretch of it these bytes stand in for rather than how many of them there are.
            sent.fetch_add(replaced, Ordering::Relaxed);
        }
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|e| err!("error.cannotReadFile", path = path.as_str(), message = e))?;
            if read == 0 {
                break;
            }
            // A tool that has stopped reading has died, and the reason will be in its status and on
            // its standard error — which say far more than this end of the pipe can.
            if sink.write_all(&buffer[..read]).is_err() {
                break;
            }
            sent.fetch_add(read as u64, Ordering::Relaxed);
        }
        // Closing it is what tells the tool there is no more to come; without this it would sit
        // waiting for input that will never arrive.
        drop(sink);
        Ok(())
    }
}

/// The last few lines of a tool's standard error, kept in case it fails.
///
/// mysqldump's `--verbose` commentary is held apart from the rest: it is chatter this module asked
/// for, and it would otherwise crowd out the one line that says what went wrong — which these tools
/// write last. It is still kept, for a failure that leaves nothing else to show.
#[derive(Default)]
struct Tail {
    said: VecDeque<String>,
    everything: VecDeque<String>,
}

impl Tail {
    fn push(&mut self, line: String) {
        if line.trim().is_empty() {
            return;
        }
        if !line.starts_with("--") {
            Self::keep(&mut self.said, line.clone());
        }
        Self::keep(&mut self.everything, line);
    }

    fn keep(lines: &mut VecDeque<String>, line: String) {
        lines.push_back(line);
        if lines.len() > 8 {
            lines.pop_front();
        }
    }

    fn message(&self) -> String {
        let lines = if self.said.is_empty() {
            &self.everything
        } else {
            &self.said
        };
        lines.iter().cloned().collect::<Vec<_>>().join("\n")
    }
}

/// One line of a tool's standard error, with its line ending off — and `None` once there are no
/// more of them.
///
/// Read as bytes and made into text afterwards rather than through `BufRead::lines`, which hands
/// back an error for a line that is not valid UTF-8. These tools write their errors in whatever
/// character set the connection is using, and a dump's is the database's own — so a `latin1`
/// database whose error mentions a name with an accent in it is a line `lines` would refuse. That
/// refusal would end the reading there and take the rest of the failure with it, which is the one
/// thing standard error is being kept for.
fn next_line(source: &mut impl BufRead, buffer: &mut Vec<u8>) -> Option<String> {
    buffer.clear();
    match source.read_until(b'\n', buffer) {
        Ok(0) | Err(_) => None,
        // Takes the `\r` of a CRLF ending with it, which is what these tools write on Windows.
        Ok(_) => Some(String::from_utf8_lossy(buffer).trim_end().to_string()),
    }
}

/// Runs a tool to completion, with its output wired to the given files, and turns anything but a
/// clean exit into the error the caller reports.
///
/// `tick` is called as the tool talks and, failing that, four times a second; a caller with nothing
/// to watch passes a closure that does nothing.
///
/// Blocking on purpose — every caller is a `spawn_blocking`, because a dump takes as long as it
/// takes and has no business holding an async worker (or the connection lock) while it does.
struct Invocation<'a> {
    tool: &'a Path,
    args: &'a [String],
    env: &'a [(&'a str, String)],
    /// The file to pour into the tool's standard input, for a restore. `None` for a dump, which
    /// writes rather than reads.
    stdin: Option<Fed>,
    /// Where the tool's standard output goes, for a dump. `None` for the tools that write the file
    /// themselves.
    stdout: Option<File>,
    /// What the tool is called in an error the user reads — `mysqldump`, `psql`.
    name: &'a str,
}

fn run(
    Invocation {
        tool,
        args,
        env,
        stdin,
        stdout,
        name,
    }: Invocation<'_>,
    watch: &Watch<'_>,
    mut tick: impl FnMut(Tick),
) -> Result<(), AppError> {
    let mut command = Command::new(tool);
    command
        .args(args)
        .envs(env.iter().map(|(name, value)| (*name, value)))
        // Poured in by this side rather than handed over as a file, which is what lets the bytes be
        // counted on their way past.
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(stdout.map_or_else(Stdio::null, Stdio::from))
        .stderr(Stdio::piped());
    // Without this a console window flashes up over the app for every tool run.
    hide_console(&mut command);

    let mut child = command
        .spawn()
        .map_err(|e| err!("error.cannotRunTool", tool = tool.display(), message = e))?;

    let feeder = stdin
        .zip(child.stdin.take())
        .map(|(fed, sink)| std::thread::spawn(move || fed.pour_into(sink)));

    // Read on a thread of its own rather than after the wait: a tool that fills the pipe while this
    // side waits for it to exit would deadlock, and the lines are wanted as they are written rather
    // than once there are no more of them.
    let (sender, lines) = mpsc::channel();
    let reader = child.stderr.take().map(|stderr| {
        std::thread::spawn(move || {
            let mut stderr = BufReader::new(stderr);
            let mut buffer = Vec::new();
            while let Some(line) = next_line(&mut stderr, &mut buffer) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        })
    });

    let mut tail = Tail::default();
    let mut cancelled = false;
    loop {
        match lines.recv_timeout(Duration::from_millis(250)) {
            Ok(line) => {
                tick(Tick::Line(&line));
                tail.push(line);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => tick(Tick::Idle),
            // Standard error is closed, which the tool does by exiting.
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if (watch.cancel)() {
            /* Killed rather than asked. These tools have no protocol for stopping politely, and a
            dump left running writes a file nobody is waiting for while its progress is reported
            against a connection that has gone. Killing also closes the stdin pipe, which is
            what stops the feeder thread below — it gets a broken pipe on its next write. */
            let _ = child.kill();
            cancelled = true;
            break;
        }
    }
    if let Some(reader) = reader {
        let _ = reader.join();
    }

    let status = child
        .wait()
        .map_err(|e| err!("error.toolWaitFailed", tool = name, message = e))?;
    /* Before the status is read for meaning: a killed tool exits without success, and reporting
    that as "mysqldump failed" would blame the tool for being stopped. */
    if cancelled {
        return Err(err!("error.transferCancelled", tool = name));
    }
    if !status.success() {
        // These tools report the real cause on stderr and only a number through the exit status, so
        // the message is what matters; the last lines of it are the ones that say why.
        return Err(err!(
            "error.toolFailed",
            tool = name,
            status = status,
            message = tail.message()
        ));
    }
    // Asked after the status and not before, because a tool that failed says why and a broken pipe
    // only says that it did. But a file that could not be read through is not allowed to pass as a
    // restore that worked: the tool would have been fed a piece of a dump and made no complaint.
    if let Some(Err(unread)) = feeder.map(|feeder| feeder.join().unwrap_or(Ok(()))) {
        return Err(unread);
    }
    Ok(())
}

fn create_file(path: &str) -> Result<File, AppError> {
    File::create(path).map_err(|e| err!("error.cannotWriteFile", path = path, message = e))
}

fn open_file(path: &str) -> Result<File, AppError> {
    File::open(path).map_err(|e| err!("error.cannotReadFile", path = path, message = e))
}

/// How far a transfer has got.
///
/// `percent` deliberately never reaches 100: the command returning is what says the work is
/// finished, and one fact should have one teller. A dump's figure is an estimate — see [`Tracker`]
/// for what it is made of — where a restore's is a count of the bytes actually handed over.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// 0 to 99, or `None` when there is no honest percentage to give — the bar shows movement
    /// instead of a number.
    pub percent: Option<u8>,
    /// What is being written out at this moment, for the line under the bar — a table of a MySQL
    /// database, a collection of a MongoDB one. A restore has none: what it is replaying is a
    /// stream, not a list of anything.
    pub part: Option<String>,
    /// Which of them that is, counted from one — the one in hand rather than the ones behind it,
    /// since "table 1 of 12" is what a reader takes "12 tables, working" to mean. Zero when there
    /// is none to be on, which is a restore and the moment before a dump reaches its first.
    pub at_part: u32,
    pub parts: u32,
}

/// Where a transfer says how far it has got, four times a second.
pub struct Watch<'a> {
    /// `Send + Sync` even though the three child-process-driven dumps below never need it — their
    /// own `Watch` never crosses an `.await`, confined to a `spawn_blocking` closure instead. The
    /// ClickHouse dump/restore in `clickhouse_dump.rs` runs as a plain `async fn` with no child
    /// process to block on, so it holds a `&Watch` across real `.await` points — which tauri only
    /// accepts from a command whose whole future is `Send`. Every closure passed in today already
    /// satisfies this (an `AppHandle`-capturing reporter, an `Arc<AtomicBool>`-capturing cancel
    /// flag are both `Send + Sync` on their own), so this costs the other three nothing.
    pub report: &'a (dyn Fn(Progress) + Send + Sync),
    /// Asked four times a second, and again after every line the tool writes: has the transfer
    /// been called off?
    ///
    /// A poll rather than a signal because the work is a blocking loop around a child process,
    /// and the only thing that reliably stops one of those is killing it. What sets it is the tab
    /// closing, `disconnect_db`, or the Cancel button — see `DbState::transfers`.
    pub cancel: &'a (dyn Fn() -> bool + Send + Sync),
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

/// How many bytes of file a byte of table is assumed to turn into, until any actually have.
///
/// `DATA_LENGTH` counts whole pages, including the room InnoDB leaves for rows to grow into, so a
/// table's share of the file usually comes out somewhat smaller than its share of the server's
/// disk. Only ever used to move the bar *within* a table that has not finished yet, and replaced by
/// the observed figure as soon as one has.
const ASSUMED_RATIO: f64 = 0.8;

/// How big a table or collection has to be before what it wrote is worth learning the ratio from.
///
/// A table's weight is rounded up to whole pages and never reads as less than one, so an empty
/// table weighs 16KB and writes a line and a half of SQL. Left in, those are what the ratio would
/// mostly be made of — and one of them, seen first, would put every table after it at eleven times
/// its true share.
const CALIBRATE_FROM: u64 = 1 << 20;

/// A dump tool's commentary and the growing file, turned into a percentage.
///
/// Neither mysqldump nor mongodump reports progress in a form worth reading — the one says nothing
/// at all unless told to be verbose, the other draws a bar every three seconds. What both will do
/// is name each table or collection as they reach it, so the ones behind are known; and weighing
/// each by what its rows or documents take on the server makes that a percentage rather than a
/// count, which would jump a third of the way along at a table holding a thousandth of the rows.
///
/// That alone would sit still through the one huge table most databases have, so the size of the
/// file being written is read as well and used to place the dump inside the one it is on.
pub(super) struct Tracker {
    /// What each table or collection weighs, by name.
    weights: HashMap<String, u64>,
    total: u64,
    /// Where the dump is being written, read for its size to see inside the current part.
    path: PathBuf,
    /// Whether that reading means anything: a structure-only dump writes much the same few hundred
    /// bytes per table however many rows it has, so there the count is the whole story.
    rows: bool,
    /// What the parts already written weighed.
    done: u64,
    /// The part being written: its name, its weight, and how big the file was when it began.
    current: Option<(String, u64, u64)>,
    parts_done: u32,
    parts: u32,
    /// The bytes written for the parts that have finished, against what those weighed — the
    /// observed form of [`ASSUMED_RATIO`].
    written: u64,
    weighed: u64,
    /// Never allowed to fall. The estimate is revised as the ratio settles, and a bar that goes
    /// backwards reads as a mistake rather than as a correction.
    percent: f64,
}

impl Tracker {
    pub(super) fn new(parts: &[(String, u64)], path: &str, rows: bool) -> Self {
        let mut weights: HashMap<String, u64> = parts.iter().cloned().collect();
        let mut total: u64 = weights.values().sum();
        let rows = rows && total > 0;
        // Two dumps have no use for what the rows weigh: one that is not writing any, and one whose
        // tables are all empty — which weigh nothing between them and would leave the bar with
        // nothing to fill against. Both fall back to counting the parts, which is coarse but is
        // the whole of the work in either case.
        if !rows {
            for weight in weights.values_mut() {
                *weight = 1;
            }
            total = weights.len() as u64;
        }
        let parts = weights.len() as u32;
        Self {
            weights,
            total,
            path: PathBuf::from(path),
            rows,
            done: 0,
            current: None,
            parts_done: 0,
            parts,
            written: 0,
            weighed: 0,
            percent: 0.0,
        }
    }

    fn size(&self) -> u64 {
        std::fs::metadata(&self.path)
            .map(|meta| meta.len())
            .unwrap_or(0)
    }

    /// The tool has reached `part`, which is also to say it has finished the one before it.
    pub(super) fn reached(&mut self, part: &str) {
        let size = self.size();
        if let Some((_, weight, from)) = self.current.take() {
            self.done += weight;
            self.parts_done += 1;
            if weight >= CALIBRATE_FROM {
                self.written += size.saturating_sub(from);
                self.weighed += weight;
            }
        }
        // A view weighs nothing and is in no list of tables, being no table: let the total grow
        // rather than report a dump that has passed more of them than it has.
        self.parts = self.parts.max(self.parts_done + 1);
        let weight = self.weights.get(part).copied().unwrap_or(0);
        self.current = Some((part.to_string(), weight, size));
    }

    pub(super) fn progress(&mut self) -> Progress {
        // Nothing was known about what the database holds, so there is nothing to be a fraction of.
        if self.total == 0 {
            return Progress {
                percent: None,
                part: self.current.as_ref().map(|(name, ..)| name.clone()),
                at_part: self.at_part(),
                parts: self.parts,
            };
        }

        let mut done = self.done as f64;
        if let Some((_, weight, from)) = &self.current {
            if !self.rows {
                // Counting parts: reaching one is as good as finishing it, since what a
                // structure-only dump writes for a table is a few hundred bytes and gone before the
                // next reading. Left out, a two-table dump would end at half.
                done += *weight as f64;
            } else if *weight > 0 {
                let ratio = if self.weighed > 0 {
                    // Held to a band either side of the assumption: one table that compresses or
                    // expands like nothing else in the database should move the estimate, not
                    // become it.
                    (self.written as f64 / self.weighed as f64).clamp(0.2, 4.0)
                } else {
                    ASSUMED_RATIO
                };
                // Never past the part's own share, however far off the ratio turns out to be.
                done += ((self.size().saturating_sub(*from) as f64) / ratio).min(*weight as f64);
            }
        }
        self.percent = self
            .percent
            .max((done / self.total as f64 * 100.0).clamp(0.0, 99.0));

        Progress {
            percent: (self.parts_done > 0 || self.current.is_some())
                .then_some(self.percent.round() as u8),
            part: self.current.as_ref().map(|(name, ..)| name.clone()),
            at_part: self.at_part(),
            parts: self.parts,
        }
    }

    /// The part in hand, counted from one — the ones behind it plus the one it is on.
    fn at_part(&self) -> u32 {
        self.parts_done + u32::from(self.current.is_some())
    }
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
/// `charset` is the character set the dump is transferred in — see `mysql_structure::dump_charset`
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
pub fn mysql_dump(
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
pub fn mysql_restore(
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
            None => (super::postgres::DEFAULT_SCHEMA, unquoted),
        },
    };
    Some(super::postgres::qualify(schema, table))
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
pub fn postgres_dump(
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
pub fn postgres_restore(
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

/// How much of a file has gone into the tool, as a percentage — and `None` for a file whose size
/// could not be read, which is a restore that runs without a number rather than one that refuses.
///
/// Stops at 99 however much has been sent: the client is still executing the last of what it was
/// given long after the last byte of it has been handed over.
fn share(sent: u64, total: u64) -> Option<u8> {
    if total == 0 {
        return None;
    }
    Some(((sent as f64 / total as f64) * 100.0).min(99.0) as u8)
}

/// Splits a MongoDB URI into the parts this module rewrites: what is before the host list, the
/// host list itself, the `/database` path, and the `?options`.
fn split_uri(uri: &str) -> Result<(String, String, String, String), AppError> {
    let (scheme, rest) = uri
        .split_once("://")
        .ok_or_else(|| err!("error.notMongoUri"))?;
    let (credentials, hosts_and_more) = match rest.rsplit_once('@') {
        Some((credentials, rest)) => (format!("{credentials}@"), rest),
        None => (String::new(), rest),
    };
    let (hosts_and_path, query) = match hosts_and_more.split_once('?') {
        Some((head, query)) => (head, format!("?{query}")),
        None => (hosts_and_more, String::new()),
    };
    let (hosts, path) = match hosts_and_path.split_once('/') {
        Some((hosts, path)) => (hosts, format!("/{path}")),
        None => (hosts_and_path, String::new()),
    };
    Ok((
        format!("{scheme}://{credentials}"),
        hosts.to_string(),
        path,
        query,
    ))
}

/// The URI the tools are given: the database path dropped, since the database is named separately
/// and the tools refuse to be told twice, and the host replaced when the connection is tunneled.
///
/// The `/` that stood before the dropped database stays: a URI whose options follow the host list
/// with nothing between them is one the tools refuse to parse at all.
fn tool_uri(uri: &str, endpoint: Option<(&str, u16)>) -> Result<String, AppError> {
    let (head, hosts, _path, query) = split_uri(uri)?;
    let Some((host, port)) = endpoint else {
        return Ok(format!("{head}{hosts}/{query}"));
    };
    if head.starts_with("mongodb+srv://") {
        return Err(err!("error.srvOverTunnel"));
    }
    // Only the one host is forwarded, so the tools must talk to it directly rather than discover
    // the replica set's own addresses and try to reach those.
    let separator = if query.is_empty() { "?" } else { "&" };
    Ok(format!(
        "{head}{host}:{port}/{query}{separator}directConnection=true"
    ))
}

/// Writes `database` to `path` as a mongodump archive: the whole database, collections and
/// indexes, in one file rather than a directory of BSON.
///
/// `documents` is what `$collStats` says the database's documents weigh, and is where the progress
/// reported through `watch` comes from — see [`archive_share`]. Zero when the server would not say,
/// which costs the dump nothing but its percentage.
pub fn mongo_dump(
    tool: &Path,
    uri: &str,
    endpoint: Option<(&str, u16)>,
    database: &str,
    path: &str,
    documents: u64,
    watch: &Watch<'_>,
) -> Result<(), AppError> {
    let args = vec![
        format!("--uri={}", tool_uri(uri, endpoint)?),
        format!("--db={database}"),
        format!("--archive={path}"),
    ];
    let archive = PathBuf::from(path);
    run(
        Invocation {
            tool,
            args: &args,
            env: &[],
            stdin: None,
            stdout: None,
            name: "mongodump",
        },
        watch,
        |_| {
            let written = std::fs::metadata(&archive)
                .map(|meta| meta.len())
                .unwrap_or(0);
            (watch.report)(Progress {
                percent: archive_share(written, documents),
                ..Progress::default()
            });
        },
    )
}

/// How big a mongodump archive comes out against what the documents in it weigh.
///
/// One, as near as makes no difference: an archive is those documents' own BSON, which is exactly
/// what `$collStats` measures, and the headers around them are a few hundred bytes per namespace.
/// Measured against this server twice — 200,066,402 bytes of archive for 200,065,450 of documents,
/// and 50,018,402 for 50,017,450. Named rather than left implicit because it is an assumption about
/// a file format, and the one place to correct if a future mongodump stops holding to it.
const ARCHIVE_PER_DOCUMENT: f64 = 1.0;

/// How far through the archive a dump is, from the bytes on disk against what the documents weigh.
///
/// This is the whole of mongodump's progress, and it is deliberately not read out of what mongodump
/// says. Its own account is a bar drawn every three seconds, and it dumps four collections at once
/// unless told otherwise — so the collection it names is one of several in flight, and counting
/// them would report a dump further along than it is. The file on disk is under no such doubt.
///
/// It does arrive in steps rather than smoothly: mongodump holds about 16MB back before writing,
/// so a 200MB dump climbs in a dozen jumps and a small one in two or three. The bigger the dump the
/// less that shows, which is the right way round — a dump small enough for the steps to be coarse
/// is over in seconds.
fn archive_share(written: u64, documents: u64) -> Option<u8> {
    if documents == 0 {
        return None;
    }
    share(written, (documents as f64 * ARCHIVE_PER_DOCUMENT) as u64)
}

/// The magic number every mongodump archive starts with.
const ARCHIVE_MAGIC: u32 = 0x8199_e26d;

/// The database a mongodump archive holds, read out of the archive itself.
///
/// An archive begins with its magic number, then a header document, then one metadata document per
/// collection — and each of those names the database it came from. Only the first is read: an
/// archive written by MixLab holds one database, being dumped with `--db`.
///
/// This is needed because `mongorestore` puts documents back into the namespaces the archive
/// names, and the only way to send them somewhere else is to tell it what to rename *from*.
fn archive_database(path: &str) -> Result<String, AppError> {
    use mongodb::bson::Document;
    use std::io::Read;

    let mut file = open_file(path)?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic)
        .map_err(|e| err!("error.cannotReadFile", path = path, message = e))?;
    if u32::from_le_bytes(magic) != ARCHIVE_MAGIC {
        return Err(err!("error.notMongoArchive", path = path));
    }
    let unreadable = |e: mongodb::bson::de::Error| {
        err!("error.archiveDatabaseUnreadable", path = path, message = e)
    };
    // The header, which carries versions rather than namespaces.
    Document::from_reader(&mut file).map_err(unreadable)?;
    let metadata = Document::from_reader(&mut file).map_err(unreadable)?;
    metadata
        .get_str("db")
        .map(str::to_string)
        .map_err(|_| err!("error.archiveNamesNoDatabase", path = path))
}

/// Restores a mongodump archive into `database`, whatever database it was dumped from.
///
/// The rename is what makes the choice in the sidebar mean something: without it the documents go
/// back where they came from, which for an archive from another server is rarely where they are
/// wanted.
///
/// The archive is poured into mongorestore rather than named to it — `--archive` with no value is
/// how it is told to read one from its standard input. That costs nothing, since it reads the
/// archive from end to end either way, and it is what makes the progress reported through `watch` a
/// count of bytes actually handed over rather than anything inferred.
pub fn mongo_restore(
    tool: &Path,
    uri: &str,
    endpoint: Option<(&str, u16)>,
    database: &str,
    path: &str,
    watch: &Watch<'_>,
) -> Result<(), AppError> {
    let mut args = vec![
        format!("--uri={}", tool_uri(uri, endpoint)?),
        "--archive".to_string(),
    ];
    let source = archive_database(path)?;
    if source != database {
        // Every collection of the one database, and only that database: an archive holding more
        // than the dump wrote would otherwise have the rest restored under their own names.
        args.push(format!("--nsInclude={source}.*"));
        args.push(format!("--nsFrom={source}.*"));
        args.push(format!("--nsTo={database}.*"));
    }

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
            name: "mongorestore",
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
mod tests {
    use super::split_uri;
    use super::{pg_reached_table, pg_rewrite_preamble, run, tool_uri, Invocation, Watch};

    /// Where a MongoDB URI comes apart, and the two characters that decide it wrongly if the
    /// splitting is done from the left.
    ///
    /// A password may hold an `@` — it is percent-encoded in a well-formed URI, but the box this
    /// comes from takes what the user pastes — so the host list starts after the *last* `@`, not
    /// the first. And the `?` that starts the options is found before the `/` that starts the
    /// path, since an option value may contain a slash (`tlsCAFile=/etc/ca.pem`).
    #[test]
    fn a_mongo_uri_comes_apart_at_the_right_characters() {
        let (head, hosts, path, query) = split_uri("mongodb://db.example:27017").unwrap();
        assert_eq!(
            (head.as_str(), hosts.as_str()),
            ("mongodb://", "db.example:27017")
        );
        assert_eq!((path.as_str(), query.as_str()), ("", ""));

        let (head, hosts, path, query) =
            split_uri("mongodb://root:pw@a:27017,b:27017/shop?replicaSet=rs0").unwrap();
        assert_eq!(head, "mongodb://root:pw@");
        assert_eq!(hosts, "a:27017,b:27017");
        assert_eq!(path, "/shop");
        assert_eq!(query, "?replicaSet=rs0");

        // An `@` in the password: split from the left, `pw` would be the whole credential and
        // `me.com:pw2@host` the host list.
        let (head, hosts, ..) = split_uri("mongodb://root:p@ssw0rd@db.example/shop").unwrap();
        assert_eq!(head, "mongodb://root:p@ssw0rd@");
        assert_eq!(hosts, "db.example");

        // A `/` inside an option value belongs to the options, not to a path.
        let (_, hosts, path, query) =
            split_uri("mongodb://db.example/?tlsCAFile=/etc/ssl/ca.pem").unwrap();
        assert_eq!(hosts, "db.example");
        assert_eq!(path, "/");
        assert_eq!(query, "?tlsCAFile=/etc/ssl/ca.pem");

        // The other scheme, which is the whole reason the scheme is kept rather than rebuilt.
        let (head, ..) = split_uri("mongodb+srv://root:pw@cluster.example/shop").unwrap();
        assert_eq!(head, "mongodb+srv://root:pw@");

        assert!(split_uri("db.example:27017").is_err());
    }

    /// Where the database name goes on a MySQL command line, and what stands in front of it.
    ///
    /// `my_getopt` reads any argument beginning with `-` as an option wherever it appears, so a
    /// database named `-x` used to be swallowed as one — and mysqldump's short options are mostly
    /// flags, which means it is not even rejected: the dump runs, on the wrong terms, over no
    /// database at all. A bare `--` ends the options in both MySQL's parser and MariaDB's.
    #[test]
    fn a_mysql_database_name_is_passed_as_a_name_and_not_as_an_option() {
        for args in [
            mysqldump_args(
                Path::new("opts.cnf"),
                "utf8mb4",
                false,
                true,
                DumpMode::All,
                "-x",
            ),
            mysql_restore_args(Path::new("opts.cnf"), "-x"),
        ] {
            assert_eq!(args.last().map(String::as_str), Some("-x"), "{args:?}");
            assert_eq!(args[args.len() - 2], "--", "{args:?}");
            // And exactly one: a second would be handed to the tool as a table name.
            assert_eq!(
                args.iter().filter(|arg| *arg == "--").count(),
                1,
                "{args:?}"
            );
        }
    }

    /// The credentials file is the only option file the tools read, first on the line. As an
    /// extra file it lost to a `password=` left in the user's `~/.my.cnf`, and the dump connected
    /// with no password — see [`OptionFile`].
    #[test]
    fn the_mysql_tools_read_the_credentials_file_alone() {
        for args in [
            mysqldump_args(
                Path::new("opts.cnf"),
                "utf8mb4",
                false,
                true,
                DumpMode::All,
                "db",
            ),
            mysql_restore_args(Path::new("opts.cnf"), "db"),
        ] {
            assert_eq!(args[0], "--defaults-file=opts.cnf", "{args:?}");
            assert!(
                !args
                    .iter()
                    .any(|arg| arg.starts_with("--defaults-extra-file")),
                "{args:?}"
            );
        }
    }

    /// The switches that depend on which tool and which server this is. MariaDB's mysqldump
    /// refuses the whole run over either of the two MySQL-only ones — see `is_mariadb_tool`.
    #[test]
    fn mysqldump_only_asks_mysqls_own_tool_for_mysql_only_options() {
        let mysql = mysqldump_args(Path::new("o"), "utf8mb4", false, false, DumpMode::All, "db");
        assert!(mysql.iter().any(|arg| arg == "--set-gtid-purged=OFF"));
        assert!(mysql.iter().any(|arg| arg == "--column-statistics=0"));

        // A server that does have the histogram table is not told to skip it.
        let has_stats = mysqldump_args(Path::new("o"), "utf8mb4", false, true, DumpMode::All, "db");
        assert!(!has_stats.iter().any(|arg| arg == "--column-statistics=0"));

        let mariadb = mysqldump_args(Path::new("o"), "utf8mb4", true, false, DumpMode::All, "db");
        assert!(!mariadb
            .iter()
            .any(|arg| arg.starts_with("--set-gtid-purged")));
        assert!(!mariadb
            .iter()
            .any(|arg| arg.starts_with("--column-statistics")));
    }

    /// The database reaches the PostgreSQL tools through the environment, and nowhere else.
    ///
    /// libpq expands a `dbname` that holds an `=` — or that starts `postgresql://` — into a whole
    /// connection string, so a name like `dbname=other` on the command line does not open a
    /// database of that name, it opens whichever server and database the name spells out. For a
    /// restore that is a file written into a database nobody chose. `PGDATABASE` is read as a
    /// plain name.
    #[test]
    fn the_postgres_database_never_reaches_the_command_line() {
        let hostile = "dbname=elsewhere host=attacker.example";
        for args in [
            pg_dump_args("localhost", 5432, "u", DumpMode::All),
            psql_args("localhost", 5432, "u"),
        ] {
            assert!(
                !args.iter().any(|arg| arg.contains("--dbname")),
                "the database is back on the command line: {args:?}",
            );
            assert!(!args.iter().any(|arg| arg.contains(hostile)), "{args:?}");
        }

        let pgpass = PgPassFile::new("localhost", 5432, "u", "p").unwrap();
        let env = pgpass.env(hostile);
        assert_eq!(
            env.iter()
                .find(|(key, _)| *key == "PGDATABASE")
                .map(|(_, value)| value.as_str()),
            Some(hostile),
        );
    }
    use super::{
        mysql_restore_args, mysqldump_args, pg_dump_args, psql_args, DumpMode, PgPassFile,
    };
    use std::path::Path;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Instant;

    /// Not a test: the child the two tests below need. Ignored so it never runs on its own, and
    /// spawned by name from `current_exe` so those tests depend on no program the machine might
    /// not have — `sleep` is not on Windows, and `ping` is not guaranteed in a container.
    #[test]
    #[ignore]
    fn sleeper() {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }

    /// That child, as a tool `run` can be pointed at. It outlives any wait a test is willing to
    /// make, so the only way it finishes is by being killed.
    fn slow_tool() -> (PathBuf, Vec<String>) {
        let args = [
            "--exact",
            "modules::db::drivers::dump::tests::sleeper",
            "--ignored",
        ];
        (
            std::env::current_exe().expect("the test binary knows where it is"),
            args.iter().map(|a| a.to_string()).collect(),
        )
    }

    /// The whole of what R29 was about: a transfer that is called off stops the tool, rather than
    /// leaving it to run to completion for a connection that has gone.
    #[test]
    fn a_cancelled_run_kills_the_tool_instead_of_waiting_for_it() {
        let (tool, args) = slow_tool();
        // Set before the run starts, so the first poll — a quarter of a second in — sees it.
        let cancel: Arc<AtomicBool> = Arc::new(AtomicBool::new(true));
        let flag = Arc::clone(&cancel);
        let watch = Watch {
            report: &|_| {},
            cancel: &|| flag.load(Ordering::Relaxed),
        };

        let started = Instant::now();
        let result = run(
            Invocation {
                tool: &tool,
                args: &args,
                env: &[],
                stdin: None,
                stdout: None,
                name: "sleeper",
            },
            &watch,
            |_| {},
        );

        assert!(
            result.is_err(),
            "a killed tool is not a transfer that worked"
        );
        assert!(
            started.elapsed().as_secs() < 10,
            "it waited the full minute out instead of killing the tool: {:?}",
            started.elapsed()
        );
    }

    /// And the other half: a run nobody stops is not cut short by the polling, and the tool's own
    /// success is still what the answer is read from.
    #[test]
    fn a_run_nobody_stops_is_left_alone() {
        // `--list` exits at once and exits cleanly, which is all this needs of it.
        let tool = std::env::current_exe().expect("the test binary knows where it is");
        let watch = Watch {
            report: &|_| {},
            cancel: &|| false,
        };
        let args = vec!["--list".to_string()];
        assert!(run(
            Invocation {
                tool: &tool,
                args: &args,
                env: &[],
                stdin: None,
                stdout: None,
                name: "list"
            },
            &watch,
            |_| {},
        )
        .is_ok());
    }

    /// The preamble pg_dump 17 and later writes, whatever the version of the server it read.
    const PREAMBLE: &str = "--\n-- PostgreSQL database dump\n--\n\n\\restrict S0j3INb3aLmWPSh\n\n\
                            -- Dumped from database version 15.18\n\
                            -- Dumped by pg_dump version 18.6\n\n\
                            SET statement_timeout = 0;\n\
                            SET lock_timeout = 0;\n\
                            SET transaction_timeout = 0;\n\
                            SET client_encoding = 'UTF8';\n";

    /// The one line a server older than 17 refuses is swapped for a block it can refuse quietly,
    /// and everything above it is handed over untouched.
    #[test]
    fn guards_the_transaction_timeout_a_dump_sets() {
        let lead = pg_rewrite_preamble(PREAMBLE.as_bytes()).expect("a lead");
        let bytes = String::from_utf8(lead.bytes).unwrap();
        let cut = PREAMBLE.find("SET transaction_timeout").unwrap();
        assert_eq!(&bytes[..cut], &PREAMBLE[..cut]);
        assert!(bytes[cut..].starts_with("DO $$ BEGIN PERFORM"), "{bytes}");
        // What is replaced is the original line, so that the rest of the file follows on from it.
        assert_eq!(
            lead.replaced as usize,
            cut + "SET transaction_timeout = 0;\n".len()
        );
    }

    /// A dump from pg_dump 16 or older never says it, and there is nothing to rewrite.
    #[test]
    fn leaves_a_dump_without_the_line_alone() {
        let older = PREAMBLE.replace("SET transaction_timeout = 0;\n", "");
        assert!(pg_rewrite_preamble(older.as_bytes()).is_none());
    }

    /// The walk stops at the first line that is not part of a preamble, so a row of data reading
    /// like the line is left as it is — a restore may not edit what it is restoring.
    #[test]
    fn stops_before_the_data() {
        let dump = "--\n-- PostgreSQL database dump\n--\n\n\
                    COPY public.notes (body) FROM stdin;\n\
                    SET transaction_timeout = 0;\n\\.\n";
        assert!(pg_rewrite_preamble(dump.as_bytes()).is_none());
    }

    /// A preamble cut off mid-line is one this side cannot read to the end of, and a half-line is
    /// not enough to say what it is.
    #[test]
    fn ignores_a_line_the_read_cut_in_half() {
        let cut = &PREAMBLE[..PREAMBLE.find("SET transaction_timeout").unwrap() + 12];
        assert!(pg_rewrite_preamble(cut.as_bytes()).is_none());
    }

    /// Every wording of the line across the pg_dump versions this app can meet, each read back as
    /// the name the rest of the app uses — which is what the weights are keyed by.
    #[test]
    fn reads_the_table_pg_dump_has_reached_however_it_is_worded() {
        // 12 and later, with the program's name in front of it.
        assert_eq!(
            pg_reached_table("pg_dump: dumping contents of table \"public.users\"").as_deref(),
            Some("users")
        );
        // Older: the two halves quoted separately.
        assert_eq!(
            pg_reached_table("pg_dump: dumping contents of table \"sales\".\"orders\"").as_deref(),
            Some("sales.orders")
        );
        // Older still: no quotes and no schema.
        assert_eq!(
            pg_reached_table("dumping contents of table users").as_deref(),
            Some("users")
        );
        // A table whose own name holds a dot: the split is at the first one, and the name comes
        // back quoted exactly as the sidebar writes it.
        assert_eq!(
            pg_reached_table("pg_dump: dumping contents of table \"public.odd.name\"").as_deref(),
            Some("\"odd.name\"")
        );
    }

    #[test]
    fn everything_else_pg_dump_says_is_not_a_table() {
        assert_eq!(
            pg_reached_table("pg_dump: last built-in OID is 16383"),
            None
        );
        assert_eq!(pg_reached_table("pg_dump: reading extensions"), None);
        assert_eq!(pg_reached_table(""), None);
    }

    /// The database is dropped from the path but its `/` is not: without it the tools refuse the
    /// URI outright ("must have a / before the query").
    #[test]
    fn drops_the_database_but_keeps_the_slash() {
        assert_eq!(
            tool_uri(
                "mongodb://user:pw@db.example:27017/shop?authSource=admin",
                None
            )
            .unwrap(),
            "mongodb://user:pw@db.example:27017/?authSource=admin"
        );
        assert_eq!(
            tool_uri("mongodb://db.example:27017", None).unwrap(),
            "mongodb://db.example:27017/"
        );
        assert_eq!(
            tool_uri("mongodb://db.example:27017/shop", None).unwrap(),
            "mongodb://db.example:27017/"
        );
    }

    /// A tunnel replaces the host list, and the one forwarded node has to be talked to directly —
    /// discovering the replica set would hand back addresses the tunnel does not reach.
    #[test]
    fn points_at_the_tunnel() {
        assert_eq!(
            tool_uri(
                "mongodb://user:pw@db.example:27017/shop",
                Some(("127.0.0.1", 5001))
            )
            .unwrap(),
            "mongodb://user:pw@127.0.0.1:5001/?directConnection=true"
        );
        assert_eq!(
            tool_uri(
                "mongodb://a:27017,b:27017/?replicaSet=rs0",
                Some(("127.0.0.1", 5001))
            )
            .unwrap(),
            "mongodb://127.0.0.1:5001/?replicaSet=rs0&directConnection=true"
        );
    }

    /// The archive layout this reads: the magic number, the header, then a metadata document per
    /// collection — the first of which names the database everything in the file came from.
    #[test]
    fn reads_the_database_out_of_an_archive() {
        use mongodb::bson::doc;

        let mut archive = super::ARCHIVE_MAGIC.to_le_bytes().to_vec();
        doc! { "concurrent_collections": 4, "version": "0.1" }
            .to_writer(&mut archive)
            .unwrap();
        doc! { "db": "pnedu_portal", "collection": "users", "size": 10_i64 }
            .to_writer(&mut archive)
            .unwrap();
        let path =
            std::env::temp_dir().join(format!("mixlab-test-{}.archive", uuid::Uuid::new_v4()));
        std::fs::write(&path, &archive).unwrap();

        let found = super::archive_database(&path.to_string_lossy());
        let not_an_archive = super::archive_database(file!());
        std::fs::remove_file(&path).unwrap();

        assert_eq!(found.unwrap(), "pnedu_portal");
        assert!(not_an_archive.is_err());
    }

    #[test]
    fn refuses_an_srv_uri_over_a_tunnel() {
        assert!(tool_uri(
            "mongodb+srv://user:pw@cluster.example/shop",
            Some(("127.0.0.1", 5001))
        )
        .is_err());
    }

    /// Real lines from `mysqldump --verbose`, of which exactly one kind says a table has been
    /// reached. Everything else it says is chatter the count must not move for.
    #[test]
    fn reads_the_table_out_of_the_commentary() {
        assert_eq!(
            super::reached_table("-- Retrieving table structure for table orders..."),
            Some("orders")
        );
        assert_eq!(super::reached_table("-- Retrieving rows..."), None);
        assert_eq!(
            super::reached_table("-- Rolling back to savepoint sp..."),
            None
        );
        assert_eq!(
            super::reached_table("-- Connecting to 192.168.1.1..."),
            None
        );
        assert_eq!(
            super::reached_table("mysqldump: Got error: 1049: Unknown database 'shop'"),
            None
        );
    }

    /// A dump's failure has to be readable through the commentary this module asked for: the lines
    /// mysqldump writes about its own progress are held back, and the one that says what went wrong
    /// is what the error carries.
    #[test]
    fn keeps_the_error_out_of_the_commentary() {
        let mut tail = super::Tail::default();
        for line in [
            "-- Connecting to db.example...",
            "-- Retrieving table structure for table orders...",
            "",
            "mysqldump: Got error: 1044: Access denied for user 'app'@'%'",
        ] {
            tail.push(line.to_string());
        }
        assert_eq!(
            tail.message(),
            "mysqldump: Got error: 1044: Access denied for user 'app'@'%'"
        );

        // A failure with nothing but commentary to show still has to say something.
        let mut nothing_else = super::Tail::default();
        nothing_else.push("-- Connecting to db.example...".to_string());
        assert_eq!(nothing_else.message(), "-- Connecting to db.example...");
    }

    /// A tool's standard error is not promised to be UTF-8: these tools write what the server told
    /// them in whatever character set the connection is using, and a dump's is the database's own.
    /// The line that says what went wrong comes last, so it only survives if a line that will not
    /// decode is stepped over rather than treated as the end of the output.
    #[test]
    fn reads_past_a_line_that_will_not_decode() {
        let mut stderr = b"-- Connecting to db.example...\n".to_vec();
        // A table name with an accent in it, from a latin1 database.
        stderr.extend_from_slice(b"-- Retrieving table structure for table h\xf3a_don...\n");
        stderr.extend_from_slice(b"mysqldump: Got error: 1044: Access denied\r\n");

        let mut source = &stderr[..];
        let mut buffer = Vec::new();
        let mut lines = Vec::new();
        while let Some(line) = super::next_line(&mut source, &mut buffer) {
            lines.push(line);
        }

        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "-- Connecting to db.example...");
        // The byte that would not decode is stood in for, and the rest of its line is still there.
        assert!(lines[1].ends_with("_don..."));
        // The one this is all for: it comes after the line that would not decode.
        assert_eq!(lines[2], "mysqldump: Got error: 1044: Access denied");
    }

    /// One temporary file, grown to order, standing in for the dump being written.
    struct Written(std::path::PathBuf);

    impl Written {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("mixlab-test-{}.sql", uuid::Uuid::new_v4()));
            std::fs::write(&path, b"").unwrap();
            Self(path)
        }

        fn grow_to(&self, bytes: usize) {
            std::fs::write(&self.0, vec![b'x'; bytes]).unwrap();
        }

        fn path(&self) -> String {
            self.0.to_string_lossy().into_owned()
        }
    }

    impl Drop for Written {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// The reckoning behind the bar: a table's share is what its rows weigh on the server, and
    /// where the dump has got to inside the table it is on is read from the file it is writing.
    #[test]
    fn weighs_each_table_by_what_its_rows_take() {
        const MB: u64 = 1 << 20;
        let file = Written::new();
        let tables = vec![
            ("small".to_string(), 4 * MB),
            ("large".to_string(), 12 * MB),
        ];
        let mut tracker = super::Tracker::new(&tables, &file.path(), true);

        // Nothing written yet, and the first table only just reached.
        tracker.reached("small");
        let start = tracker.progress();
        assert_eq!(start.percent, Some(0));
        assert_eq!(start.part.as_deref(), Some("small"));
        assert_eq!((start.at_part, start.parts), (1, 2));

        // Halfway through the first table's bytes, at the ratio assumed before any are known: half
        // of a table that is a quarter of the database.
        file.grow_to((2 * MB) as usize * 4 / 5);
        assert_eq!(tracker.progress().percent, Some(12));

        // The whole of it, and the next table begun — the first table's share, and no more, however
        // much of the file it turned out to take.
        file.grow_to((6 * MB) as usize);
        tracker.reached("large");
        let second = tracker.progress();
        assert_eq!(second.percent, Some(25));
        assert_eq!((second.at_part, second.parts), (2, 2));

        // A third of the way through the larger table — measured at the ratio the first one turned
        // out to have, six bytes of file to four of table, rather than at the assumed one.
        file.grow_to((12 * MB) as usize);
        assert_eq!(tracker.progress().percent, Some(50));

        // A table that overruns what it was thought to weigh does not take the bar past its share,
        // and 100 is never claimed: the command returning is what says the dump is finished.
        file.grow_to((400 * MB) as usize);
        assert_eq!(tracker.progress().percent, Some(99));
    }

    /// A view weighs nothing and is in no list of tables — mysqldump writes one out all the same,
    /// and the count has to make room for it rather than report five tables of four.
    #[test]
    fn makes_room_for_what_it_was_not_told_about() {
        let file = Written::new();
        let tables = vec![("orders".to_string(), 1 << 20)];
        let mut tracker = super::Tracker::new(&tables, &file.path(), true);

        tracker.reached("orders");
        tracker.reached("recent_orders");
        let progress = tracker.progress();
        assert_eq!(progress.part.as_deref(), Some("recent_orders"));
        assert_eq!((progress.at_part, progress.parts), (2, 2));
    }

    /// A structure-only dump writes much the same for a table whatever it holds, so its bar counts
    /// tables — weighing them by rows it is not writing would stall on the big one and then leap.
    ///
    /// The table it is on counts as written: a few hundred bytes go out for one and the next
    /// reading is a quarter of a second later, so waiting for the next table to say this one is
    /// done would leave a two-table dump ending at half.
    #[test]
    fn counts_the_tables_when_the_rows_are_not_being_written() {
        let file = Written::new();
        let tables = vec![
            ("small".to_string(), 1 << 10),
            ("large".to_string(), 1 << 30),
        ];
        let mut tracker = super::Tracker::new(&tables, &file.path(), false);

        tracker.reached("small");
        assert_eq!(tracker.progress().percent, Some(50));
        tracker.reached("large");
        assert_eq!(tracker.progress().percent, Some(99));
    }

    /// A restore's figure is a count rather than an estimate, and the only thing it has to be
    /// careful about is the end: the client goes on executing after the last byte has gone in.
    #[test]
    fn counts_a_restore_out_of_the_file_it_is_fed() {
        assert_eq!(super::share(0, 16), Some(0));
        assert_eq!(super::share(8, 16), Some(50));
        assert_eq!(super::share(16, 16), Some(99));
        // A file whose size could not be read: no number to give, but the restore still runs.
        assert_eq!(super::share(0, 0), None);
    }

    /// A mongodump archive is the documents' own BSON with little else in it, so what is on disk
    /// against what `$collStats` says they weigh is a fair reading of how far along the dump is.
    #[test]
    fn measures_an_archive_against_what_its_documents_weigh() {
        const MB: u64 = 1 << 20;
        assert_eq!(super::archive_share(0, 100 * MB), Some(0));
        assert_eq!(super::archive_share(50 * MB, 100 * MB), Some(50));
        // Overrunning what it was expected to come to does not take the bar past the end, and 100
        // is never claimed: the command returning is what says the dump is finished.
        assert_eq!(super::archive_share(500 * MB, 100 * MB), Some(99));
        // A server that would not say what its collections hold: no number to give, but the dump
        // still runs and the bar still moves.
        assert_eq!(super::archive_share(50 * MB, 0), None);
    }

    /// Nothing known about the database's tables — a user `information_schema` shows nothing to —
    /// leaves no honest percentage to give, but the dump still runs and still says where it is.
    #[test]
    fn gives_no_percentage_it_cannot_stand_behind() {
        let file = Written::new();
        let mut tracker = super::Tracker::new(&[], &file.path(), true);

        tracker.reached("orders");
        let progress = tracker.progress();
        assert_eq!(progress.percent, None);
        assert_eq!(progress.part.as_deref(), Some("orders"));
        assert_eq!(progress.parts, 1);
    }
}
