//! `mixengined` — the only process that owns state. Clients are thin; this is not.

mod adopt;
mod api;
mod args;
mod autostart;
mod background;
mod bin_watch;
mod blueprints;
mod certs;
mod crash;
mod credentials;
mod databases;
mod detach;
mod diagnostics;
mod disk;
mod dns;
mod doctor;
mod domains;
mod elevation;
mod error;
mod extensions;
mod helper;
mod jobs;
mod logging;
mod mdns;
mod metrics;
mod packages;
mod php_extensions;
mod projects;
mod repair;
mod requirements;
mod runtimes;
mod secrets;
mod serve;
mod services;
mod shims;
mod sites;
mod storage;
mod uninstall;
mod updates;

use std::io::{IsTerminal, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Context as _;
use clap::Parser;
use mixengine_core::{Home, Store, config};
use mixengine_platform::ipc;

use args::{Args, Launch};
use detach::{detach, take_over};
use error::ToWire as _;
use serve::{CONFIRMATION_REPRIEVE, serve};

/// What `--version` prints. `mix`'s reason, in the binary a service manager starts: a build that is
/// not a release says so, because it keeps a home of its own (ADR 0024).
const VERSION: &str = if mixengine_platform::RELEASE {
    env!("CARGO_PKG_VERSION")
} else {
    concat!(env!("CARGO_PKG_VERSION"), " (development build)")
};

fn main() -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("Error: cannot start the async runtime: {error}");
            return ExitCode::FAILURE;
        }
    };

    let code = runtime.block_on(exit_code());

    // **Not the runtime's own drop**, which waits for every blocking thread still running, for as
    // long as it runs — CI run 37679651605. By here the store is closed, the lock released and every
    // `spawn_blocking` somebody awaited has returned; what is left is work whose owner was stopped at
    // shutdown, and a daemon that has finished must not stay alive for it.
    runtime.shutdown_background();

    code
}

/// [`run`], and what its failure is turned into.
async fn exit_code() -> ExitCode {
    let Err(error) = run().await else {
        return ExitCode::SUCCESS;
    };

    // **`mix` sends whoever reads its message to this file**: a daemon that stopped before it
    // listened is reported as "logs/daemon.log says why". Until this line it did not. The reason
    // went to this process's stderr alone, and nobody holds it — the client that autostarted the
    // daemon has already given up waiting on the endpoint, and a daemon a service manager started
    // has no stderr at all. Measured on an upgrade that could not migrate its database: the log
    // ended at the line before the failure, and the only way to read the reason was to run
    // `mixengined` by hand.
    //
    // **An event rather than a returned `Result`, and that is what removes a second copy.** The
    // subscriber writes to stderr as well as to the file — one filter for both, deliberately, see
    // `logging::subscriber` — so returning the error as well would put a `tracing` line and
    // `anyhow`'s own `Error:` line on the same terminal. The event is also the better of the two
    // for a machine: under `log.format = "json"` the collector reading this daemon's stderr gets
    // one more object, where a bare `Error:` line is prose it cannot parse.
    //
    // Flattened onto one line, because the log is read a line at a time and a wire error's `hint`
    // arrives with a newline in front of it.
    if logging::started() {
        tracing::error!(
            error = format!("{error:#}").replace('\n', "; "),
            "mixengined stopped"
        );
    } else {
        // Nothing is installed yet, so an event would go nowhere. Everything that fails this early
        // — the home directory, `config.toml`, opening the log — fails with somebody watching this
        // stream, and the two-line shape with the hint under the message is what they should read.
        eprintln!("Error: {error:#}");
    }

    ExitCode::FAILURE
}

