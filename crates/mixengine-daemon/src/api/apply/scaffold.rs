//! Running a blueprint's own command — roadmap task **T78a**.
//!
//! **This is the first thing MixEngine runs that MixEngine did not write**, and every guard around
//! it is somewhere else: the consent that names the command is checked in
//! [`super`](crate::api::apply), the trust that decides how loudly it is asked for is a column
//! decided at import, and the shell that starts it is `mixengine-platform`'s. What is here is the
//! part that is only true while it runs — where the lines go, what a cancellation does, and what the
//! exit code becomes.
//!
//! Three things it deliberately does not do:
//!
//! - **It never elevates.** The command runs under the account this daemon runs as, and nothing it
//!   does reaches the elevation queue. T78's "one prompt, at the end" is about the hosts file and
//!   the trust store; a blueprint's command is not admitted to it.
//! - **It invents no environment.** The working directory is the project's and `PATH` gains the shim
//!   directory, which is how the blueprint's own `[runtimes]` reaches the command — the shims
//!   resolve a version from the project they are run in. Nothing else is added.
//! - **It has no timeout** (the T78a design, D10). Every number that could be chosen kills a
//!   legitimate `composer install` on a slow line, and a scaffold is by definition somebody else's
//!   program doing an unknown amount of work. The bound is the job: it is visible, its output is
//!   streaming, and `job.cancel` stops it — which kills the process *group*, so the tree a package
//!   manager forked goes with it.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use mixengine_core::Paths;
use mixengine_platform::process::Limits;
use mixengine_proto::{LogLine, LogPolicy, StepResult};
use mixengine_supervisor::logs::Capture;
use tokio::sync::broadcast;

use crate::jobs::JobHandle;
use crate::services::logs::ServiceLog;

/// How often the command is asked whether it has ended.
///
/// `services::runner`'s own liveness poll, and for its reason: there is nothing to wait *on* that a
/// cancellation could also interrupt, so the wait is a poll and the interval is short enough that a
/// finished command is not sat on.
const POLL: Duration = Duration::from_millis(50);

/// How many of the command's last lines are quoted when it fails.
///
/// A job's log is a ring and no file (D13), so what survives the terminal scrolling past is this.
const LAST_WORDS: usize = 3;

/// How long a command that has exited is given for its output to be read to end of file.
///
/// `services::runner`'s own number, for its own reason: a grandchild that left the group
/// can hold the write end open indefinitely, so this wait is bounded and losing it costs the last
/// few lines rather than the daemon.
const FLUSH: Duration = Duration::from_secs(2);

/// Where a running command's output goes.
///
/// A trait so the tests can watch the lines without a job registry behind them, and so this module
/// says out loud that it only ever *writes* — nothing here reads the log back.
pub(crate) trait Sink: Send + Sync {
    /// One line, as it arrived.
    fn line(&self, line: LogLine);

    /// Lines that were lost because this daemon fell behind the command's own output.
    ///
    /// **Said rather than swallowed**, which is
    /// [ADR 0009](../../../../../docs/decisions/0009-logs-travel-on-their-own-stream.md)'s own
    /// rule one subject along: a `npm install` can outrun the reader below, and a hole nobody
    /// mentions is worse than one that is named.
    fn missed(&self, lines: u64);
}

impl Sink for ServiceLog {
    fn line(&self, line: LogLine) {
        self.record(line);
    }

    fn missed(&self, lines: u64) {
        ServiceLog::missed(self, lines);
    }
}

/// A sink for a test that only wants the outcome.
#[cfg(test)]
pub(crate) struct Discarding;

#[cfg(test)]
impl Sink for Discarding {
    fn line(&self, _line: LogLine) {}

    fn missed(&self, _lines: u64) {}
}

