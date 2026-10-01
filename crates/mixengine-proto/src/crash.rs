//! What `mixengined` leaves behind when it hits a bug in itself. Roadmap task **T91**.
//!
//! **Every field here is one of five things**: a compile-time constant of the build that wrote it
//! (the location, the version, the target), a literal from `std` or `tokio` (the thread name), a
//! symbol name out of a backtrace, an offset into the executable, or the executable's build
//! identifier. None of them can hold a value from the home it was written in — not a project's
//! directory, not a site's name, not a password. That is what lets one of these be attached to a
//! public bug report without being read first. The last two arrived with format 2 (T91a,
//! [ADR 0060](../../../docs/decisions/0060-a-crash-report-carries-offsets-and-the-release-keeps-the-symbols.md)).
//!
//! **The panic message is deliberately not here.** It is `format!`-ed from whatever was in scope at
//! the moment of a bug, which is the one string in this product nobody reviewed: an `unwrap()` on
//! `mixengine_core::Error::Io` renders the path that error carries. It goes to `daemon.log`
//! instead, where it is on the user's own machine and beside the paths that log has always carried.
//!
//! **Not a redaction pass**, for the reason [`bundle_api`](crate::Part)'s own header gives about
//! one: a filter is a guess that a pattern matched, and it invites the next reader to believe a
//! file is filtered rather than clean. What this module owes instead is the field list below —
//! short enough to read in one screen, which is where the guarantee actually lives.
//!
//! Decided in
//! [ADR 0022](../../../docs/decisions/0022-a-crash-report-is-recorded-by-default-and-sent-by-nothing.md).

use crate::{DaemonVersion, Timestamp};

/// The number [`CrashReport::format`] carries, so a reader that does not know a shape stops rather
/// than guessing at one — [`MANIFEST_FORMAT`](crate::MANIFEST_FORMAT)'s reasoning.
///
/// **2 since T91a**: frames became offsets into the executable, the names moved to
/// [`CrashReport::symbols`], and [`CrashReport::build_id`] arrived. [`CrashReport::from_json`] still
/// reads a format-1 report.
pub const CRASH_FORMAT: u32 = 2;

/// One frame of the panicking thread, as a position in the executable.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CrashFrame {
    /// The frame's return address minus the executable's base, as `0x…` hex: the same on every
    /// machine that ran this build, whatever address-space randomisation chose. `scripts/symbolize.mjs`
    /// turns it back into a name with the release's symbol file.
    ///
    /// [`None`] for a frame outside the executable — `libc`, `ntdll`, a system framework. Nothing
    /// names the library it was in, because a module's name is a path on the machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<String>,
}

/// Where in **this repository's own source** a panic was raised.
///
/// [`file`](Self::file) is `std::panic::Location::file`, which is the path as it was written in the
/// source tree — `crates/mixengine-daemon/src/…` — and is a `&'static str` baked into the binary
/// rather than a directory on anybody's disk.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CrashLocation {
    /// The source file, as this repository spells it.
    pub file: String,

    /// The line.
    pub line: u32,

    /// The column.
    pub column: u32,
}

/// One panic, as much of it as may travel.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct CrashReport {
    /// [`CRASH_FORMAT`].
    pub format: u32,

    /// When the hook ran, which is within microseconds of the panic.
    pub recorded_at: Timestamp,

    /// What was running, and what it spoke.
    pub daemon: DaemonVersion,

    /// `std::env::consts::OS`.
    pub os: String,

    /// `std::env::consts::ARCH`.
    pub arch: String,

    /// The panicking thread's name, or [`None`] when it had none.
    ///
    /// Every thread name reachable here is a literal from `std` or `tokio` — nothing in this
    /// workspace names a thread — so this is one of the three safe kinds of field the module header
    /// lists rather than an exception to them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,

    /// Where it was raised, or [`None`] when `std` reported none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<CrashLocation>,

    /// The backtrace, innermost first, as offsets into the executable.
    ///
    /// Empty in a format-1 report, whose names are in [`symbols`](Self::symbols), and when the
    /// platform could not unwind — which leaves a location, still most of what a reader needs.
    pub frames: Vec<CrashFrame>,

    /// The backtrace as symbol names, where this build resolved them itself: a debug build, and
    /// every format-1 report.
    ///
    /// **Symbol names and nothing else**: the `at <path>:<line>` lines a rendered backtrace carries
    /// are the one place a build machine's directories appear, and they are dropped before a report
    /// is built. A release build resolves nothing in the hook and leaves this empty. **Not paired
    /// with [`frames`](Self::frames)**: the names come from `std`'s own capture, which starts in a
    /// different frame.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<String>,

    /// The executable's build identifier: the GNU build-id on Linux, `LC_UUID` on macOS, the
    /// CodeView GUID and age on Windows. A symbol file is matched against it, so a report is never
    /// read against the wrong build. [`None`] in a format-1 report, or when the platform could not
    /// read one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_id: Option<String>,
}

/// A report as format 1 wrote it, before T91a: frames were names.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CrashReportV1 {
    format: u32,
    recorded_at: Timestamp,
    daemon: DaemonVersion,
    os: String,
    arch: String,
    #[serde(default)]
    thread: Option<String>,
    #[serde(default)]
    location: Option<CrashLocation>,
    frames: Vec<String>,
}

