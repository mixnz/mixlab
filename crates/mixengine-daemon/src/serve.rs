//! A running daemon: everything it is made of, built in the order a start needs, the
//! clients it accepts, and the shutdown that ends it.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use mixengine_core::{Paths, Store, config};
use mixengine_platform::{ipc, signal};
use tokio_util::sync::CancellationToken;

use crate::args::{Launch, Sources};
use crate::error::ToWire as _;
use crate::{
    api, autostart, background, bin_watch, crash, credentials, dns, elevation, jobs, mdns,
    services, shims,
};

mod accept;
mod clients;
mod clocks;
mod machine;
mod projections;
mod recovery;
mod repairs;
mod started;

/// How long to wait before accepting again after `accept` itself failed.
///
/// A failure there is nearly always about one connection — a client that died between the kernel
/// queueing it and us asking who it was — and must not end the accept loop, which would take the
/// daemon and every service it supervises down with it. But the failures that are *not* per
/// connection would spin this loop at the speed of the CPU, so the retry is paced.
const ACCEPT_PAUSE: Duration = Duration::from_millis(200);

/// How long a shutting-down daemon waits for the connections that are still open.
///
/// Deliberately shorter than the seconds `daemon.shutdown` gives a *service* to stop: a service is
/// flushing a database, while a client is finishing one local request. What used to consume this
/// budget was `GET /events`, which never ends on its own; it now ends with the root token, so this
/// is the margin for a request that is genuinely mid-flight rather than the normal cost of shutting
/// down — including the answer to `daemon.shutdown` itself, which is written into a connection this
/// daemon has already decided to stop waiting for.
///
/// **The `daemon.shutdown` path's number, and since the signal path got its own, only that
/// path's.** A shutdown the OS asked for has no answer to write and arrives with a clock running —
/// see [`SIGNAL_CLIENT_GRACE`].
pub(crate) const CLIENT_GRACE: Duration = Duration::from_secs(2);

/// The same, for the shutdown an operating system asked for.
///
/// **A quarter of [`CLIENT_GRACE`], and the one difference between the two paths is what pays for
/// it.** `daemon.shutdown` has an answer to write into one of these connections, and the two seconds
/// above are mostly for that; a console control event has no answer to write to anybody. What is
/// left holding a connection when one arrives is a `mix status` mid-request — a local round trip
/// measured in milliseconds — or a client sitting on a keep-alive socket between requests, which
/// nobody is going to write to again and which dropping the set already handles correctly. Waiting
/// the full two seconds on *that* is two seconds taken from the WAL checkpoint on the one system
/// where something else is counting, which is the trade [`CEILING_RESERVE`] describes.
const SIGNAL_CLIENT_GRACE: Duration = Duration::from_millis(500);

/// What [`CEILING_RESERVE`] keeps back for `Store::close`.
///
/// The checkpoint is the step whose overrun actually loses something: a process terminated in the
/// middle of it leaves the `-wal` sidecar holding the newest commits, and every other number here
/// exists to make sure this one is reached. A second is generous against a database of service rows
/// and settings, and it is deliberately not measured from a run on this machine — what it has to
/// survive is a laptop resuming from sleep with a hundred processes asking for the disk at once.
const CHECKPOINT_MARGIN: Duration = Duration::from_secs(1);

/// What [`CEILING_RESERVE`] keeps back for nothing in particular.
///
/// Every other part of the reserve is a wait this daemon performs and can therefore be sure of.
/// This one covers what it cannot: a task the runtime schedules late because eight runners are
/// finishing at once, a `tracing` line that blocks while the log file rotates, a `join_next` that
/// returns a moment after the last connection did. The reserve used to have none of this — the
/// arithmetic came to exactly the ceiling — so the process was one slow scheduler away from being
/// terminated mid-checkpoint, which is the outcome the reserve exists to prevent rather than to
/// meet exactly.
///
/// It is also what the two waits a spent budget still permits are spent out of: see [`KILL_GRACE`]
/// and [`CONFIRMATION_REPRIEVE`].
const SCHEDULING_SLACK: Duration = Duration::from_secs(1);