/// What the command sees: the project's directory, and the shims in front of this daemon's `PATH`.
///
/// **The shim directory is the whole of how a blueprint's `[runtimes]` reaches the command.** A shim
/// reads the project it is run in and resolves the version pinned there, so `composer` finds the PHP
/// this apply just pinned without anything here computing a path to it. The rest of `PATH` is this
/// daemon's own, because a scaffold needs the `git`, `npm` and `composer` a person installed and
/// inventing that list is not something a daemon can do honestly.
///
/// **`std::env::join_paths` rather than a separator of our own**, which is what keeps this file free
/// of a `#[cfg(windows)]` the way `CLAUDE.md` asks of everything above
/// `mixengine-platform`: the standard library already knows what this system puts between two `PATH`
/// entries. A `PATH` this cannot be joined back into — an entry holding the separator itself — leaves
/// the command with the shims alone, which is the half that matters here.
pub(crate) fn environment(paths: &Paths) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();

    env.insert(
        "PATH".to_owned(),
        path(paths).to_string_lossy().into_owned(),
    );

    // **The shims in that directory have to know which home they belong to** — found by T27c's
    // first real `composer create-project`, on a daemon started with `--home`. A shim reads
    // `MIXENGINE_HOME` and otherwise falls back to the OS default, which is *another* home's
    // database whenever this daemon's is not the default one: every `php`, `npx` or `composer` a
    // scaffold ran there resolved against the wrong install. Set to this daemon's root, so the
    // shim and the daemon that put it on the PATH agree about what "here" is.
    env.insert(
        "MIXENGINE_HOME".to_owned(),
        paths.root().to_string_lossy().into_owned(),
    );

    // **There is no terminal on the other end of this** — roadmap task **T120a**. A program that
    // assumes one writes control sequences into a string a window renders as text, which is how
    // `create-next-app`'s refusal reached a person as `[31m"Next.js 1"[39m`.
    //
    // This bends the note above about inventing no environment, and the bend is deliberate rather
    // than overlooked: it changes nothing about what the command *does*, it answers a question
    // about where the output is going that the command would otherwise guess wrong. It is also
    // never the guarantee — [`without_escapes`] is, because `NO_COLOR` is a convention and not
    // every program reads it.
    env.insert("NO_COLOR".to_owned(), "1".to_owned());

    env
}

/// The same text with every ANSI escape sequence taken out — roadmap task **T120a**.
///
/// **Here rather than from a crate**, for one screenful of matching with no dependency and no
/// version to it: the shapes a build tool emits are CSI (`ESC [ … final`), OSC (`ESC ] …` ended by
/// BEL or by ST) and the two-character escapes. A line holding none of them comes back unchanged.
///
/// **Removed at capture rather than at display.** There are three renderers of this text — the
/// CLI, the desktop's dialog, and the job log T120 built — and a rule enforced in one place is a
/// rule, while a rule enforced in three is a schedule for the fourth to be written without it.
fn without_escapes(text: &str) -> String {
    let mut clean = String::with_capacity(text.len());
    let mut characters = text.chars();

    while let Some(character) = characters.next() {
        if character != '\u{1b}' {
            clean.push(character);
            continue;
        }

        match characters.next() {
            // CSI: parameters and intermediates, then one final byte in `@`..=`~`.
            Some('[') => {
                for inside in characters.by_ref() {
                    if matches!(inside, '\u{40}'..='\u{7e}') {
                        break;
                    }
                }
            }

            // OSC: a string ended by BEL, or by ST — which is itself an escape, so the character
            // after it belongs to the sequence too.
            Some(']') => {
                while let Some(inside) = characters.next() {
                    match inside {
                        '\u{7}' => break,
                        '\u{1b}' => {
                            characters.next();
                            break;
                        }
                        _ => {}
                    }
                }
            }

            // `ESC (B` and its relatives: one more character belongs to the sequence. A bare
            // `ESC M` would lose the character after it, which is the trade this shape makes and
            // costs nothing against the programs that actually reach here.
            Some(_) => {
                characters.next();
            }

            // A trailing escape with nothing after it. Dropped, which is the whole job.
            None => {}
        }
    }

    clean
}

/// One captured line, with nothing in it a terminal would obey.
fn scrubbed(mut line: LogLine) -> LogLine {
    if line.text.contains('\u{1b}') {
        line.text = without_escapes(&line.text);
    }

    line
}

/// The `PATH` a scaffold command runs with: `<home>/bin` first, then this daemon's own.
///
/// One function for both the plan and the shell — roadmap task **T78b**, its design's D3: the plan
/// judges a command's program against exactly the string the command would be started with, and a
/// program installed after this daemon started is invisible to both until it restarts. What
/// [`environment`] says about the shims and about `join_paths` is about this half of it.
pub(crate) fn path(paths: &Paths) -> std::ffi::OsString {
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let ahead = std::iter::once(paths.bin().to_path_buf());

    std::env::join_paths(ahead.chain(std::env::split_paths(&inherited)))
        .unwrap_or_else(|_| paths.bin().as_os_str().to_owned())
}

