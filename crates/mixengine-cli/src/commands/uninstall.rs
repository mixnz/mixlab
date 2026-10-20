//! `mix uninstall`, and waiting for what it removes to be gone.

use std::io::Write as _;
use std::process::ExitCode;

use mixengine_platform::ipc::Endpoint;
use mixengine_proto::{
    DaemonStatus, ElevationStatus, Error, ErrorCode, JobOutcome, JobSummary, Removal, ResidueId,
    UninstallQuery, UninstallReport, rpc,
};

use crate::autostart::Autostart;
use crate::by_hand::{self, Route, Terminal};
use crate::client::{Client, ask, encode};
use crate::confirm::unanswered;
use crate::jobs::follow;
use crate::output::{emit, rendered};
use crate::{confirm, render};

/// `mix uninstall` — roadmap task **T87**.
///
/// **The plan is asked for first and always**, even on the way to the real thing: what a person is
/// about to allow is what they are shown, which is T64's rule applied to the one command that cannot
/// be undone. `--dry-run` is that same call and then nothing else.
///
/// **Exit `3` is "something is in the way"** (the T182 design, D4): a process running from a
/// directory that would be removed. Distinct from `1`, so the Windows uninstaller can tell *close
/// this and try again* from *this failed*.
pub(crate) async fn uninstall(
    endpoint: &Endpoint,
    autostart: Option<&Autostart>,
    json: bool,
    wanted: UninstallAsk,
) -> Result<ExitCode, Error> {
    let UninstallAsk {
        dry_run,
        keep_home,
        keep_relocated,
        package,
        relocated,
        blocked,
        yes,
        no_wait,
    } = wanted;

    let mut client = Client::connect(endpoint, autostart).await?;

    // T182b, D8: which process this is, by pid *and* the moment it began, so the wait at the end is
    // for this daemon and not for whatever the OS later hands the pid to. A daemon that will not say
    // is waited for the old way, by its endpoint going quiet.
    let daemon = ask::<DaemonStatus>(&mut client, rpc::method::DAEMON_STATUS, None)
        .await
        .ok()
        .and_then(|status| {
            mixengine_platform::process::started_at(status.pid)
                .ok()
                .flatten()
                .map(|began| (status.pid, began))
        });

    let query = UninstallQuery {
        keep_home,
        keep_relocated,
        // A plan raises nothing whatever this says; sent as it will be sent to the act, so the two
        // calls are visibly one question asked twice.
        grant: false,
        // T182e: the listing the uninstaller reads while its banner is up names folders and
        // nothing else, so it does not pay for reading the handle table.
        skip_holders: relocated,
        package,
        by_hand: false,
    };

    let planned: UninstallReport = ask(
        &mut client,
        rpc::method::DAEMON_UNINSTALL_PLAN,
        encode(&query),
    )
    .await?;

    // T182, D9: the listing the Windows uninstaller reads — the directories `[paths]` moved out,
    // and not the tombstones beside them, which are garbage and not a choice.
    if relocated {
        for item in &planned.items {
            if item.id == ResidueId::RelocatedDirectory
                && !item.location.contains(mixengine_platform::tombstone::MARK)
            {
                emit(&format!("{}\n", item.location))?;
            }
        }

        return Ok(ExitCode::SUCCESS);
    }

    // T182e, D6: the listing the uninstaller's "close these first" page reads — one line per program
    // in the way, and nothing else. A question and not a refusal, so it succeeds either way. ASCII
    // only: the uninstaller reads it through `nsExec`, which spells a dash as three other characters.
    if blocked {
        for item in &planned.items {
            if matches!(item.outcome, Removal::Blocked { .. }) {
                emit(&format!("{}: {}\n", item.what, item.location))?;
            }
        }

        return Ok(ExitCode::SUCCESS);
    }

    // **One document per run under `--json`, and the plan is not it.** The plan is printed so that a
    // person can read what they are about to allow; a caller reading JSON is not being asked
    // anything, and two objects on one stdout is not JSON at all. Under `--dry-run` the plan *is*
    // the answer, so there it is printed either way.
    if dry_run || !json {
        emit(&rendered(json, &planned, || {
            render::uninstall::uninstall_report(&planned)
        }))?;
    }

    if planned.blocked() {
        return Ok(ExitCode::from(BLOCKED));
    }

    if dry_run {
        return Ok(ExitCode::SUCCESS);
    }

    // ADR 0065: how the one prompt will be raised is settled before the question. A run that cannot
    // raise one is not refused — most of an uninstall needs no administrator — but it is said
    // here, while nothing has changed.
    let route = privileged_route(&mut client, json).await?;

    if !yes && !agreed_to_uninstall(&planned, keep_home, keep_relocated, json)? {
        // Saying no is an answer and not a failure — `mix elevation grant`'s rule. Nothing was
        // removed, so the same command works when the person is ready.
        return Ok(ExitCode::SUCCESS);
    }

    // Measured now, from the plan, while nothing is being removed: what `left_behind` says while it
    // waits is how much there is to remove (T182b).
    let removing = bytes_going(&planned);

    let started: JobSummary = ask(
        &mut client,
        rpc::method::DAEMON_UNINSTALL,
        encode(&UninstallQuery {
            // The plan above *is* the batch this allows, and it has just been shown or answered for
            // in advance — which is T64's rule met, so the prompt is raised inside the one job the
            // caller is already following.
            grant: true,
            // And the act always looks: something stuck that appeared since the plan refuses here.
            skip_holders: false,
            by_hand: route.by_hand(),
            ..query
        }),
    )
    .await?;

    if no_wait {
        emit(&rendered(json, &started, || {
            render::job::job_status(&started)
        }))?;
        return Ok(ExitCode::SUCCESS);
    }

    let ended = match route {
        Route::ByHand => by_hand::run_waiting(&mut client, started.id).await?,
        Route::Dialog => None,
    };
    let finished = match ended {
        Some(finished) => finished,
        None => follow(&mut client, started, json).await?,
    };

    let Some(JobOutcome::Succeeded { result }) = finished.outcome.clone() else {
        emit(&rendered(json, &finished, || {
            render::job::job_status(&finished)
        }))?;
        return Ok(ExitCode::FAILURE);
    };

    let report: UninstallReport = serde_json::from_value(result).map_err(|error| {
        Error::new(
            ErrorCode::Internal,
            format!(
                "mix {} cannot read the uninstall report: {error}",
                env!("CARGO_PKG_VERSION")
            ),
        )
    })?;

    emit(&rendered(json, &report, || {
        render::uninstall::uninstall_report(&report)
    }))?;

    let still_there = left_behind(&report, endpoint, daemon, removing).await;

    // **And the process itself** (the T182b design, D8). The endpoint goes quiet before the daemon
    // has finished removing its home and exited, and until it has, its image is still mapped: the
    // Windows uninstaller deleting `mixengined.exe` straight after this returned is what found that.
    let lingering = finished_uninstall(&report)
        && daemon.is_some_and(|(pid, began)| !process_has_ended(pid, began));

    // **Why, before what.** A daemon that could not remove its home says so in a note outside it
    // (T182b), and that sentence is the one that tells a person what to close.
    if let Some((pid, _)) = daemon.filter(|_| !lingering) {
        print_the_daemons_note(pid);
    }

    for path in &still_there {
        report_left(path);
    }

    // T182a: the program's own files, read back off the disk once the daemon has gone, like the
    // home. `mix` itself is one of them and goes on running from memory, as the window does.
    for path in package_left(&report) {
        report_left(&path);
    }

    if let Some((pid, _)) = daemon.filter(|_| lingering) {
        report_left(&format!("the daemon (pid {pid}) is still running"));
    }

    Ok(
        match report.left_behind() || !still_there.is_empty() || lingering {
            true => ExitCode::FAILURE,
            false => ExitCode::SUCCESS,
        },
    )
}