/// What a shutdown keeps back from an operating system's ceiling for everything that is not a
/// service.
///
/// Windows gives a console control handler about five seconds and then ends the process whatever it
/// is doing ([`signal::STOP_CEILING`]), and stopping services is not the last thing a shutdown does:
/// the connections still open get [`SIGNAL_CLIENT_GRACE`], and `Store::close` then checkpoints the
/// write-ahead log, which is what leaves one database file behind instead of one with a `-wal`
/// sidecar holding the newest commits. A budget that spent the whole ceiling on services would be
/// terminated in the middle of exactly that.
///
/// **Summed from its parts rather than chosen, because as one number it left no margin at all.** It
/// was two and a half seconds against a two-second client grace, so the checkpoint had five hundred
/// milliseconds and the whole came to the ceiling exactly: 2.5 s of services, 2 s of clients and
/// 0.5 s of checkpoint is 5 s of 5, with nothing left for a task scheduled late or a machine under
/// load — and `windows/signal.rs` says the ceiling itself may be *shorter* than five where
/// `WaitToKillTimeout` or `HungAppTimeout` were configured. So what it keeps back is now stated as
/// the three things that happen after the last service stops:
///
/// - [`SIGNAL_CLIENT_GRACE`], half a second for the connections still open,
/// - [`CHECKPOINT_MARGIN`], a second for `Store::close` and the write-ahead log,
/// - [`SCHEDULING_SLACK`], a second that is left over on purpose.
///
/// The total is the same 2.5 s, which leaves the services 2.5 s on Windows and puts the daemon at
/// 2.5 + 0.5 + 1 = 4 s of the 5 s the OS allows, one second of it slack. **The margin is bought from
/// the clients rather than from the services**, which is the choice worth naming: shortening the
/// client grace on the signal path costs a client that is between requests nothing, where raising
/// this constant instead would have taken the same second out of MariaDB's flush.
///
/// Subtracted **only** where there is a ceiling to subtract it from. On Unix nothing is counting and
/// the budget is the configured one entire.
pub(crate) const CEILING_RESERVE: Duration = SIGNAL_CLIENT_GRACE
    .saturating_add(CHECKPOINT_MARGIN)
    .saturating_add(SCHEDULING_SLACK);

/// How long a shutdown that was asked to hurry still waits for the services it has just told to
/// kill.
///
/// A second request to stop narrows the budget to nothing, which puts every runner still to reach a
/// stop at the same place: `Supervised`'s kill — `TerminateJobObject` or a `SIGKILL` to the group,
/// and the reap of a child this process is the parent of. Those are syscalls rather than grace
/// periods, so half a second is a great deal of room for them even on a machine that is swapping.
///
/// **Bounded at all, because the whole point of the escalation is that a third signal must not be
/// needed.** A narrowed budget reaches a runner that has not begun its stop, and does not reach one
/// that already has: a grace period is a deadline fixed from what the budget said when the stop
/// began, so a service that was inside one when the second request arrived runs to the end of the
/// time the *first* request granted it. That is the case this bound exists for, and it is the common
/// one — the person is asking again precisely because something is taking its whole grace period.
///
/// What running out costs is stated rather than hidden: the runners still waiting are abandoned
/// rather than aborted, so a service this daemon had asked to stop is left for the next one to meet
/// as the crash recovery it already performs. That is the same bargain `kill -9` would have made —
/// except that this way the database still gets its checkpoint, which is the whole reason the
/// escalation is not a `return`.
///
/// **Bounded at half a second, because [`SCHEDULING_SLACK`] is what pays for it.** The wait arrives
/// on top of a budget that may already have run to the end of its clock, so the worst case on
/// Windows is 2.5 s of services, 0.5 s here, 0.5 s of clients and 1 s of checkpoint — 4.5 s of the
/// 5 s the OS allows. That it fits inside the slack is a test rather than this sentence.
const KILL_GRACE: Duration = Duration::from_millis(500);