/// Run one command in `root`, with its output going to `sink` as it arrives.
///
/// **A non-zero exit is a [`StepResult::Failed`] and not an error** (D7 and D8): the apply did what
/// it was asked, the project it made works, and the command's exit code is the command's own news.
/// Returning an error here would spend T78's rollback ledger on a project that is fine.
///
/// A cancellation stops the group and answers [`StepResult::NotRun`], because a cancellation is not
/// a request to undo anything — running the apply again offers the command afresh.
pub(crate) async fn run_command(
    command: &str,
    root: &Path,
    env: &BTreeMap<String, String>,
    sink: &dyn Sink,
    cancelled: Option<&JobHandle>,
) -> StepResult {
    let mut child = match mixengine_platform::process::spawn_shell_supervised(
        command,
        root,
        env,
        &Limits::default(),
    ) {
        Ok(child) => child,

        // The shell itself would not start, which is a machine that cannot run any scaffold rather
        // than a command that failed. Still this step's outcome and not the apply's: the project is
        // made either way.
        //
        // **Flattened, because the platform's sentence is only `cannot start <shell>`** and the
        // OS's reason is its `#[source]`: a working directory that is not there reads, without it,
        // as a broken `cmd.exe`.
        Err(error) => {
            return StepResult::Failed {
                why: format!(
                    "it could not be started: {}",
                    mixengine_proto::flatten(&error)
                ),
            };
        }
    };

    // Before anything waits: a pipe nobody drains stops the command writing to it, which looks
    // exactly like a command that has hung. `services::runner`'s obligation, discharged here for
    // the one child this module starts.
    let mut capture = Capture::start(&mut child, "a blueprint's command", policy(), None);
    let (already_said, mut lines) = capture.read();

    for line in already_said {
        sink.line(scrubbed(line));
    }

    let mut last = Vec::new();

    loop {
        drain(&mut lines, sink, &mut last);

        if cancelled.is_some_and(JobHandle::is_cancelled) {
            // The group, not the pid: a package manager's children are what would otherwise be left
            // running with nothing owning them.
            let _ = child.stop();

            return StepResult::NotRun {
                why: format!("`{command}` was cancelled; running the apply again offers it again"),
            };
        }

        match child.exited() {
            Ok(Some(exit)) => {
                // **The reader threads are waited for before the last drain, not after it.** A
                // process exiting is not its output having been read: the pipes are drained on
                // threads of their own, and `drain` is a `try_recv` loop over what they have
                // published *so far*. A command that printed one line and exited at once — which is
                // every scaffold's last line, and `echo` in the test below — loses it on a machine
                // where those threads have not been scheduled yet. Found by this task's CI run,
                // where a loaded Linux runner lost the line and Windows and macOS did not.
                //
                // `Runner::kill`'s shape, and its budget: off the runtime because `finish` blocks,
                // and bounded because end of file is the last process holding the write end
                // exiting, which a grandchild that left the group can delay for ever. A wait that
                // runs out costs the last few lines and nothing else.
                // Held to the end of this arm rather than assigned back: what it is for is keeping
                // the sending half alive across the drain below.
                let _drained = match tokio::task::spawn_blocking(move || {
                    let flushed = capture.finish(FLUSH);
                    (capture, flushed)
                })
                .await
                {
                    Ok((capture, true)) => capture,
                    Ok((capture, false)) => {
                        tracing::warn!(
                            command,
                            "the last lines of this command were not read before it ended"
                        );

                        capture
                    }
                    Err(error) => {
                        tracing::error!(%error, "the task draining this command did not finish");

                        Capture::detached()
                    }
                };

                // Whatever was still in the pipes when it ended.
                drain(&mut lines, sink, &mut last);

                return match exit.is_success() {
                    true => StepResult::Done,
                    false => StepResult::Failed {
                        why: failure_in(command, exit.code(), &last, root),
                    },
                };
            }

            Ok(None) => tokio::time::sleep(POLL).await,

            Err(error) => {
                return StepResult::Failed {
                    why: format!("this daemon lost track of `{command}`: {error}"),
                };
            }
        }
    }
}

