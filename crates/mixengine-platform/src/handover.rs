//! Becoming another program, and the record a resolver hands a trampoline.
//!
//! **Its own feature, because `mixengine-trampoline` takes this and nothing else.** A trampoline is
//! copied into `<root>/bin` once per command name on Windows, so every byte it links is paid for
//! dozens of times, and `process` would bring tokio with it. See
//! `docs/specs/2026-09-25-t185-a-bin-that-weighs-almost-nothing-design.md`.
//!
//! # The record
//!
//! What the resolver would have handed to [`hand_over`], written to its stdout instead: the
//! program, the arguments that go before the user's, and the environment to set. A sequence of
//! fields, each a tag byte, a native-endian `usize` length, and the value as this OS spells an
//! `OsStr` — UTF-16 code units on Windows, bytes on Unix — so nothing that is not Unicode is lost.
//! Resolver and trampoline ship in one release for one machine, so the two ends always agree on
//! endianness and width.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::sys::handover as sys;
use crate::{Error, Result};

/// Set by `mixengine-trampoline` on the resolver it runs, to the command name it was copied as.
/// Its presence is what tells `mixengine-shim` to write a [`Handover`] rather than hand over.
pub const SHIM_AS_ENV: &str = "MIXENGINE_SHIM_AS";

/// The file in `<root>/bin` naming the resolver's absolute path, one line of UTF-8.
pub const RESOLVER_POINTER: &str = "mixengine-shim.path";

const PROGRAM: u8 = b'P';
const ARGUMENT: u8 = b'A';
const KEY: u8 = b'K';
const VALUE: u8 = b'V';

/// What a resolved command becomes: see the module documentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handover {
    /// The program to run.
    pub program: PathBuf,
    /// Arguments that go before the user's own — `composer.phar` for Composer, otherwise none.
    pub args: Vec<OsString>,
    /// Applied over the inherited environment, exactly as [`hand_over`] applies it.
    pub env: BTreeMap<String, OsString>,
}

impl Handover {
    /// The bytes the resolver writes to its stdout.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        field(&mut out, PROGRAM, self.program.as_os_str());

        for argument in &self.args {
            field(&mut out, ARGUMENT, argument);
        }

        for (key, value) in &self.env {
            field(&mut out, KEY, OsStr::new(key));
            field(&mut out, VALUE, value);
        }

        out
    }

    /// Read what [`encode`](Self::encode) wrote.
    ///
    /// # Errors
    ///
    /// [`Error::Os`] with `InvalidData` for anything that is not exactly one such record.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut rest = bytes;
        let mut program = None;
        let mut args = Vec::new();
        let mut env = BTreeMap::new();
        let mut key: Option<String> = None;

        while !rest.is_empty() {
            let (tag, value, after) = next(rest)?;
            rest = after;

            match (tag, key.take()) {
                (PROGRAM, None) if program.is_none() => program = Some(PathBuf::from(value)),
                (ARGUMENT, None) => args.push(value),
                (KEY, None) => {
                    let name = value
                        .into_string()
                        .map_err(|_| malformed("a variable name that is not Unicode"))?;
                    key = Some(name);
                }
                (VALUE, Some(name)) => {
                    env.insert(name, value);
                }
                _ => return Err(malformed("fields out of order")),
            }
        }

        if key.is_some() {
            return Err(malformed("a variable with no value"));
        }

        let program = program.ok_or_else(|| malformed("no program"))?;

        Ok(Self { program, args, env })
    }
}

fn field(out: &mut Vec<u8>, tag: u8, value: &OsStr) {
    let bytes = sys::os_bytes(value);
    out.push(tag);
    out.extend_from_slice(&bytes.len().to_ne_bytes());
    out.extend_from_slice(&bytes);
}

fn next(bytes: &[u8]) -> Result<(u8, OsString, &[u8])> {
    let (&tag, rest) = bytes
        .split_first()
        .ok_or_else(|| malformed("an empty field"))?;
    let (length, rest) = rest
        .split_at_checked(size_of::<usize>())
        .ok_or_else(|| malformed("a truncated length"))?;
    let length = usize::from_ne_bytes(
        length
            .try_into()
            .map_err(|_| malformed("a truncated length"))?,
    );
    let (value, rest) = rest
        .split_at_checked(length)
        .ok_or_else(|| malformed("a truncated value"))?;

    Ok((tag, sys::os_string(value)?, rest))
}

/// Shared with `sys::handover`, whose decoding can fail on Windows.
pub(crate) fn malformed(what: &str) -> Error {
    Error::Os {
        action: "read what the resolver handed over",
        source: io::Error::new(io::ErrorKind::InvalidData, what.to_owned()),
    }
}