#[warn(clippy::cognitive_complexity)]
async fn run() -> anyhow::Result<()> {
    // First line of the process, so that `daemon.status` answers when this daemon *started* rather
    // than when it finished starting: creating a home, running the migrations and opening SQLite
    // are seconds a user would otherwise never see in `uptime`.
    let started = api::Started::now();

    let mut args = Args::parse();

    // Refused before the home is opened when no recorded row could make it right — a
    // release on a system that is not Linux asked to keep its passwords in a file.
    credentials::refuse_early(
        mixengine_platform::RELEASE,
        std::env::consts::OS,
        args.credential_store,
    )
    .map_err(anyhow::Error::msg)?;

    // Before anything else: find the home directory, read config.toml, create what is missing.
    // It happens before logging is set up because the log level is one of the things it reads —
    // a failure here is reported by `main` returning it, not by a logger that does not exist yet.
    let host = mixengine_platform::host();

    // A development checkout's suggested home, weighed here at the binary's edge and not inside
    // `mixengine_core::paths::resolve_root` — ADR 0040. The library reads no environment, which is
    // what lets every test hand it a mock host and know which home it gets; `--home` and
    // `MIXENGINE_HOME` arrive through clap for the same reason.
    if args.home.is_none() {
        args.home = mixengine_platform::home::development_home(host.as_ref());
    }
    // Through the wire mapping even though there is no wire yet: the boundary is the only place a
    // hint is written, and a startup failure — the wrong MIXENGINE_HOME, a `[paths]` override onto
    // a disk nobody mounted — is exactly the kind that needs one. Whoever is reading stderr now
    // gets the same sentence a client would get later.
    // **Before `open_home`, deliberately.** This one answers a question *about* a home rather than
    // working in one, and the home it is asked about is routinely a home that does not exist yet.
    // Everything below this line creates something.
    if args.storage {
        let report = storage::report(args.home.as_deref(), host.as_ref()).await?;

        // One line, so that a caller reading stdout gets one document and not a pretty-printed
        // stream it has to find the end of.
        println!(
            "{}",
            serde_json::to_string(&report).context("the storage report could not be rendered")?
        );

        return Ok(());
    }

    let home = mixengine_core::open_home(args.home.as_deref(), host.as_ref())
        .map_err(|error| error.to_wire())?;

    // Computed here rather than inside `serve`, because two of the three ways out of this function
    // need it before anything is bound: `--detach` waits on it, and a daemon that finds the lock
    // taken prints it.
    let endpoint = ipc::Endpoint::in_run_dir(home.paths.run()).map_err(|error| error.to_wire())?;

    // Deliberately before `logging::init`, and it is the *duration* that decides it rather than the
    // number of writers. This process is not a daemon: it lives alongside the one it starts for as
    // long as that one takes to come up, which is precisely when the daemon is writing its startup
    // lines and may rotate the file out from under a second writer. It also has nothing to say that
    // belongs in a daemon's log — one line on stdout is its entire output, and that is what the
    // person who typed the command is reading.
    //
    // The daemon that finds the lock taken below is the other way round on both counts: two lines
    // and gone in milliseconds, and the two lines are worth keeping, because "somebody tried to
    // start a second daemon at 3am" is the kind of thing the log exists to answer.
    if args.detach {
        return detach(&args, &home.paths, &endpoint).await;
    }

    start_logging(&args, &home)?;

    say_what_config_no_longer_does(&home.config);

    // **Before `Store::open`, and that ordering is the point of taking it here.** `sqlx-sqlite`
    // implements the migration lock as a no-op, SQLite having no advisory lock to use, so two
    // daemons that both got as far as opening the database could both read the schema as behind and
    // both migrate it. A single-instance lock acquired afterwards would guard nothing.
    let Some(lock) = take_over(home.paths.lock_file(), &endpoint).await? else {
        // Not a failure, and the exit status says so. The caller asked for a running daemon for
        // this home and there is one — `docs/architecture/daemon-and-ipc.md` has this print the
        // endpoint and stop, which is also what makes two clients autostarting at the same instant
        // produce one daemon and no error message.
        println!("{endpoint}");
        return Ok(());
    };

    // Through the same mapping as `open_home`, and for the same reason: a database that will not
    // open is a startup failure whose way out — a home directory that moved, a copy taken before
    // the last upgrade — is written at the boundary and nowhere else.
    let store = Store::open(home.paths.database_file())
        .await
        .map_err(|error| error.to_wire())?;

    tracing::info!(database = %store.file().display(), "database open and up to date");

    let (credential_store, credential_facts) =
        choose_credentials(&args, &store, &home.paths).await?;

    let home = apply_storage(&args, &store, home, host.as_ref()).await?;

    // Everything that runs with the database open lives in `serve`, and its result is held rather
    // than propagated with `?`, so that the close below is on the only way out — the transport
    // fails to bind whenever something else is already listening, and a `?` there would skip the
    // checkpoint on exactly the exits that matter.
    // Read here rather than inside `serve`, on the same rule the flags follow: the environment and
    // the process's own identity enter the program at `main`. What it is for is finding
    // `mixengine-shim`, which ships beside this binary — see [`mixengine_core::shims::source`].
    let program = std::env::current_exe().context("cannot find the running mixengined binary")?;

    let served = serve(
        &home.paths,
        &store,
        started,
        &endpoint,
        &home.config,
        &args.sources(),
        Launch {
            program,
            credentials: credential_store.at(home.paths.credentials_file()),
            credential_facts,
        },
    )
    .await;

    // Awaited rather than dropped: closing the pool checkpoints the write-ahead log, which is what
    // leaves a single file behind instead of one with a `-wal` sidecar holding the newest commits.
    store.close().await;

    // Released last, and explicitly. While it is held no other daemon can reach the database, which
    // is the property the checkpoint above needs: dropping the lock first would open a window in
    // which the next daemon starts against a file this one is still finishing with.
    drop(lock);

    // **The uninstall's last step, and the last thing this process ever does.** `daemon.uninstall`
    // cannot remove the directory holding the database it has open, so it armed these and stopped
    // the daemon; by here every service has been stopped in dependency order, the write-ahead log
    // has been checkpointed and the lock file is closed, which is the only point at which every
    // handle this process holds inside the home is closed.
    //
    // Empty on every other run.
    if let Ok(armed) = &served
        && !armed.is_empty()
    {
        // **The last thing that may log, and then the log itself.** Windows will not remove a
        // directory holding an open file — not even one already marked for deletion — and
        // `logs/daemon.log` is inside the home this is about to remove. Nothing below writes a line.
        logging::release();

        // **And out of the home.** `detach` starts a background daemon with the home as its working
        // directory, and Windows will not rename a directory some process is standing in — this one
        // included — so every uninstall of a daemon a client had autostarted kept its home (found
        // by the first real Windows uninstall). The temporary directory is outside anything armed;
        // a failure here is left for the rename to report.
        let _ = std::env::set_current_dir(std::env::temp_dir());

        remove_what_the_uninstall_armed(armed, home.paths.bin());
    }

    served.map(|_| ())
}

