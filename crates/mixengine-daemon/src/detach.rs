//! Starting in the background, and taking over from a daemon that is on its way out:
//! `--detach` respawns this binary with no console and waits for it to answer, and
//! [`take_over`] waits for the single-instance lock a dying daemon still holds.

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use anyhow::Context as _;
use mixengine_core::Paths;
use mixengine_platform::{ipc, lock, process};
use tokio::time::Instant;

use crate::args::{Args, as_arg};
use crate::error::ToWire as _;

/// How long `--detach` waits for the daemon it started to answer.
///
/// A ceiling and not a wait: the poll returns the moment the endpoint answers. It is generous
/// because the first start of a home creates the directory tree, runs the migrations and opens
/// SQLite, and because the machine this has to be reliable on is a loaded CI runner.
const DETACH_TIMEOUT: Duration = Duration::from_secs(30);

/// How often it asks during that.
const DETACH_POLL: Duration = Duration::from_millis(50);

/// How long a daemon that finds the lock taken waits for the holder to either answer or let go.
///
/// **The holder of the lock is not always a daemon that is running.** A daemon closes its endpoint
/// first and releases the lock last — between the two it drains its clients ([`CLIENT_GRACE`]) and
/// checkpoints the write-ahead log — so a daemon started inside that window finds the lock held by
/// a process that will never answer. Standing aside there, as this used to, meant nobody started:
/// `mix self-update` waits for the endpoint to go quiet and then starts the new daemon, which lands
/// exactly inside the window, and its `--detach` then waited the whole of [`DETACH_TIMEOUT`] for a
/// daemon nobody was going to start (measured on CI). `mix daemon stop` followed by any command
/// that autostarts is the same handoff by hand.
///
/// Sized to cover a holder that is leaving — the clients, then the checkpoint, with room for a loaded
/// machine — and a fraction of [`DETACH_TIMEOUT`], since a `--detach` parent is usually what is
/// waiting behind this. A holder that neither answers nor leaves inside it is one still starting
/// up — roadmap task T10's window, `Store::open` mid-migration — and standing aside for that one
/// stays right: it will answer, and the parent waits for it.
///
/// [`CLIENT_GRACE`]: crate::CLIENT_GRACE
const HANDOFF: Duration = Duration::from_secs(5);

/// How often the lock is asked for again during that.
const HANDOFF_POLL: Duration = Duration::from_millis(50);