/// How far past its own deadline a shutdown may still watch a process it has just killed.
///
/// **The one wait for which zero is not a real answer.** Every other constant the budget shortens is
/// a wait whose absence costs something bounded and stated: a grace period of zero is a service
/// killed at once, a log drain of zero is a tail nobody was reading. The poll after the kill in
/// `Runner::stop_adopted` is different in kind, because what it bounds is not a wait but a
/// *question* — whether the survivor this daemon just killed has left the process table. Asked with
/// no window at all it is asked microseconds after the kill, the kernel has not finished with the
/// process yet, and the answer is read as a survivor that will not go: the row keeps its `stopping`,
/// the stop is reported as failed, and the walk stops there on the ordering rule, so every service
/// after it in the plan is left running by a stop that in fact succeeded.
///
/// **One window for the whole walk rather than an allowance per service**, which is what keeps it
/// out of the ceiling arithmetic: it is expressed as a second deadline this far past the first (see
/// [`Budget`](services::Budget)), so eight survivors reached after the budget ran out cost what one
/// of them costs. Per service it would have been unbounded, and an unbounded term is exactly what
/// [`CEILING_RESERVE`] cannot contain.
///
/// **A quarter second, and paid from [`SCHEDULING_SLACK`] alongside [`KILL_GRACE`].** The worst case
/// on Windows is 2.5 s of services, 0.5 s of escalation, 0.25 s here, 0.5 s of clients and 1 s of
/// checkpoint — 4.75 s of the 5 s the OS allows. That it fits is a test rather than this sentence.
pub(crate) const CONFIRMATION_REPRIEVE: Duration = Duration::from_millis(250);

/// The handles most steps of a start read, borrowed from [`serve`]'s frame.
///
/// Built once the last of them exists, and passed to a step that reads more than two of them,
/// rather than one parameter per handle: the step that builds the signed sources reads ten. Every
/// field is a borrow, so the struct is `Copy` and a step destructures it as `*shared`; a step that
/// changes something — [`background::Background`] — takes it as a parameter of its own.
#[derive(Clone, Copy)]
struct Shared<'a> {
    paths: &'a Paths,
    store: &'a Store,
    config: &'a config::Config,
    host: &'a Arc<dyn mixengine_platform::Host>,
    events: &'a api::Events,
    jobs: &'a Arc<jobs::Jobs>,
    services: &'a Arc<services::Registry>,
    shims: &'a Arc<shims::Shims>,
    elevation: &'a Arc<elevation::Elevation>,
    shutdown: &'a CancellationToken,
}