/// Start the daemon's log, install the crash hook, and say that this daemon is starting.
#[warn(clippy::cognitive_complexity)]
fn start_logging(args: &Args, home: &Home) -> anyhow::Result<()> {
    // **A daemon a service manager started is handed a console it did not ask for**, and on Windows
    // that is a terminal window on the user's desktop at every login — measured under Task
    // Scheduler, where `<Hidden>true</Hidden>` does not stop it either. Nothing happens here when
    // the console is shared with a shell, which is every developer's `mixengined`, and nothing
    // happens on either Unix.
    //
    // **Before the options below rather than after**, so that `is_terminal` answers about the
    // streams this process actually ends up with: colour written into the null device is not wrong,
    // but it is a decision made about a terminal that is no longer there. Logged after
    // `logging::init`, because there is nowhere to say it until then.
    let released = mixengine_platform::process::release_unattended_console();

    // A flag beats the file, and the file beats the default. Neither is read anywhere but here.
    let options = logging::Options {
        file: home.paths.daemon_log_file(),
        level: args
            .log_level
            .map_or(home.config.log.level, config::LogLevel::from),
        format: args
            .log_format
            .map_or(home.config.log.format, config::LogFormat::from),
        // Colour only when a human is watching. The file never gets any — see `logging`.
        colour: std::io::stderr().is_terminal(),
    };

    logging::init(&options).with_context(|| {
        format!(
            "cannot write the daemon log at {}",
            home.paths.daemon_log_file().display()
        )
    })?;

    // After `logging::init`, because the hook's last step is a log line; and
    // before the first line below, so that a panic during the rest of start-up is recorded like any
    // other. A panic *earlier* than this — parsing arguments, resolving the home, reading
    // `config.toml` — gets the default hook, and that is right: those failures happen while
    // somebody is watching stderr, and none of them has a log to be written to yet. The `--detach`
    // parent returns above this and installs nothing, for the same reason.
    crash::Reports::new(&home.paths, home.config.crash.enabled).install();
    #[cfg(feature = "crash-probe")]
    crash::probe::raise_if_asked();

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        protocol = %mixengine_proto::PROTOCOL_VERSION,
        home = %home.paths.root().display(),
        log = %home.paths.daemon_log_file().display(),
        "mixengined starting"
    );

    if released {
        tracing::debug!("released a console this process was the only one attached to");
    }

    Ok(())
}