/// Which way the uninstall's one prompt is raised — ADR 0065.
///
/// Where neither a dialog nor this terminal can ask, or the helper is not one a terminal may run,
/// the reason is printed and the dialog is asked for anyway: the daemon then leaves what needs an
/// administrator behind and says so, which is what an uninstall did before.
async fn privileged_route(client: &mut Client, json: bool) -> Result<Route, Error> {
    let elevation: ElevationStatus = ask(client, rpc::method::ELEVATION_STATUS, None).await?;

    let decided =
        by_hand::route(&elevation, Terminal::here(), json, "mix uninstall").and_then(|route| {
            match route {
                Route::ByHand => by_hand::trusted_here(elevation.helper.as_deref()).map(|()| route),
                Route::Dialog => Ok(route),
            }
        });

    Ok(decided.unwrap_or_else(|error| {
        let _ = writeln!(
            std::io::stderr(),
            "{error}\nwhat needs an administrator's permission will be left behind"
        );
        Route::Dialog
    }))
}

/// The `.pkg`'s paths still on disk after a run that says it removed the package — T182a.
fn package_left(report: &UninstallReport) -> Vec<String> {
    let removed = report.items.iter().any(|item| {
        item.id == ResidueId::Package && matches!(item.outcome, Removal::Removed { .. })
    });
    if !removed {
        return Vec::new();
    }
    mixengine_platform::install::package_paths()
        .map(|paths| {
            paths
                .binaries
                .into_iter()
                .chain([paths.bundle])
                .filter(|path| path.symlink_metadata().is_ok())
                .map(|path| path.display().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Did the daemon say this uninstall finished, which is what ends it (T182b, D1)?
fn finished_uninstall(report: &UninstallReport) -> bool {
    !report.items.iter().any(|item| {
        matches!(
            item.outcome,
            Removal::Failed { .. } | Removal::Enqueued { .. }
        )
    })
}

/// Wait up to [`PROCESS_GONE`] for the process that began at `began` to have ended, and say whether
/// it did.
///
/// **By pid and start time**, so a pid the OS reused for something else in the meantime reads as
/// ended rather than as this daemon still running.
fn process_has_ended(pid: u32, began: mixengine_platform::process::StartTime) -> bool {
    let deadline = std::time::Instant::now() + PROCESS_GONE;

    loop {
        if has_ended(pid, began) {
            return true;
        }

        if std::time::Instant::now() >= deadline {
            return false;
        }

        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// Has the process that began at `began` ended? Asked once.
fn has_ended(pid: u32, began: mixengine_platform::process::StartTime) -> bool {
    !mixengine_platform::process::started_at(pid)
        .ok()
        .flatten()
        .is_some_and(|now| now == began)
}

/// How many bytes the directories this plan removes hold — the home, the relocated directories and
/// the window's folders that are `Planned` (T182b).
///
/// **Without following a link**, since a removal does not either, and with every unreadable entry
/// counted as nothing: this is a figure for a person, and a wrong one is only a wrong figure.
fn bytes_going(plan: &UninstallReport) -> u64 {
    plan.items
        .iter()
        .filter(|item| matches!(item.outcome, Removal::Planned { .. }))
        .filter(|item| {
            matches!(
                item.id,
                ResidueId::Home
                    | ResidueId::RelocatedDirectory
                    | ResidueId::WindowData
                    | ResidueId::WindowCache
            )
        })
        .map(|item| bytes_under(std::path::Path::new(&item.location)))
        .sum()
}

/// The size of everything under `path`, or of `path` itself when it is a file.
fn bytes_under(path: &std::path::Path) -> u64 {
    let mut total = 0;
    let mut waiting = vec![path.to_path_buf()];

    while let Some(current) = waiting.pop() {
        let Ok(metadata) = std::fs::symlink_metadata(&current) else {
            continue;
        };

        if metadata.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&current) {
                waiting.extend(entries.filter_map(Result::ok).map(|entry| entry.path()));
            }
        } else {
            total += metadata.len();
        }
    }

    total
}

/// Print, and then remove, what the daemon `pid` said about a removal it could not finish.
fn print_the_daemons_note(pid: u32) {
    let note = mixengine_platform::tombstone::note_for(pid);

    let Ok(said) = std::fs::read_to_string(&note) else {
        return;
    };

    for line in said.lines().filter(|line| !line.trim().is_empty()) {
        let _ = writeln!(std::io::stderr(), "{line}");
    }

    let _ = std::fs::remove_file(&note);
}

/// What `mix uninstall` exits with when a process is in the way — the T182 design, D4.
const BLOCKED: u8 = 3;

/// What `mix uninstall` was asked to do: its six flags, on
/// [`SelfUpdateAsk`](crate::commands::self_update::SelfUpdateAsk)'s precedent.
#[derive(Debug, Clone, Copy)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "six independent command-line switches, each named where it is read"
)]
pub(crate) struct UninstallAsk {
    pub(crate) dry_run: bool,
    pub(crate) keep_home: bool,
    pub(crate) keep_relocated: bool,
    pub(crate) package: bool,
    pub(crate) relocated: bool,
    pub(crate) blocked: bool,
    pub(crate) yes: bool,
    pub(crate) no_wait: bool,
}

/// How long `mix uninstall` waits for the daemon it just ended.
///
/// It has a shutdown budget of its own out of `config.toml` and then removes several directories, so
/// this is generous rather than tight: what a short wait would buy is a false *"still there"* on a
/// slow machine, which is the one wrong answer this command must not give.
const GOING: std::time::Duration = std::time::Duration::from_secs(60);

/// How long a finished uninstall waits for the daemon's *process* to end once its endpoint has —
/// the T182b design, D8. Removing a home with runtimes in it is the slow part, and a machine with a
/// large `runtimes/` is exactly the one where giving up early would be a false *"still running"*.
const PROCESS_GONE: std::time::Duration = std::time::Duration::from_secs(120);

/// The paths the daemon said it was taking with it that are still on disk once it has gone.
///
/// **A process cannot measure the removal of the directory it is running out of**, so the daemon
/// names these and the client reads them back — which is what makes this command's exit code mean
/// *nothing is left behind* rather than *the daemon said so* (the T87 design, D9).
///
/// **And the paths are waited for, not read once.** The endpoint stops answering the moment the
/// daemon commits to going, which is a long way before it has stopped its services, checkpointed the
/// database, dropped the home lock and removed these directories. Reading at that moment reported
/// every path as left behind on a run where nothing was — measured on CI's Windows runner on
/// 2026-09-04, where the removal was complete a fraction of a second later and the command still
/// exited non-zero.
///
/// A daemon that is still there when the budget runs out has not removed them, and what this answers
/// is the honest thing: they are still there.
///
/// **A daemon whose process has ended will not remove anything more**, so once `daemon` has gone the
/// paths are read one last time and answered, rather than waited on for the rest of the budget
/// (T182b: a home held open by File Explorer kept the uninstaller waiting a minute for nothing).
///
/// **And it says that it is waiting** (T182b). Removing a home of a gigabyte takes a minute on
/// Windows, and a person watching the uninstaller saw nothing move for all of it: one line when the
/// wait begins, with how much is going, and one every `STILL` after.
async fn left_behind(
    report: &UninstallReport,
    endpoint: &Endpoint,
    daemon: Option<(u32, mixengine_platform::process::StartTime)>,
    removing: u64,
) -> Vec<String> {
    /// How often a line says the removal is still going.
    const STILL: std::time::Duration = std::time::Duration::from_secs(10);

    /// How often the paths are looked at while the daemon finishes.
    const STEP: std::time::Duration = std::time::Duration::from_millis(100);

    let going: Vec<&str> = report
        .items
        .iter()
        .filter(|item| matches!(item.outcome, Removal::OnExit { .. }))
        .map(|item| item.location.as_str())
        .collect();

    // T182, D1: a finished uninstall ends the daemon even when nothing of its own is going, and
    // this waits for that too — a script reading the exit code should not find the daemon still up.
    let finished = !report.items.iter().any(|item| {
        matches!(
            item.outcome,
            Removal::Failed { .. } | Removal::Enqueued { .. }
        )
    });

    if going.is_empty() {
        if finished && !Client::gone(endpoint, GOING).await {
            report_left("this home's daemon is still running after a finished uninstall");
        }

        return Vec::new();
    }

    let _ = writeln!(
        std::io::stderr(),
        "{}",
        render::uninstall::uninstall_removing(going.len(), removing)
    );

    if !Client::gone(endpoint, GOING).await {
        report_left("this home's daemon is still running, so nothing of its own has been removed");
    }

    let began = tokio::time::Instant::now();
    let deadline = began + GOING;
    let mut said = began;

    loop {
        // Asked before the paths are read, so the reading after it is one the daemon cannot change.
        let ended = daemon.is_some_and(|(pid, began)| has_ended(pid, began));

        // A tombstone beside a path is that path half-removed (T182, D6), and counts as left.
        let mut left: Vec<String> = going
            .iter()
            .flat_map(|path| {
                let path = std::path::Path::new(path);

                path.exists()
                    .then(|| path.to_path_buf())
                    .into_iter()
                    .chain(mixengine_platform::tombstone::tombstones_beside(path))
                    .map(|left| left.display().to_string())
            })
            .collect();
        left.sort();
        left.dedup();

        if left.is_empty() || ended || tokio::time::Instant::now() + STEP >= deadline {
            return left;
        }

        if said.elapsed() >= STILL {
            said = tokio::time::Instant::now();
            let _ = writeln!(
                std::io::stderr(),
                "{}",
                render::uninstall::uninstall_still_removing(began.elapsed())
            );
        }

        tokio::time::sleep(STEP).await;
    }
}

/// One line on stderr about something that is still on this machine.
///
/// Standard error, beside the report rather than inside it: stdout carries the daemon's answer, and
/// this is what the client found afterwards.
fn report_left(what: &str) {
    let _ = writeln!(std::io::stderr(), "still there: {what}");
}

/// Ask, once, in front of the plan that was just printed.
///
/// **`--json` never asks**, on `confirmed`'s rule and for its reason: a caller reading JSON has
/// nobody at the keyboard by construction, and answering for them either way is worse than refusing
/// — yes would remove a home nobody agreed to lose, and no would be a decline the caller could not
/// tell from an uninstall that happened.
pub(crate) fn agreed_to_uninstall(
    planned: &UninstallReport,
    keep_home: bool,
    keep_relocated: bool,
    json: bool,
) -> Result<bool, Error> {
    if json {
        return Err(unanswered());
    }

    // The relocated directories are only worth a clause of their own when there are some (T182, D2).
    let moved = planned.items.iter().any(|item| {
        item.id == ResidueId::RelocatedDirectory
            && !item.location.contains(mixengine_platform::tombstone::MARK)
    });

    let question = match (keep_home, moved, keep_relocated) {
        (true, false, _) => {
            "undo everything MixEngine has done to this machine, and keep this home?"
        }
        (false, false, _) => {
            "remove MixEngine from this machine, including this home and every database in it?"
        }
        (true, true, true) => {
            "undo everything MixEngine has done to this machine, and keep this home and the \
             directories moved out of it?"
        }
        (true, true, false) => {
            "undo everything MixEngine has done to this machine, keep this home, and remove the \
             directories moved out of it?"
        }
        (false, true, true) => {
            "remove MixEngine from this machine, including this home and every database in it, but \
             keep the directories moved out of it?"
        }
        (false, true, false) => {
            "remove MixEngine from this machine, including this home, the directories moved out of \
             it, and every database in them?"
        }
    };

    match confirm::ask(&format!("\n{question} [y/N] ")) {
        confirm::Answer::Yes => Ok(true),

        confirm::Answer::No => {
            // On stderr, beside the question it answers. Stdout carries what the command was asked
            // for, which was the plan above.
            let _ = writeln!(
                std::io::stderr(),
                "nothing was removed; {} row(s) are still where they were",
                planned.items.len()
            );

            Ok(false)
        }

        confirm::Answer::Unanswerable => Err(unanswered()),
    }
}
