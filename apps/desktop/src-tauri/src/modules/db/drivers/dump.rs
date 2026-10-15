//! Backing up and restoring a whole database by driving the vendors' own command-line tools, and
//! what every engine's backup shares: the mode, the progress a transfer reports, and the way it is
//! called off. The tools themselves are asked in `mysql/dump.rs` (`mysqldump`/`mysql`),
//! `postgres/dump.rs` (`pg_dump`/`psql`) and `mongo/dump.rs` (`mongodump`/`mongorestore`); where
//! they come from is {@link super::tools}' business.
//!
//! Writing a dump by hand would mean reimplementing every corner of the three servers' output —
//! definers, triggers, routines, sequences, extensions, BSON types — and being wrong about one of
//! them is a restore that silently differs from the original.
//!
//! Nothing here interpolates a password into a command line where the tool offers another way:
//! on most systems the arguments of a running process are readable by any other process of the
//! same user. Each engine's file says which way it uses.

use crate::error::AppError;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
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

/// What the caller of [`run`] is shown while the tool runs: every line the tool writes to its
/// standard error as it arrives, and nothing at all every quarter second — so a caller watching
/// something other than the tool's own words, such as the size of the file it is writing, still has
/// somewhere to look from.
pub(super) enum Tick<'a> {
    Line(&'a str),
    Idle,
}

/// A file to be poured into a tool's standard input, counting what has gone in.
///
/// The count is the whole of a restore's progress: the mysql client has no idea how far through
/// the file it is, but the file is of a known size and every byte of it has to be handed over.
pub(super) struct Fed {
    pub(super) file: File,
    /// Where the file came from, for the error a file that cannot be read through has to raise.
    pub(super) path: String,
    /// How much of it the tool has been given, read by the caller's ticks from another thread.
    pub(super) sent: Arc<AtomicU64>,
    /// What goes in ahead of the file, in place of the start of it that `file` is already
    /// positioned past. Only the PostgreSQL restore ever has one; see `pg_rewrite_preamble` in
    /// `postgres/dump.rs`.
    pub(super) lead: Option<Lead>,
}

/// A rewritten start of a dump: `bytes` are handed to the tool in place of the file's first
/// `replaced` bytes.
pub(super) struct Lead {
    pub(super) bytes: Vec<u8>,
    pub(super) replaced: u64,
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
pub(super) struct Invocation<'a> {
    pub(super) tool: &'a Path,
    pub(super) args: &'a [String],
    pub(super) env: &'a [(&'a str, String)],
    /// The file to pour into the tool's standard input, for a restore. `None` for a dump, which
    /// writes rather than reads.
    pub(super) stdin: Option<Fed>,
    /// Where the tool's standard output goes, for a dump. `None` for the tools that write the file
    /// themselves.
    pub(super) stdout: Option<File>,
    /// What the tool is called in an error the user reads — `mysqldump`, `psql`.
    pub(super) name: &'a str,
}

pub(super) fn run(
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

pub(super) fn create_file(path: &str) -> Result<File, AppError> {
    File::create(path).map_err(|e| err!("error.cannotWriteFile", path = path, message = e))
}

pub(super) fn open_file(path: &str) -> Result<File, AppError> {
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
    /// `Send + Sync` even though the three child-process-driven dumps (`mysql/dump.rs`,
    /// `postgres/dump.rs`, `mongo/dump.rs`) never need it — their own `Watch` never crosses an
    /// `.await`, confined to a `spawn_blocking` closure instead. The
    /// ClickHouse dump/restore in `clickhouse/dump.rs` runs as a plain `async fn` with no child
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

/// How much of a file has gone into the tool, as a percentage — and `None` for a file whose size
/// could not be read, which is a restore that runs without a number rather than one that refuses.
///
/// Stops at 99 however much has been sent: the client is still executing the last of what it was
/// given long after the last byte of it has been handed over.
pub(super) fn share(sent: u64, total: u64) -> Option<u8> {
    if total == 0 {
        return None;
    }
    Some(((sent as f64 / total as f64) * 100.0).min(99.0) as u8)
}

#[cfg(test)]
mod tests;