/// What the daemon does while its state is open.
///
/// Separate from `main` so that `Store::close` has a single call site that every exit passes
/// through, including the failing ones.
///
/// **Answers the directories a finished uninstall left to remove**, and is empty on every other
/// run. They are returned rather than removed here, because the database is still open on the last
/// line of this function and `MIXENGINE_HOME` is what holds it: `main` removes them once
/// `Store::close` has checkpointed the write-ahead log and the home lock has been dropped.
pub(crate) async fn serve(
    paths: &Paths,
    store: &Store,
    started: api::Started,
    endpoint: &ipc::Endpoint,
    config: &config::Config,
    sources: &Sources,
    launch: Launch,
) -> anyhow::Result<Vec<PathBuf>> {
    let Launch {
        program,
        credentials,
        credential_facts,
    } = launch;

    // The two settings this function spends, read out of the file `main` loaded. One argument
    // rather than two, because a seventh would put this over the count clippy allows and because
    // the next task to want a key would have added an eighth.
    let shutdown_grace = Duration::from_secs(config.daemon.shutdown_grace_seconds);

    // **Whether a site with nothing behind it answers with a page of ours.**
    // Read here beside the grace period rather than passed in, for the reason above: the file is
    // already an argument, and every generator this daemon builds has to read one answer or a drift
    // check would report a difference against a rendering nothing wrote.
    let welcome = config.sites.welcome_page;

    // **Crash reports.** Built here rather than passed in, and an eighth argument is only half
    // the reason — it would put this function over the count clippy allows, which is the same wall
    // the note above describes. The other half is that there is nothing to pass: `Reports` is a pure
    // function of the two things this function already has, so the value `main` installed the panic
    // hook from and this one are the same value rather than two copies of one.
    let crashes = crash::Reports::new(paths, config.crash.enabled);

    // **`<root>/bin` is refreshed on every start.** It is a projection of a table compiled into
    // this binary, exactly as `etc/` is a projection of the database, so a home whose `bin/` was
    // emptied is repaired by starting the daemon. Touching nothing outside the root is what
    // separates it from putting that directory on the user's PATH — that is `path.install`'s, and
    // is only ever done when somebody asks.
    //
    // **Before the endpoint is bound**, unlike the two recovery passes below, and the reason is the
    // endpoint rather than the work: a client that dials after the bind waits in the backlog for
    // its answer, so every moment between the two is a moment it is kept waiting. Recovery is a
    // database read and a handful of process lookups; nineteen file copies are not. (Before the
    // named pipe kept a backlog, that wait was a refusal on Windows — `ERROR_PIPE_BUSY` — and
    // putting these copies after the bind made an ordinary parallel test run fail.) Nothing is
    // listening yet while this runs, and a client that finds nothing there retries — which is the
    // same thing it does for the migrations that ran a moment ago.
    //
    // Nothing here fails the start, on the rule the recovery passes follow: a `bin/` that could not
    // be written leaves a home whose shims are missing, which a person can see and act on, where
    // refusing to start would leave them with no daemon at all.
    let shims = Arc::new(shims::Shims::new(
        paths,
        program.clone(),
        mixengine_platform::host(),
        store.clone(),
        services::spec::catalogue(),
    ));

    // **Built here and never called here.** The entry it registers is outside the home, so nothing
    // touches it on the daemon's own initiative — `shims` above refreshes `bin/` at every start
    // because that is inside the root, and puts the directory on the PATH only when asked, which is
    // the same rule this whole capability is one line of.
    let autostart = Arc::new(autostart::Autostart::new(
        program.clone(),
        paths.root().to_path_buf(),
        mixengine_platform::host(),
        store.clone(),
    ));

    // The fourth reader of this path, and the last: the updater. Kept here because
    // `elevation::Candidates` takes the original by value further down, and because the
    // question the updater asks of it is a different one from the other three: not *what shall I
    // run*, but *is the directory this binary sits in one this account may write*.
    let daemon_exe = program.clone();

    projections::refresh_bin(&shims).await;

    projections::seed_gallery(store, paths).await;

    // Through the wire mapping, and for the same reason the startup steps above are: the failure a
    // person actually meets here is "something else is already listening for this home", and the
    // sentence that says what to do about it is written at the boundary and nowhere else.
    //
    // **Accepting from here on, not from `accept::accept_until_stopped`.** Everything
    // between this line and that loop — recovery, the trust store, the registry — used to be a
    // window in which a bound named pipe had one instance and nobody taking it, so a second client
    // met `ERROR_PIPE_BUSY` and gave up after its second of retries. The backlog accepts now and
    // queues, which is what a Unix socket's listen queue always did: an early client waits for its
    // answer rather than being told nobody is there. The steps below that spawn rather than
    // await still do, because a queued client is still a client waiting.
    let mut listener = ipc::Listener::bind(endpoint)
        .map_err(|error| error.to_wire())?
        .backlog();

    // Registered before the first client rather than inside the loop. `select!` builds its futures
    // afresh on every turn, so registering there would tear the handlers down and reinstall them
    // continuously and could lose a signal that arrived in between. Failing the start is the right
    // answer to a handler the OS will not install: a daemon that cannot be asked to stop is one
    // somebody has to kill, and a shutdown is the wrong moment to find that out.
    let mut signals = signal::Signals::listen().map_err(|error| error.to_wire())?;

    // The root token every other shutdown path is a branch of. It is cancelled by a signal and
    // awaited by the accept loop and by every `GET /events`; `daemon.shutdown` cancels the same
    // object, and every service the registry supervises hangs a child token off it.
    let shutdown = CancellationToken::new();

    // The work a start leaves running behind it, owned so that none of it outlives the store it
    // reads — see `background`. Stopped beside the clients, before `serve` returns.
    let background = background::Background::new();

    // **And from here a watch keeps `bin/` current.** A `npm install -g yarn` typed a minute later
    // changes a runtime's bindir and tells nobody; the kernel tells this, and nothing runs while
    // nothing changes. See `bin_watch`. Started once there is a set to own it and a token to stop
    // it; `bin/` was refreshed above, and the watch's first pass catches anything in between.
    bin_watch::start(
        &background,
        Arc::clone(&shims),
        store.clone(),
        shutdown.clone(),
    );

    // Made here rather than inside the API, because the API is no longer the only publisher: the
    // registry announces every state change it persists, from tasks that outlive any one request.
    let events = api::Events::new();

    // **The source is the generator.** Every `service.*` call renders the
    // `services` table into `etc/` and into the specs the registry supervises, which is also why
    // there is nothing to do here beyond handing it the two things it reads: a home with no services
    // in it declares nothing, and the registry, the graph and the walk all handle that without a
    // special case.
    // **Before the registry**, which takes it. A service that has never been started here may have
    // a first-run ritual to perform, and that is minutes of work reported through a job rather than
    // something a `service.start` can do inline.
    let jobs = Arc::new(jobs::Jobs::new(store, events.clone(), shutdown.clone()));

    // One host for both, rather than two: `declared` asks it what this system makes a front end
    // bind, and the registry keeps it for everything else.
    //
    // And it is the one host that reaches `keyring()` — directly, and through `elevation.host()`,
    // which the API hands to extensions and databases. Every other `host()` in this crate reaches
    // pools, activation, shims, autostart or machine facts, none of which read a credential.
    tracing::info!(
        credentials = %credentials::describe(&credentials, mixengine_platform::RELEASE),
        choosable = credential_facts.choosable,
        "credentials"
    );
    let host = mixengine_platform::host_with(credentials);

    let services = Arc::new(
        services::Registry::new(
            paths,
            store,
            Arc::clone(&host),
            events.clone(),
            services::declared(paths, store, host.as_ref(), welcome),
            shutdown.clone(),
            Arc::clone(&jobs),
        )
        .with_welcome(welcome),
    );

    // **The DNS server, and the mode it puts this home in.** Started here, after the host and
    // before the queue that reads its mode: `require_hosts` asks it which managed TLDs a resolver
    // routes here, and leaves the names under those out of the hosts file.
    //
    // Nothing here fails the start, on the rule every block around it follows — a port somebody else
    // is holding is a mode and a sentence on `mix status`, not a machine with no daemon.
    let dns = Arc::new(dns::Dns::start(&config.dns, host.as_ref(), shutdown.clone()).await);

    // **The mDNS responder.** Started beside the DNS server and on the same rule: a home where UDP
    // 5353 cannot be bound advertises nothing and shares by address, which is exactly what LAN
    // sharing did before this responder existed. It is deliberately not the DNS server: that one
    // answers the managed TLDs for this machine, and this one answers one name for the network.
    let mdns = Arc::new(mdns::Mdns::start(shutdown.clone()));

    // **Read once, reported, never refused.** Refusing to start is what ADR 0005's first sentence
    // seems to demand and is wrong here for a measured reason: CI's whole Windows third runs the
    // daemon suites under a full administrative token, and a hard refusal would turn one of three
    // platforms red for a reason that has nothing to do with the code under test. What is worth
    // saying about it is not that this daemon cannot elevate — it is that every service it
    // supervises inherits the token.
    let elevation = elevation::Elevation::new(
        paths,
        store,
        events.clone(),
        Arc::clone(&jobs),
        // The same host as the registry's, and not a second one. The API hands this one on to
        // extensions, databases, bundles and certificates, which is where most credentials are read
        // and written — a separate `host()` here kept them in the OS store.
        Arc::clone(&host),
        elevation::Candidates {
            program,
            // Where this operating system keeps an installed privileged helper. `ok()` and not `?`:
            // a machine that will not name one is a machine with no installed copy, which is the
            // ordinary state of a development tree and not a reason to refuse to start.
            installed: mixengine_platform::install::helper_path().ok(),
        },
        Arc::clone(&dns),
    );

    if mixengine_platform::elevated::is_elevated() {
        tracing::warn!(
            "this daemon holds an administrative token; every service it supervises inherits it, \
             and writes files into this home as an administrator. `mix status` says so too."
        );
    }

    let shared = Shared {
        paths,
        store,
        config,
        host: &host,
        events: &events,
        jobs: &jobs,
        services: &services,
        shims: &shims,
        elevation: &elevation,
        shutdown: &shutdown,
    };

    recovery::recover(&services).await;

    machine::ask_for_grants(&elevation, &services).await;

    machine::certificates(&shared, &background).await;

    let metrics = clocks::start_clocks(&shared, &background);

    let adopted = repairs::repair_home(&shared, &background).await;

    let clients::Clients {
        registry,
        runtimes,
        packages,
        updates,
    } = clients::build_clients(&shared, sources, &daemon_exe, &adopted, &background).await?;

    // Built after the listener rather than before it, so `daemon.status` reports the endpoint that
    // was actually bound instead of the one that would be computed again now.
    let api = api::Api::new(
        paths,
        store,
        endpoint,
        started,
        events,
        api::Supervision {
            services: Arc::clone(&services),
            jobs: Arc::clone(&jobs),
            runtimes,
            packages,
            registry,
            shims,
            autostart,
            updates: Arc::clone(&updates),
            elevation: Arc::clone(&elevation),
            dns,
            mdns,
            metrics,
            memory_over_minutes: config.services.memory_over_minutes,
            crashes,
            credentials: credential_facts,
        },
        api::Shutdown::new(shutdown.clone(), shutdown_grace),
    );

    started::after_the_api(&api, &elevation, config, &shutdown, &background).await;

    started::after_an_update(&updates, &services, &elevation, paths, &background);

    // **No check at start and no clock** — ADR 0056 rule 8. The feed is read only when somebody
    // asks: `mix self-update`, `mix self-update --check`, or `update.check` from a client. A
    // headless install may be a production server, and nothing there changes unless a person said
    // so.

    tracing::info!(endpoint = %endpoint, "listening for clients");

    // **And what asked to start, starts.** The fifth member of the family of background loops
    // `clocks` and `started` start and the only one that runs *after* this line rather than before
    // it: those four are sweeps whose first pass must not race the first client, and this one
    // starts real programs. A daemon that will not answer `daemon.status` until MariaDB's first run
    // has finished looks hung at exactly the moment somebody is looking at it, and what fills a
    // dashboard in as it goes is the event stream this walk announces on — which needs a client
    // able to connect to it.
    //
    // After recovery for `recover`'s own reason, which is satisfied by everything above: a service
    // recovery adopted is already up, and a walk counts one that is up as reached rather than
    // restarting it.
    crate::services::autostart::start(
        &background,
        Arc::clone(&services),
        store.clone(),
        shutdown.clone(),
    );

    let (connections, on_the_os_clock) = accept::accept_until_stopped(
        &mut listener,
        &mut signals,
        &api,
        &services,
        &shutdown,
        shutdown_grace,
    )
    .await;

    accept::stop_everything(
        &mut signals,
        &services,
        &jobs,
        shutdown_grace,
        connections,
        on_the_os_clock,
        &background,
    )
    .await;

    // Read after the clients have gone, so an uninstall that was still writing its answer has
    // finished writing it.
    Ok(api.armed())
}