/// Say which keys in `config.toml` no longer do anything.
#[warn(clippy::cognitive_complexity)]
fn say_what_config_no_longer_does(config: &config::Config) {
    // Said rather than silently ignored: a key that does nothing looks exactly like one that does
    // not work (ADR 0057).
    if config.retired_bin.rescan_seconds.is_some() {
        tracing::info!(
            "`[bin] rescan_seconds` in config.toml no longer does anything: MixEngine notices a \
             tool installed into a runtime as it lands; the line can be removed"
        );
    }
    if config.daemon.retired_ipc_path.is_some() {
        tracing::info!(
            "`[daemon] ipc_path` in config.toml has never done anything: the daemon listens where \
             the platform puts the endpoint, under run/; the line can be removed"
        );
    }
    // A warning rather than the two notes above: whoever set these believed they were steering a
    // check the daemon no longer makes, and `enabled = false` in particular reads as a promise.
    if config.updates.enabled.is_some() || config.updates.check_seconds.is_some() {
        tracing::warn!(
            "`[updates] enabled` and `check_seconds` in config.toml no longer do anything: the \
             daemon never looks for a newer MixEngine on its own, and MixLab checks for itself; \
             the lines can be removed"
        );
    }
}

/// **Which store this home keeps its credentials in.** Read after the database and before the host
/// that reads a credential, so that a start with no flag gets the store the home chose. A release
/// asked for another one switches only if the switch is allowed, and records it; a development
/// build's flag is for this start alone.
#[warn(clippy::cognitive_complexity)]
async fn choose_credentials(
    args: &Args,
    store: &Store,
    paths: &mixengine_core::Paths,
) -> anyhow::Result<(credentials::Store, credentials::Facts)> {
    let release = mixengine_platform::RELEASE;
    let recorded = credentials::recorded(store).await?;
    let credential_store = match credentials::at_start(release, args.credential_store, recorded) {
        credentials::AtStart::Use(chosen) => chosen,
        credentials::AtStart::Switch { from, to } => {
            credentials::check_switch(
                store,
                paths.credentials_file(),
                release,
                std::env::consts::OS,
                from,
                to,
            )
            .await
            .map_err(anyhow::Error::msg)?;
            credentials::record(store, to).await?;
            to
        }
    };
    // Read once for the run: whether a switch would be offered (`daemon.status`'s `choosable`).
    // The operating system's store is asked only where the answer depends on it, a Linux release on
    // it, so no other machine is made to reach its keyring at start.
    let os_store_absent = release
        && std::env::consts::OS == "linux"
        && credential_store == credentials::Store::Os
        && tokio::task::spawn_blocking(|| {
            credentials::os_store_absent(mixengine_platform::host().keyring())
        })
        .await
        .unwrap_or(false);
    let credential_facts = credentials::Facts {
        store: credential_store,
        choosable: credentials::choosable(
            release,
            std::env::consts::OS,
            credential_store,
            os_store_absent,
        ),
    };

    Ok((credential_store, credential_facts))
}