/// Only the format, to decide which shape to read the rest as.
#[derive(serde::Deserialize)]
struct Format {
    format: u32,
}

impl CrashReport {
    /// A report from a file, in either format this build knows.
    ///
    /// **A format-1 report keeps `format: 1`**, with its names in [`symbols`](Self::symbols) and no
    /// frames, so whoever reads a bundle can tell which shape each report was written in.
    ///
    /// # Errors
    ///
    /// [`serde_json::Error`] when the bytes are not a report of a format this build knows, which
    /// includes a format newer than [`CRASH_FORMAT`].
    pub fn from_json(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        let format = serde_json::from_slice::<Format>(bytes)?.format;
        if format > CRASH_FORMAT {
            return Err(serde::de::Error::custom(format!(
                "crash report format {format} is newer than the {CRASH_FORMAT} this build reads"
            )));
        }
        if format != 1 {
            return serde_json::from_slice(bytes);
        }

        let old: CrashReportV1 = serde_json::from_slice(bytes)?;
        Ok(Self {
            format: old.format,
            recorded_at: old.recorded_at,
            daemon: old.daemon,
            os: old.os,
            arch: old.arch,
            thread: old.thread,
            location: old.location,
            frames: Vec::new(),
            symbols: old.frames,
            build_id: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProtocolVersion;

    fn sample() -> CrashReport {
        CrashReport {
            format: CRASH_FORMAT,
            recorded_at: Timestamp(1_757_000_000_000),
            daemon: DaemonVersion {
                version: "0.1.0".to_owned(),
                protocol: ProtocolVersion(1),
            },
            os: "windows".to_owned(),
            arch: "x86_64".to_owned(),
            thread: Some("tokio-runtime-worker".to_owned()),
            location: Some(CrashLocation {
                file: "crates/mixengine-daemon/src/services/mod.rs".to_owned(),
                line: 412,
                column: 9,
            }),
            frames: vec![
                CrashFrame {
                    offset: Some("0x1a2b3c".to_owned()),
                },
                CrashFrame { offset: None },
            ],
            symbols: Vec::new(),
            build_id: Some("1234567812345678010203040506070803".to_owned()),
        }
    }

    /// The whole of what a report is, in both directions — through the reader files go through.
    #[test]
    fn a_report_round_trips() {
        let encoded = serde_json::to_vec(&sample()).expect("a report serialises");

        assert_eq!(
            CrashReport::from_json(&encoded).expect("and comes back"),
            sample()
        );
    }

    /// An absent thread, location, build id and frame offset, and an empty list of names, are
    /// absent on the wire rather than null, which is this crate's rule: a fact nobody established is
    /// missing, not empty. `frames` is the exception, always present, because an empty backtrace
    /// is itself the answer.
    #[test]
    fn what_std_did_not_report_is_absent_rather_than_null() {
        let bare = CrashReport {
            thread: None,
            location: None,
            frames: vec![CrashFrame { offset: None }],
            build_id: None,
            ..sample()
        };

        let encoded = serde_json::to_string(&bare).expect("a report serialises");

        assert!(!encoded.contains("thread"), "{encoded}");
        assert!(!encoded.contains("location"), "{encoded}");
        assert!(!encoded.contains("build_id"), "{encoded}");
        assert!(!encoded.contains("symbols"), "{encoded}");
        assert!(encoded.contains("\"frames\":[{}]"), "{encoded}");
    }

    /// A report written before T91a reads back with its format and its names intact.
    #[test]
    fn a_format_1_report_keeps_its_format_and_its_names() {
        let written = serde_json::json!({
            "format": 1,
            "recorded_at": 1_757_000_000_000_i64,
            "daemon": { "version": "0.0.13", "protocol": 1 },
            "os": "linux",
            "arch": "x86_64",
            "thread": "tokio-runtime-worker",
            "frames": ["mixengine_daemon::services::start", "core::ops::function::FnOnce::call_once"]
        });

        let read = CrashReport::from_json(&serde_json::to_vec(&written).expect("serialises"))
            .expect("a format-1 report is still read");

        assert_eq!(read.format, 1);
        assert!(read.frames.is_empty());
        assert_eq!(
            read.symbols,
            [
                "mixengine_daemon::services::start",
                "core::ops::function::FnOnce::call_once"
            ]
        );
        assert_eq!(read.build_id, None);
    }

    /// A report from a newer build is refused by number, not read as far as its shape happens to
    /// match.
    #[test]
    fn a_newer_format_is_refused() {
        let mut value = serde_json::to_value(sample()).expect("a report serialises");
        value["format"] = serde_json::json!(CRASH_FORMAT + 1);

        let refused = CrashReport::from_json(&serde_json::to_vec(&value).expect("serialises"))
            .expect_err("a format this build does not know");

        assert!(refused.to_string().contains("newer"), "{refused}");
    }

    /// A report edited by hand into a shape this build does not know is refused, rather than read
    /// with the unknown half silently dropped — [`DiagnosticsBundle`](crate::DiagnosticsBundle)'s
    /// rule, for the same reason.
    #[test]
    fn an_unknown_field_is_refused_rather_than_ignored() {
        let mut value = serde_json::to_value(sample()).expect("a report serialises");
        value["message"] = serde_json::json!("this is not a field a report has");

        assert!(CrashReport::from_json(&serde_json::to_vec(&value).expect("serialises")).is_err());
    }
}