/// What a shutdown the *operating system* asked for may spend on services.
///
/// **One budget, two ceilings.** `daemon.shutdown` arrives over a socket with nothing counting
/// against it and gets the configured number entire; a console control event on Windows arrives with
/// about five seconds already ticking, and a daemon that spent the configured ten would be
/// terminated somewhere in the middle of a database it had asked to flush — the worst of both, since
/// the polite stop was begun and not finished.
///
/// So where the OS states a ceiling this takes the smaller of the two, minus what the rest of the
/// shutdown still has to do afterwards ([`CEILING_RESERVE`]). Where it states none — every Unix, and
/// the `--detach`ed Windows daemon that has no console for an event to arrive on — the configured
/// budget is the whole answer, because nothing else is going to end this process early.
///
/// Saturating rather than clamped to a minimum: a machine whose ceiling is smaller than the reserve
/// is one where services get nothing and are killed at once, which is the honest outcome and is what
/// the row and the log then say. Inventing a floor there would spend time the OS has already decided
/// this process does not have.
fn signalled_budget(configured: Duration) -> Duration {
    match signal::STOP_CEILING {
        Some(ceiling) => configured.min(ceiling.saturating_sub(CEILING_RESERVE)),
        None => configured,
    }
}

/// Let the connections that are still open finish, then stop waiting.
///
/// A grace period rather than an abort, because a client mid-request has already been told the
/// daemon accepted it — and rather than an unbounded wait, because a connection is kept alive
/// between requests: a client that has ended its `GET /events` (the root token does that as this is
/// called) may still be holding a socket nobody is going to write to again. Dropping the set at the
/// end aborts whatever is left, which for a connection with no request in flight is exactly right.
///
/// `grace` is [`CLIENT_GRACE`] or the shorter [`SIGNAL_CLIENT_GRACE`], and which of the two it is,
/// is the whole of what the two ways of shutting down differ by from here on: one of them has an
/// answer to write into a connection and the other arrived with an OS clock already running. Passed
/// rather than read from a constant inside, because the caller is the only thing that knows which
/// shutdown this is.
async fn shut_down(mut connections: tokio::task::JoinSet<()>, grace: Duration) {
    if connections.is_empty() {
        return;
    }

    tracing::info!(
        open = connections.len(),
        ?grace,
        "waiting for clients to finish"
    );

    if tokio::time::timeout(grace, async {
        while connections.join_next().await.is_some() {}
    })
    .await
    .is_err()
    {
        tracing::info!(
            open = connections.len(),
            "closing connections that were still open"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The "two ceilings" half of the shutdown budget, asserted as the one rule rather than as
    /// three per-OS numbers.
    ///
    /// Written this way on purpose: a test that said "2.5 seconds on Windows" would be a second copy
    /// of `STOP_CEILING` in the daemon, which is exactly the `#[cfg]` the platform layer exists to
    /// keep out of here. What is checked is the relationship — a ceiling shortens the budget and
    /// leaves room for what comes after the services, and no ceiling leaves it alone.
    #[test]
    fn a_shutdown_the_operating_system_asked_for_fits_inside_whatever_clock_it_started() {
        let configured = Duration::from_secs(10);
        let budget = signalled_budget(configured);

        match signal::STOP_CEILING {
            Some(ceiling) => {
                assert!(
                    budget + CEILING_RESERVE <= ceiling,
                    "the connections and the WAL checkpoint happen after the services stop, and \
                     this budget leaves no room for them: {budget:?} of {ceiling:?}"
                );
                assert!(budget < configured, "a ceiling shortens the budget");
            }

            // Nothing is counting, so the configured budget is the whole answer and shortening it
            // would kill a database that had every right to finish flushing.
            None => assert_eq!(budget, configured),
        }
    }

    /// The margin the reserve is now made of, asserted as the relation and not as its parts.
    ///
    /// The defect this pins is that the old reserve left none: 2.5 s of services, 2 s of clients and
    /// what was left for the checkpoint added up to the ceiling exactly, so any of the three running
    /// a moment over meant a process terminated mid-checkpoint — the one thing the reserve exists to
    /// prevent. **Strictly less, not at most**, because "adds up to exactly the ceiling" is precisely
    /// the arrangement that was wrong, and a reserve that is only ever spent in full is a reserve in
    /// name.
    #[test]
    fn a_shutdown_the_operating_system_asked_for_leaves_slack_over_after_the_checkpoint() {
        let budget = signalled_budget(Duration::from_secs(10));

        // Only where a clock is running. Where none is — every Unix — there is no ceiling for
        // anything to fit inside, and what this test still has to say is the clause below it, which
        // holds on all three systems.
        if let Some(ceiling) = signal::STOP_CEILING {
            assert!(
                budget + SIGNAL_CLIENT_GRACE + CHECKPOINT_MARGIN < ceiling,
                "the clients and the WAL checkpoint follow the services, and what this budget \
                 leaves for them is the whole of the rest of the ceiling: {budget:?} of {ceiling:?}"
            );
        }

        assert!(
            SIGNAL_CLIENT_GRACE + CHECKPOINT_MARGIN < CEILING_RESERVE,
            "the reserve is supposed to keep back more than the two waits it is spent on; \
             {SCHEDULING_SLACK:?} of it is meant to be left over"
        );
    }

    /// The escalation half of the shutdown budget: a second request to stop is paid for out of the
    /// slack.
    ///
    /// Asserted against [`SCHEDULING_SLACK`] rather than against a ceiling, so that it is a claim
    /// every OS can check — the arithmetic it stands for is that a budget spent to its last
    /// millisecond, plus this wait, plus the clients and the checkpoint, still ends inside whatever
    /// clock the OS started. The version with the ceiling in it follows for the one system that has
    /// one.
    #[test]
    fn a_second_request_to_stop_costs_less_than_the_reserve_keeps_in_hand() {
        assert!(
            KILL_GRACE + CONFIRMATION_REPRIEVE < SCHEDULING_SLACK,
            "an escalated shutdown waits {KILL_GRACE:?}, and a stop watching a killed survivor \
             {CONFIRMATION_REPRIEVE:?}, on top of a budget that may already be spent, and only \
             {SCHEDULING_SLACK:?} of the reserve is not already promised to something"
        );

        if let Some(ceiling) = signal::STOP_CEILING {
            let budget = signalled_budget(Duration::from_secs(10));

            assert!(
                budget
                    + KILL_GRACE
                    + CONFIRMATION_REPRIEVE
                    + SIGNAL_CLIENT_GRACE
                    + CHECKPOINT_MARGIN
                    < ceiling,
                "a second Ctrl-C at the last instant of the budget must still leave the checkpoint \
                 inside {ceiling:?}"
            );
        }
    }

    #[test]
    fn a_configured_budget_smaller_than_the_ceiling_is_still_the_one_that_applies() {
        // The ceiling is what the OS *allows*, not what a shutdown is entitled to take. Somebody who
        // set a second in `config.toml` gets a second on every system, which is the whole reason
        // this is a `min` of the two rather than "the ceiling wherever there is one".
        assert_eq!(
            signalled_budget(Duration::from_millis(1)),
            Duration::from_millis(1)
        );
    }
}