/// How many of a command's lines the daemon keeps for a reader that connects late.
///
/// Small on purpose: what a client following the job reads is streaming past it, and what is kept
/// is only the backlog somebody arriving mid-command is handed.
pub(crate) const RING_LINES: usize = 200;

/// The ring the capture keeps while the command runs.
fn policy() -> LogPolicy {
    LogPolicy {
        ring_lines: u16::try_from(RING_LINES).unwrap_or(u16::MAX),
        ..LogPolicy::default()
    }
}

/// Move whatever the capture holds into the sink, and say so when some of it was lost.
///
/// **A command can outrun this.** The capture's broadcast holds a bounded backlog, and a
/// `npm install` printing faster than one poll drains costs the reader lines — which is reported as
/// a gap rather than passed over, so a log with a hole in it says where the hole is.
fn drain(lines: &mut broadcast::Receiver<LogLine>, sink: &dyn Sink, last: &mut Vec<String>) {
    loop {
        match lines.try_recv() {
            Ok(line) => {
                let line = scrubbed(line);

                keep_last(last, &line);
                sink.line(line);
            }

            Err(broadcast::error::TryRecvError::Lagged(missed)) => sink.missed(missed),

            // Nothing more for now, or the command's readers have ended. Either way this pass is
            // over; the caller decides whether there will be another.
            Err(_) => return,
        }
    }
}

/// Keep the last few lines, for the sentence a failure has to write.
fn keep_last(last: &mut Vec<String>, line: &LogLine) {
    if last.len() == LAST_WORDS {
        last.remove(0);
    }

    last.push(line.text.clone());
}