/// Become `program`: run it in this process's place and answer with the status it ended on.
///
/// **What a shim is made of.** A process the user invoked as `php` has worked out which PHP this
/// directory means and now has to get out of the way of it — with the same arguments, the same
/// standard streams, the same terminal and, at the end, the same exit code. `env` is applied *over*
/// this process's own environment rather than replacing it, which is the opposite of
/// `process::spawn_supervised` and for the opposite reason: a service's environment is declared in full by
/// its spec, and a shim is standing in the middle of somebody's shell session, where everything they
/// exported has to arrive intact.
///
/// # Unix: there is nothing to describe
///
/// `execve`. The process image is replaced, so the pid, the streams, the terminal, the process group
/// and every signal disposition are the ones the user's shell set up — Ctrl-C reaches the program
/// because the program *is* this process. **It returns only on failure**, which is why the `i32` in
/// the signature is Windows's answer and not a value any Unix caller will see.
///
/// # Windows: a child, and two things arranged around it
///
/// There is no `exec`, so the program is a child of a process that then does nothing but wait.
///
/// - **A Job Object with `KILL_ON_JOB_CLOSE`**, so a shim that is killed does not leave the program
///   it fronted running: `taskkill` on a `php -S` would otherwise take the shim and leave the
///   server holding the port, with nothing on the machine still naming it.
/// - **Ctrl-C and Ctrl-Break are swallowed by this process**, in `windows/handover.rs`.
///   A console event goes to *every* process attached to the console, so the child already has its
///   own copy; the default handling would end the shim first, close the job, and kill the child
///   before it could act on the interrupt it had just been sent. Closing the window, signing out and
///   shutting down are deliberately left alone — those are the cases where the child *should* go
///   down with this process.
///
/// **A child that exits before it can be put in the job is not a failure here**, which is where this
/// departs from `process::spawn_supervised`. Windows will not assign an ended process to a job and reports
/// that as `ERROR_ACCESS_DENIED`, indistinguishable from a real refusal — and for a shim the case is
/// not exotic but the common one, since `php -v` is over in a few milliseconds. Failing the run, or
/// killing the child, would turn the ordinary invocation into an error; so the assignment is
/// attempted, and the wait happens either way. What is lost when it does fail is the guarantee
/// above, for a program that has already finished.
///
/// # Errors
///
/// [`Error::Io`] naming the program when it cannot be started at all — which for a shim means an
/// install whose directory has been emptied — and [`Error::Os`] when Windows will not create the job
/// object, will not let this process handle its own console events, or cannot be waited on.
pub fn hand_over(
    program: &Path,
    args: &[OsString],
    env: &BTreeMap<String, OsString>,
) -> Result<i32> {
    let mut command = Command::new(program);

    // Everything else — the streams, the working directory, the rest of the environment — is
    // inherited by saying nothing about it, which is exactly what standing in for a program means.
    command.args(args).envs(env);

    sys::hand_over(command, program)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::*;

    fn record() -> Handover {
        Handover {
            program: PathBuf::from("runtimes/php/8.4.24/php.exe"),
            args: vec![OsString::from("composer.phar")],
            env: BTreeMap::from([
                (
                    "PATH".to_owned(),
                    OsString::from("runtimes/php/8.4.24;C:\\Windows"),
                ),
                (
                    "PHP_INI_SCAN_DIR".to_owned(),
                    OsString::from("etc/php/8.4/conf.d"),
                ),
            ]),
        }
    }

    #[test]
    fn a_record_comes_back_as_it_was_written() {
        let written = record();
        assert_eq!(
            Handover::decode(&written.encode()).expect("decodes"),
            written
        );
    }

    #[test]
    fn a_record_with_no_arguments_and_no_environment_comes_back() {
        let written = Handover {
            program: PathBuf::from("php"),
            args: Vec::new(),
            env: BTreeMap::new(),
        };
        assert_eq!(
            Handover::decode(&written.encode()).expect("decodes"),
            written
        );
    }

    /// A `PATH` is not promised to be Unicode on either system, and the record carries one.
    #[test]
    fn a_value_that_is_not_unicode_survives() {
        let mut written = record();
        written
            .env
            .insert("PATH".to_owned(), sys::not_unicode_for_tests());
        assert_eq!(
            Handover::decode(&written.encode()).expect("decodes"),
            written
        );
    }

    #[test]
    fn a_truncated_record_is_refused() {
        let bytes = record().encode();
        assert!(Handover::decode(&bytes[..bytes.len() - 1]).is_err());
    }

    #[test]
    fn a_record_with_no_program_is_refused() {
        assert!(Handover::decode(&[]).is_err());
    }
}