/// Honour the `[paths]` flags this start was given, and return the home as it now is.
///
/// Here rather than inside `open_home`: whether these flags may be honoured is a question for the
/// database, and `open_home` never opens one. What makes the ordering safe is that `mixengine.db`
/// is the one file `[paths]` cannot move, so the store was opened at a path no relocation can
/// change underneath this decision.
#[warn(clippy::cognitive_complexity)]
async fn apply_storage(
    args: &Args,
    store: &Store,
    home: Home,
    host: &dyn mixengine_platform::Host,
) -> anyhow::Result<Home> {
    let home = match storage::apply(
        store,
        home.paths.config_file(),
        &home.config.paths,
        &args.requested_paths(),
    )
    .await?
    {
        storage::Applied::Nothing => home,

        // Read again rather than patched in place. `open_home` is idempotent and documented as
        // such — it is what `mix doctor` reuses — so this is the honest way to pick up a layout
        // that changed a moment ago: resolve, read the file that was just written, and let
        // `bootstrap` create what the new value names.
        //
        // `endpoint` and `lock` above are **not** recomputed, and do not need to be: both come out
        // of `run/`, which is the directory `[paths]` refuses to move, so a second read answers
        // with what this process is already holding.
        storage::Applied::Written(_) => {
            let moved = mixengine_core::open_home(args.home.as_deref(), host)
                .map_err(|error| error.to_wire())?;

            // The default layout was created a moment ago by the `open_home` at the top of this
            // function, because the window these flags need is a question for a database that did
            // not exist yet. What that leaves behind is an empty directory in the home, at exactly
            // the path somebody who just chose a disk would go looking for their data.
            storage::tidy(&home.paths, &moved.paths);

            moved
        }
    };

    Ok(home)
}

