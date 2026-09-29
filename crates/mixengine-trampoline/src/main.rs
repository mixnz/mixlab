//! `php.exe`, `node.exe`, `npm.exe` … in `<root>/bin` on Windows — roadmap task **T185**.
//!
//! A shim on Windows cannot `exec`: it stays as the parent of the program it starts, for as long
//! as that program runs, so the file it was started from stays open. Each name in `bin/` is
//! therefore a copy of its own, and this is what is in it — a few hundred KB rather than
//! `mixengine-shim`, which links the database and the runtime catalogue to resolve a version.
//!
//! ```text
//! 1  read <own directory>/mixengine-shim.path     where the resolver is
//! 2  run it with MIXENGINE_SHIM_AS=<own name>      it resolves and writes a record, then exits
//! 3  hand_over(record + own arguments)             Job Object child, its exit code is ours
//! ```
//!
//! The resolver speaks for itself on stderr when it refuses, and its exit code is passed through
//! untouched, so a person sees one sentence whichever of the two programs failed.
//!
//! See `docs/specs/2026-09-25-t185-a-bin-that-weighs-almost-nothing-design.md`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use mixengine_platform::handover::{self, Handover};

/// A shell's "command not found", as `mixengine-shim` uses it.
const NOT_RUNNABLE: i32 = 127;

fn main() {
    // `current_exe` and not `argv[0]`: on Windows `argv[0]` is what was typed, often a bare `php`,
    // and this file is always a copy, never a link, so its own path is its name.
    let own = std::env::current_exe().unwrap_or_default();
    let name = own
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();

    match run(&own, &name, arguments) {
        Ok(code) => std::process::exit(code),
        Err(said) => {
            eprintln!("{name}: {said}");
            std::process::exit(NOT_RUNNABLE);
        }
    }
}

fn run(own: &Path, name: &str, arguments: Vec<OsString>) -> Result<i32, String> {
    let bin = own
        .parent()
        .ok_or_else(|| "cannot tell which directory this program is in".to_owned())?;
    let resolver = resolver(bin)?;

    let ran = Command::new(&resolver)
        .env(handover::SHIM_AS_ENV, name)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| {
            format!(
                "cannot run {}: {error}; restarting MixEngine puts it back",
                resolver.display()
            )
        })?;

    // The resolver has already said why, on the stderr it shares with us.
    if !ran.status.success() {
        return Ok(ran.status.code().unwrap_or(NOT_RUNNABLE));
    }

    let record = Handover::decode(&ran.stdout).map_err(|error| {
        format!(
            "{} answered with something this cannot read: {}",
            resolver.display(),
            chain(&error)
        )
    })?;

    let mut args = record.args;
    args.extend(arguments);

    handover::hand_over(&record.program, &args, &record.env).map_err(|error| chain(&error))
}

/// Where the resolver is, as `shims::refresh` last wrote it.
fn resolver(bin: &Path) -> Result<PathBuf, String> {
    let pointer = bin.join(handover::RESOLVER_POINTER);

    let said = std::fs::read_to_string(&pointer).map_err(|error| {
        format!(
            "cannot read {}: {error}; restarting MixEngine writes it again",
            pointer.display()
        )
    })?;

    Ok(PathBuf::from(said.trim_end()))
}

/// An error and every cause under it, on one line.
fn chain(error: &dyn std::error::Error) -> String {
    let mut said = error.to_string();
    let mut cause = error.source();

    while let Some(inner) = cause {
        said.push_str(": ");
        said.push_str(&inner.to_string());
        cause = inner.source();
    }

    said
}