/// What a failed command's outcome says, for a command that ran in `root`.
///
/// The exit code, and the last of what it printed — because a job's log is memory only, and by the
/// time somebody reads the outcome the lines may be gone.
///
/// **And the folder, when npm would not have taken its name** — roadmap task **T120c**, its
/// design's D8. The plan blocks this before anything is installed, but only for a blueprint whose
/// author declared `needs_npm_safe_dir`, and nobody's imported blueprint ever will. So the same
/// sentence is written here as well, arriving after the download instead of before it, which is
/// worse than the block and better than npm's own words.
///
/// **Only for a command of the npm family**, which is the one guess this makes and the reason it is
/// kept narrow: `composer create-project` takes its package name from its argument, so an npm rule
/// added to *its* failure would be a false sentence sending somebody to rename a folder for
/// nothing — the comprehension failure T120c is about, pointed back at us. A command the guess does
/// not recognise simply gets no extra sentence.
///
/// Both rules are asked of [`mixengine_core::blueprints`] rather than restated: a second copy of
/// the charset, or of the program list, is two rules that drift.
fn failure_in(command: &str, code: Option<i32>, last: &[String], root: &Path) -> String {
    let ended = match code {
        Some(code) => format!("`{command}` exited with {code}"),
        None => format!("`{command}` was ended by a signal"),
    };

    let said = match last.iter().find(|line| !line.trim().is_empty()) {
        Some(_) => format!(
            "{ended}; its last words:
{}",
            last.join(
                "
"
            )
        ),
        None => ended,
    };

    if !mixengine_core::blueprints::program::is_an_npm_command(command) {
        return said;
    }

    match mixengine_core::blueprints::plan::not_an_npm_name(root) {
        Some(reason) => format!(
            "{said}

{reason}"
        ),
        None => said,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A command that writes a file, on this system. A blueprint's command is its author's, and a
    /// test's is the test's.
    fn writing_a_file() -> &'static str {
        if cfg!(windows) {
            "echo hello> made.txt"
        } else {
            "printf hello > made.txt"
        }
    }

    /// It runs in the project's directory, which is the whole of where a scaffold belongs.
    #[tokio::test]
    async fn a_command_runs_in_the_project_directory() {
        let root = tempfile::tempdir().expect("a directory");

        let result = run_command(
            writing_a_file(),
            root.path(),
            &BTreeMap::new(),
            &Discarding,
            None,
        )
        .await;

        assert_eq!(result, StepResult::Done, "{result:?}");
        assert!(root.path().join("made.txt").is_file());
    }

    /// **A command that could not be started says why.** The platform error's own sentence is only
    /// `cannot start <shell>`, and the OS's reason is its `#[source]` — so a Laravel apply into a
    /// directory somebody had since deleted reported a broken `cmd.exe` and not the missing folder.
    #[tokio::test]
    async fn a_command_that_cannot_start_says_what_the_os_said() {
        let home = tempfile::tempdir().expect("a directory");
        let gone = home.path().join("deleted since");

        let result = run_command("exit 0", &gone, &BTreeMap::new(), &Discarding, None).await;

        let StepResult::Failed { why } = result else {
            panic!("a failed step, not {result:?}");
        };

        assert!(why.contains("os error"), "{why}");
    }

    /// **A command that fails is a failed step, not a failed job** — roadmap task **T78a**, its
    /// design's D7. The exit code is in the outcome, so somebody reading it afterwards has the one
    /// fact the log may no longer hold.
    #[tokio::test]
    async fn a_command_that_exits_non_zero_is_a_failed_step() {
        let root = tempfile::tempdir().expect("a directory");

        let result = run_command("exit 3", root.path(), &BTreeMap::new(), &Discarding, None).await;

        let StepResult::Failed { why } = result else {
            panic!("a failed step, not {result:?}");
        };

        assert!(why.contains('3'), "{why}");
    }

    /// The output reaches the sink while the command is running, which is what a client following
    /// the job's log is reading.
    #[tokio::test]
    async fn what_the_command_printed_reaches_the_log() {
        use std::sync::Mutex;

        struct Collecting(Mutex<Vec<String>>);

        impl Sink for Collecting {
            fn line(&self, line: LogLine) {
                self.0.lock().expect("the lock").push(line.text);
            }

            fn missed(&self, lines: u64) {
                self.0
                    .lock()
                    .expect("the lock")
                    .push(format!("<{lines} lines lost>"));
            }
        }

        let root = tempfile::tempdir().expect("a directory");
        let collected = Collecting(Mutex::new(Vec::new()));

        let result = run_command(
            "echo scaffolding",
            root.path(),
            &BTreeMap::new(),
            &collected,
            None,
        )
        .await;

        assert_eq!(result, StepResult::Done);
        assert!(
            collected
                .0
                .lock()
                .expect("the lock")
                .iter()
                .any(|line| line.contains("scaffolding")),
            "{:?}",
            collected.0.lock().expect("the lock")
        );
    }

    /// The shims go in front of what this daemon inherited, because that is how a blueprint's
    /// `[runtimes]` reaches a command that only ever types `php`.
    #[test]
    fn the_shim_directory_leads_the_path() {
        let home = tempfile::tempdir().expect("a directory");
        let paths = Paths::new(home.path().to_path_buf(), &Default::default());

        let env = environment(&paths);
        let path = env.get("PATH").expect("a PATH");

        assert!(
            path.starts_with(&paths.bin().display().to_string()),
            "{path}"
        );
    }

    /// **And the shims are told which home they belong to** — roadmap task **T27c**, found on the
    /// way: a daemon started with `--home` put its `bin/` on the PATH and the shim in it looked up
    /// the OS default home's database instead.
    #[test]
    fn the_command_is_told_which_home_the_shims_belong_to() {
        let home = tempfile::tempdir().expect("a directory");
        let paths = Paths::new(home.path().to_path_buf(), &Default::default());

        let env = environment(&paths);

        assert_eq!(
            env.get("MIXENGINE_HOME").map(String::as_str),
            Some(paths.root().to_string_lossy().as_ref())
        );
    }

    /// **The command is told there is no terminal** — roadmap task **T120a**.
    ///
    /// A complement to the scrub below and never a substitute: `NO_COLOR` is a convention, honoured
    /// by `create-next-app` and not by everything. Measured on 2026-09-13 against
    /// `create-next-app@latest`: `NO_COLOR=1` silences it and `FORCE_COLOR=0` does not, which is
    /// why the obvious one is not the one set here.
    #[test]
    fn a_command_is_told_that_nothing_is_a_terminal() {
        let home = tempfile::tempdir().expect("a directory");
        let paths = Paths::new(home.path().to_path_buf(), &Default::default());

        assert_eq!(
            environment(&paths).get("NO_COLOR").map(String::as_str),
            Some("1")
        );
    }

    /// **What a command printed is data, not control** — roadmap task **T120a**.
    ///
    /// `create-next-app` colours a pipe, so its refusal reached a person as
    /// `[31m"Next.js 1"[39m` — and a terminal reading the same string would have obeyed it
    /// instead of showing it.
    #[test]
    fn an_escape_sequence_never_survives_capture() {
        let coloured = "Could not create a project called \u{1b}[31m\"Next.js 1\"\u{1b}[39m";

        let scrubbed = without_escapes(coloured);

        assert_eq!(
            scrubbed, "Could not create a project called \"Next.js 1\"",
            "{scrubbed:?}"
        );
    }

    /// The other shapes: the bold/reset pair npm writes around a bullet, an OSC sequence — which is
    /// the one that does not end in a letter — and a two-character escape.
    #[test]
    fn the_other_shapes_of_escape_go_too() {
        for noisy in [
            "\u{1b}[1m*\u{1b}[22m name can only contain URL-friendly characters",
            "\u{1b}]0;a title\u{7}still here",
            "\u{1b}]0;a title\u{1b}\\still here",
            "\u{1b}(Bplain",
        ] {
            let scrubbed = without_escapes(noisy);

            assert!(!scrubbed.contains('\u{1b}'), "{noisy:?} -> {scrubbed:?}");
            assert!(
                scrubbed.contains("name")
                    || scrubbed.contains("still here")
                    || scrubbed.contains("plain"),
                "{noisy:?} -> {scrubbed:?}"
            );
        }
    }

    /// A line with nothing to take out is the same line.
    #[test]
    fn a_plain_line_is_unchanged() {
        assert_eq!(
            without_escapes("npm warn deprecated"),
            "npm warn deprecated"
        );
    }

    /// **Multi-line output stays multi-line** — roadmap task **T120a**. Joining with `" / "` turned
    /// a three-line explanation into one line whose punctuation read like paths.
    #[test]
    fn a_failures_last_words_keep_their_lines() {
        let last = vec![
            "Could not create a project called \"Next.js 1\":".to_owned(),
            "  * name can only contain URL-friendly characters".to_owned(),
        ];

        let said = failure_in(
            "npx create-next-app .",
            Some(1),
            &last,
            Path::new("/work/next-js-1"),
        );

        assert!(said.contains("exited with 1"), "{said}");
        assert_eq!(said.lines().count(), 3, "{said}");
    }

    /// **The blueprint that never declared it** — roadmap task **T120c**, its design's D8. A
    /// plan-time block only reaches a blueprint whose author asked for one; an imported one never
    /// will, so a failure in a folder npm would refuse carries the same explanation — late, which
    /// is worse than early and better than nothing.
    #[test]
    fn a_failure_in_a_folder_npm_would_refuse_says_so() {
        let last = vec!["Could not create a project called \"Next.js 1\"".to_owned()];

        let said = failure_in(
            "npx create-next-app .",
            Some(1),
            &last,
            Path::new("/work/Next.js 1"),
        );

        assert!(said.contains("Next.js 1"), "{said}");
        assert!(said.contains("next-js-1"), "{said}");
    }

    /// **And npm's rule is only spoken about npm's commands.** `composer create-project` takes its
    /// package name from its argument, so telling somebody whose `composer` failed over a network
    /// that their folder is the problem is a false sentence sending them to rename a folder for
    /// nothing — the same comprehension failure this whole task exists to answer, pointed the other
    /// way. D8's guess is narrow because a miss costs a hint and a wrong guess costs a wasted
    /// afternoon.
    #[test]
    fn a_composer_failure_is_told_nothing_about_npms_rule() {
        let last = vec!["Could not resolve dependencies".to_owned()];

        let said = failure_in(
            "composer create-project laravel/laravel . --no-interaction",
            Some(1),
            &last,
            Path::new("/work/My Blog"),
        );

        assert!(!said.contains("npm"), "{said}");
        assert!(!said.contains("rename"), "{said}");
    }

    /// And a folder npm is happy with gets no lecture — including the underscored one, which is the
    /// row that decided the rule.
    #[test]
    fn a_failure_in_a_folder_npm_accepts_is_left_alone() {
        let last = vec!["some other problem".to_owned()];

        let said = failure_in(
            "npx create-next-app .",
            Some(1),
            &last,
            Path::new("/work/next_js_1"),
        );

        assert!(!said.contains("rename"), "{said}");
    }
}