/// Start a daemon in the background and wait until it answers.
///
/// The child is this same binary started again without `--detach`, rather than a fork: Windows has
/// no `fork`, and forking a process that already has a Tokio runtime — several threads, a reactor,
/// a pool of locks held by threads that do not exist in the child — is a way of producing a daemon
/// that hangs on its first `await`. One mechanism on all three systems also means one code path to
/// keep working on them.
///
/// The arguments are rebuilt from what was parsed rather than filtered out of `args_os`, which
/// matters most for the home: the child is told the *resolved* root, so a `--home` given as a
/// relative path, or one that came from `MIXENGINE_HOME`, cannot be re-resolved by the child against
/// a working directory or an environment that is not the same one.
pub(crate) async fn detach(
    args: &Args,
    paths: &Paths,
    endpoint: &ipc::Endpoint,
) -> anyhow::Result<()> {
    // Asked before anything is started, because the common case for the caller this flag exists for
    // is that a daemon is *already* running: a client autostarts one whenever it cannot reach the
    // endpoint, and two clients doing that at once means the second one arrives to a daemon that is
    // up. Spawning a process whose entire job would be to find the lock taken and exit is a cost
    // with nothing on the other side of it.
    if ipc::Connection::connect(endpoint).await.is_ok() {
        println!("{endpoint}");
        return Ok(());
    }

    let program = std::env::current_exe().context("cannot find the running mixengined binary")?;

    let mut arguments = vec![
        OsString::from("--home"),
        paths.root().as_os_str().to_owned(),
    ];

    if let Some(level) = args.log_level {
        arguments.push("--log-level".into());
        arguments.push(as_arg(level).into());
    }

    // Passed explicitly even though the child inherits `MIXENGINE_LOG_FORMAT` with the rest of the
    // environment: a `--log-format` given on the command line has to beat that variable in the
    // child exactly as it did here, and only an argument does that.
    if let Some(format) = args.log_format {
        arguments.push("--log-format".into());
        arguments.push(as_arg(format).into());
    }

    // The same rule again, and it matters more here than for the log format: a mirror named on the
    // command line has to reach the child, or a `--detach`ed daemon would quietly go back to the
    // published index and refuse the mirror's signature — which is the one failure a person setting
    // these would have no way of explaining.
    if let Some(url) = &args.index_url {
        arguments.push("--index-url".into());
        arguments.push(url.into());
    }
    if let Some(key) = &args.index_key {
        arguments.push("--index-key".into());
        arguments.push(key.into());
    }

    // And the same for the update feed, for exactly the reason above one step along: a `--detach`ed
    // daemon that went back to the published feed would refuse the staging feed's signature, and
    // nobody would be able to explain why.
    if let Some(url) = &args.update_url {
        arguments.push("--update-url".into());
        arguments.push(url.into());
    }
    if let Some(key) = &args.update_key {
        arguments.push("--update-key".into());
        arguments.push(key.into());
    }

    // And the four relocations — roadmap task **T144**. They have to reach the child rather than
    // being applied here, because the child is the process that opens the database and only a
    // process that has opened one can know whether these may still be honoured. A `--detach` that
    // wrote them itself would be answering that question without having asked it — and a
    // `--detach` that dropped them would be a flag that works in the foreground and silently does
    // nothing through the one caller that starts most daemons.
    //
    // The name of each flag is the name of the key it writes, which is why this can be a loop over
    // the same list `config.toml` is written from rather than four blocks like the ones above.
    let requested = args.requested_paths();
    for (key, directory) in requested.entries() {
        if let Some(directory) = directory {
            arguments.push(format!("--{key}").into());
            arguments.push(directory.as_os_str().to_owned());
        }
    }

    // The home, and deliberately not this process's working directory. A daemon holds its working
    // directory for days, and the directory a client autostarting one happens to be in is a project
    // folder somebody is working in — which they would then be unable to rename or delete on
    // Windows. The home is the one directory the daemon is entitled to pin, and it exists by now
    // because `open_home` has just created it.
    let mut daemon = process::spawn_detached(
        &program,
        &arguments,
        paths.root(),
        &std::collections::BTreeMap::new(),
    )
    .map_err(|error| error.to_wire())?;

    let deadline = Instant::now() + DETACH_TIMEOUT;

    // Kept rather than re-asked, so the loop below reads as one question answered once: the child is
    // running until it is not, and what it exited with does not change afterwards.
    let mut exit: Option<process::Exit> = None;

    loop {
        // Dialling the endpoint rather than asking `/health`: what a client needs to know is that
        // there is something at the other end to send a request to, and a connection proves that
        // without this process having to speak HTTP at all.
        if ipc::Connection::connect(endpoint).await.is_ok() {
            println!("{endpoint}");
            return Ok(());
        }

        // "Not up yet" and "gone" are the same silence, and only one of them is worth waiting out.
        if exit.is_none() {
            exit = daemon.exited().map_err(|error| error.to_wire())?;
        }

        // A child that *failed* has nothing left to wait for, and saying so at once is the whole
        // point of watching it. A child that succeeded is the opposite case and must not be treated
        // as this one: it exits 0 precisely when another daemon already holds this home — and that
        // daemon takes the lock **before** it opens SQLite, so between those two moments there is a
        // whole set of migrations during which the endpoint is legitimately not answering yet. This
        // used to end the wait there and turn two clients autostarting at the same instant (roadmap
        // task T10) into a failure for whichever of them lost the race. The deadline below is what
        // that case waits on, and is generous for exactly this reason.
        if let Some(status) = &exit
            && !status.is_success()
        {
            anyhow::bail!(
                "the daemon stopped without listening on {endpoint} ({status}); {} says why",
                paths.daemon_log_file().display()
            );
        }

        if Instant::now() >= deadline {
            return Err(match exit {
                // It stood aside for a daemon that then never answered. The lock file names the one
                // to go and look at, which is not a process this command started.
                Some(status) => anyhow::anyhow!(
                    "another daemon holds {} and did not start listening on {endpoint} within \
                     {DETACH_TIMEOUT:?}; the one started here stood aside for it ({status}), and \
                     {} says what it is doing",
                    paths.lock_file().display(),
                    paths.daemon_log_file().display()
                ),

                None => anyhow::anyhow!(
                    "the daemon (pid {}) did not start listening on {endpoint} within \
                     {DETACH_TIMEOUT:?}; it is still running, and {} says what it is doing",
                    daemon.pid(),
                    paths.daemon_log_file().display()
                ),
            });
        }

        tokio::time::sleep(DETACH_POLL).await;
    }
}

/// Take the single-instance lock, waiting out a holder that is on its way out.
///
/// `None` when another daemon has this home: it answered on `endpoint`, or it held the lock for the
/// whole of [`HANDOFF`] without answering, which is a daemon that is still starting. Either way the
/// caller prints the endpoint and exits 0, which is what a client autostarting a daemon asked for.
///
/// **The endpoint is asked only once the lock has refused**, and the lock is asked first on every
/// turn: a daemon that answers *is* the holder, so a `Some` here can never be a second daemon
/// beside a live one — that is the guarantee the lock exists for, and dialling the endpoint adds a
/// way of standing aside sooner, not a way of proceeding.
///
/// # Errors
///
/// Whatever [`lock::Lock::acquire`] reported — a `run/` that cannot be written, or a lock the OS
/// refused for a reason other than somebody holding it.
pub(crate) async fn take_over(
    path: &Path,
    endpoint: &ipc::Endpoint,
) -> anyhow::Result<Option<lock::Lock>> {
    let deadline = Instant::now() + HANDOFF;
    let mut waiting_on = None;

    loop {
        let holder = match lock::Lock::acquire(path).map_err(|error| error.to_wire())? {
            lock::Acquired::Held(lock) => {
                if let Some(holder) = waiting_on {
                    tracing::info!(%holder, "the daemon that held this home has left; taking it over");
                }

                return Ok(Some(lock));
            }

            lock::Acquired::Taken(holder) => holder,
        };

        if ipc::Connection::connect(endpoint).await.is_ok() || Instant::now() >= deadline {
            tracing::info!(%holder, %endpoint, "a daemon is already running for this home");
            return Ok(None);
        }

        // Said once, on the first refusal, so the log explains a start that takes a few seconds
        // without saying so fifty times over.
        if waiting_on.is_none() {
            tracing::info!(
                %holder,
                %endpoint,
                within = ?HANDOFF,
                "another daemon holds this home and is not answering; waiting for it to answer or \
                 to let go"
            );
        }

        waiting_on = Some(holder);

        tokio::time::sleep(HANDOFF_POLL).await;
    }
}