/// Remove the directories a finished uninstall named, and say why about any that stay.
///
/// **Standard error and a note outside the home, not the log**, for the obvious reason: the log is
/// one of the things being removed. A daemon started in the background has nowhere for standard
/// error to go, so the same lines go to
/// [`tombstone::note_for`](mixengine_platform::tombstone::note_for), where `mix` reads them.
/// `mix uninstall` reads these paths back once this process is gone, which is what makes its exit
/// code mean *nothing is left behind* rather than *the daemon said so* — so a failure here is
/// reported by the client whether or not anybody reads this line.
///
/// **All of them or none**: every directory is renamed aside first, and one that refuses — a file
/// held open inside it, a shell whose working directory it is — puts every other one back, so no
/// database is ever left half deleted. What could be renamed and then not deleted stays as a
/// tombstone the next uninstall finds.
///
/// A path that is already gone is not a failure: on a home with no relocation the root removes
/// everything under it, and a `[paths]` entry pointing inside the root would be removed with it.
///
/// `bin/` goes out on its own first — it is on `PATH`, and an editor watching it would otherwise
/// keep the whole home (measured with VS Code) — and on a refusal, whatever else another program
/// holds that can be moved goes out on its own too. See
/// [`remove_all_or_nothing`](mixengine_platform::tombstone::remove_all_or_nothing).
fn remove_what_the_uninstall_armed(armed: &[PathBuf], bin: &Path) {
    use mixengine_platform::tombstone::{PATIENCE, QUICK, remove_all_or_nothing};

    let pid = std::process::id();
    let note = mixengine_platform::tombstone::note_for(pid);

    // A note left by an earlier process that had this pid would be read as this one's.
    let _ = std::fs::remove_file(&note);

    // **Once as before, with `bin/` lifted**, and on a machine nothing else holds that is the whole
    // of it — no scan is paid for. **On a refusal, look**: everything was put back, so what other
    // programs hold is read, whatever can be moved is lifted out too, and the renames go again with
    // the full patience. What cannot be moved is named below.
    let mut lifted = vec![bin.to_path_buf()];
    let mut stuck: Vec<mixengine_platform::occupants::HeldItem> = Vec::new();

    let outcome = match remove_all_or_nothing(armed, &lifted, pid, QUICK) {
        Err(_) => {
            let held = mixengine_platform::occupants::held_under(armed, Some(pid));
            let (movable, unmovable): (Vec<_>, Vec<_>) =
                held.into_iter().partition(|item| item.movable);
            lifted.extend(movable.into_iter().map(|item| item.path));
            stuck = unmovable;
            remove_all_or_nothing(armed, &lifted, pid, PATIENCE)
        }
        done => done,
    };

    let lines: Vec<String> = match outcome {
        Ok(left) => left
            .into_iter()
            .map(|leftover| {
                format!(
                    "mixengined: {} could not be removed ({}). the next uninstall removes it",
                    leftover.path.display(),
                    leftover.error
                )
            })
            .collect(),
        // **What refused, and who, before where**: an uninstaller's log cuts a long line off, and
        // the path is the part a person can most easily do without. The handle table's answer comes
        // first, since it names the program and says it cannot be moved; `first_held`'s is what is
        // left when the table could not see the holder.
        Err(refused) => {
            let seconds = refused.tried_for.as_secs();
            let named = stuck.iter().find(|item| !item.holders.is_empty());
            vec![match (named, &refused.held) {
                (Some(item), _) => format!(
                    "mixengined: nothing was removed, {} holds {} open, so it cannot be moved or \
                     deleted (tried for {seconds} s). close it, then run the uninstall again",
                    item.holders
                        .iter()
                        .map(|holder| format!("{} ({})", holder.name, holder.pid))
                        .collect::<Vec<_>>()
                        .join(", "),
                    item.path.display()
                ),
                (None, Some(held)) if !held.by.is_empty() => format!(
                    "mixengined: nothing was removed, {} holds {} open (tried for {seconds} s). \
                     close it, then run the uninstall again",
                    held.by.join(", "),
                    held.path.display()
                ),
                (None, Some(held)) => format!(
                    "mixengined: nothing was removed, another program has {} open, such as File \
                     Explorer or a terminal (tried for {seconds} s). close it, then run the \
                     uninstall again",
                    held.path.display()
                ),
                (None, None) => format!(
                    "mixengined: nothing was removed, {} could not be moved aside: {} (tried for \
                     {seconds} s). close any program using it, then run the uninstall again",
                    refused.path.display(),
                    refused.error
                ),
            }]
        }
    };

    if lines.is_empty() {
        return;
    }

    for line in &lines {
        let _ = writeln!(std::io::stderr(), "{line}");
    }

    // For `mix`, which reads it once this process has gone (`tombstone::note_for`).
    let _ = std::fs::write(&note, lines.join("\n") + "\n");
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// A home with `bin/`, `etc/caddy/` and `data/`, each holding a file, and a relocated `logs/`
    /// beside it holding `daemon.log` — the layout the real uninstalls met.
    fn a_home_with_relocated_logs() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().expect("tempdir");
        let home = root.path().join("MixEngine");
        for inner in ["bin", "etc/caddy", "data"] {
            std::fs::create_dir_all(home.join(inner)).expect("a directory");
            std::fs::write(home.join(inner).join("file"), b"x").expect("a file");
        }
        let logs = root.path().join("mixlab_data").join("logs");
        std::fs::create_dir_all(&logs).expect("the relocated logs");
        std::fs::write(logs.join("daemon.log"), b"x").expect("a log");
        (root, home, logs)
    }

    /// A program started *apart from* this test process — not its child — the way VS Code, File
    /// Explorer or a terminal is not the daemon's child: the removal spares the daemon's own
    /// descendants, so a child of the test would be spared and prove nothing. PowerShell's
    /// `Start-Process` starts it hidden and exits, which leaves it with no living parent in this
    /// test's family. `directory` reaches it through the environment rather than inside a quoted
    /// string. Returns its pid, once it can be seen holding `directory`.
    fn apart_holding(directory: &Path, start_process: &str) -> u32 {
        let started = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", start_process])
            .env("T182E_WATCHED", directory)
            .output()
            .expect("PowerShell starts the program");
        let pid: u32 = String::from_utf8_lossy(&started.stdout)
            .trim()
            .parse()
            .unwrap_or_else(|_| {
                panic!(
                    "Start-Process gave no pid: {}",
                    String::from_utf8_lossy(&started.stderr)
                )
            });

        let parent = directory.parent().expect("a parent").to_path_buf();
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while std::time::Instant::now() < deadline {
            let held = mixengine_platform::occupants::held_under(
                std::slice::from_ref(&parent),
                Some(std::process::id()),
            );
            if held
                .iter()
                .any(|item| item.holders.iter().any(|holder| holder.pid == pid))
            {
                return pid;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        end(pid);
        panic!("pid {pid} was never seen holding {}", directory.display());
    }

    /// A watch on `directory`, from a program apart from this test.
    fn apart_watching(directory: &Path) -> u32 {
        apart_holding(
            directory,
            "(Start-Process powershell -WindowStyle Hidden -PassThru -ArgumentList \
             '-NoProfile','-NonInteractive','-Command',\
             '$w = New-Object IO.FileSystemWatcher $env:T182E_WATCHED; \
             $w.EnableRaisingEvents = $true; Start-Sleep -Seconds 60').Id",
        )
    }

    /// A program apart from this test whose working directory is `directory` — a terminal in it.
    fn apart_standing_in(directory: &Path) -> u32 {
        apart_holding(
            directory,
            "(Start-Process ping -WindowStyle Hidden -PassThru -WorkingDirectory \
             $env:T182E_WATCHED -ArgumentList '-n','60','127.0.0.1').Id",
        )
    }

    fn end(pid: u32) {
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }

    /// End to end: other programs watching `bin/` and `etc/caddy/` — VS Code and File
    /// Explorer on the machine that found this — do not keep the home. The removal moves both
    /// watched folders out on its own and every armed directory goes, relocated `logs/` included.
    #[test]
    fn watched_folders_do_not_keep_the_home() {
        // Windows alone refuses the rename of a folder another program holds.
        if !cfg!(windows) {
            return;
        }
        let (_root, home, logs) = a_home_with_relocated_logs();
        let watchers = [
            apart_watching(&home.join("bin")),
            apart_watching(&home.join("etc").join("caddy")),
        ];

        remove_what_the_uninstall_armed(&[home.clone(), logs.clone()], &home.join("bin"));

        watchers.into_iter().for_each(end);
        assert!(!home.exists(), "the home was kept");
        assert!(!logs.exists(), "the relocated logs were kept");
        assert!(
            mixengine_platform::tombstone::tombstones_beside(&home).is_empty(),
            "{:?}",
            mixengine_platform::tombstone::tombstones_beside(&home)
        );
    }

    /// End to end: a program standing in `data/` cannot be moved past, and the removal
    /// puts every directory back — nothing half deleted — and names it in the note `mix` reads.
    #[test]
    fn a_program_standing_in_the_home_keeps_all_of_it() {
        if !cfg!(windows) {
            return;
        }
        let (_root, home, logs) = a_home_with_relocated_logs();
        let standing = apart_standing_in(&home.join("data"));

        remove_what_the_uninstall_armed(&[home.clone(), logs.clone()], &home.join("bin"));

        let note =
            std::fs::read_to_string(mixengine_platform::tombstone::note_for(std::process::id()))
                .unwrap_or_default();
        end(standing);

        for kept in ["bin/file", "etc/caddy/file", "data/file"] {
            assert!(home.join(kept).exists(), "{kept} was not put back");
        }
        assert!(
            logs.join("daemon.log").exists(),
            "the relocated logs were not put back"
        );
        assert!(
            note.contains(&format!("({standing})")),
            "the note does not name the program standing in data: {note}"
        );
    }
}
