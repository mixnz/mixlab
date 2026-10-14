//! One task per service: spawn it, wait for it to be ready, watch it, restart it, stop it.
//!
//! **This is the loop `mixengine-supervisor` deliberately does not contain.** That crate delivers
//! the mechanisms — capture, ready, health, restart — as pieces with no loop, no clock and no state
//! row, because the thing that owns the timing is also the thing that owns the registry of running
//! services, the [`CancellationToken`] they hang off and the
//! [`transition`](mixengine_core::services::transition) that persists each move. That is the daemon,
//! and this module is where the four are tied together.
//!
//! Every state change goes through `mixengine_core::services::transition` and is published from the
//! value it hands back, so the row and the event cannot describe different events — the registry
//! never writes `services.state` behind `core`'s back.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use mixengine_core::{Store, services};
use mixengine_platform::Host;
use mixengine_platform::process::Adopted;
use mixengine_proto::{RestartPolicy, ServiceId, ServiceSpec, ServiceState, StateReason};
use mixengine_supervisor::logs::Capture;
use mixengine_supervisor::{Restarts, Surroundings};
use tokio::sync::{Notify, watch};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use super::logs::ServiceLog;
use crate::api::Events;

mod attempt;
mod environment;
mod reload;
mod stop;

/// How often a running service is asked whether it is still there.
///
/// Coarser than the 50 ms `ready::wait` polls at, and for a different question: that one is racing a
/// probe on a service somebody is waiting for, this one runs for days on a service nobody is
/// watching. A quarter of a second is well inside what a person perceives as immediate and costs a
/// handful of syscalls a second across every service on the machine.
const WATCH: Duration = Duration::from_millis(250);

/// How often a stopping service is asked whether it has gone yet.
///
/// Paid only during a stop, and every one of these is latency a user is sitting through, so it is
/// the fine-grained one.
const POLL: Duration = Duration::from_millis(50);

/// How long a killed survivor is waited for before the daemon gives up on watching it go.
///
/// Only ever reached by an **adopted** service (roadmap task T18), because that is the one this
/// process cannot wait on: it is not the survivor's parent, so there is no status to reap and the
/// only question available is "is it still there", asked at [`POLL`]. A `Supervised` child is waited
/// for by the kernel and needs no ceiling at all.
///
/// Generous, because what it is measuring is a `SIGKILL` being delivered and the process leaving the
/// table — milliseconds unless the machine is in trouble. What happens when it runs out is in
/// [`Runner::stop_adopted`], and it is deliberately not "record it as stopped anyway".
///
/// **Generous is affordable only because a shutdown shortens it** — see [`Runner::within_budget`].
/// This is waited after the grace period rather than inside it, so an adopted survivor that took
/// its whole budget being asked politely and then five seconds more being watched would put the
/// walk five seconds past a total somebody was told. The constant is what a stop is allowed when
/// nothing is counting; the budget is what it gets when something is.
const GONE: Duration = Duration::from_secs(5);

/// The least of [`GONE`] a killed survivor is watched for, whatever a shutdown has left — roadmap
/// task **T9a**.
///
/// **Because zero is a wrong answer here and a right one everywhere else the budget reaches.** A
/// grace period of zero is a service killed at once and a log drain of zero is a tail nobody was
/// reading — both stated, both survivable. This poll is not a wait but a question, and one asked
/// with no window at all is asked microseconds after the kill: the kernel has not finished with the
/// process, [`Adopted::exited`] says `Ok(None)`, and a process that stopped exactly as it was told
/// to is reported as a survivor that will not go. What follows from that report is the whole of the
/// defect — the row keeps its `stopping`, [`Registry::stop_one`](super::Registry) reads it and says
/// the stop failed, and the walk stops there on the ordering rule, leaving every service after it in
/// the plan running.
///
/// Two polls of [`POLL`], which is what a delivered kill needs in the ordinary case and no more than
/// that: the floor exists to make the question answerable, not to wait out a process that is
/// genuinely stuck. One that is still there after it is reported as still there, exactly as before.
///
/// Drawn from [`Budget::reprieve`](super::Budget) rather than granted outright, so that a walk with
/// eight survivors in it costs the OS ceiling what a walk with one costs — see
/// [`Runner::seeing_it_go`].
const GONE_FLOOR: Duration = Duration::from_millis(100);

/// How long a service's environment is waited for before whatever needed it goes on without it.
///
/// A deadline rather than a patience, and the reason is the store it reads: a `Keyring` value goes
/// through the OS credential store, which on Linux is a D-Bus round trip to a daemon that may be
/// *prompting the user*. A locked keyring answers when somebody types a password, or never. Without
/// a ceiling here a `mix stop` of an adopted MariaDB, or a whole daemon shutting down, waits for
/// that forever.
///
/// **Both paths that resolve one are bounded by this, and they differ only in what they do
/// afterwards.** A stop goes on with the entries the spec states outright
/// ([`Runner::where_commands_run`]), because a `mariadb-admin shutdown` short one entry still beats
/// a kill; a start refuses ([`Runner::environment`]), because carrying on would be the empty
/// password that function exists to prevent.
///
/// The start path is the one that could hang a whole daemon, and did until it was bounded. The
/// read happens on a blocking task and holds no cancellation point, so a service sitting in
/// [`Runner::attempt`] when a shutdown arrives never reaches its token: the cancellation releases
/// nothing, [`Registry::stop_one`](super::Registry) waits on a task that is not coming back, and
/// the answer to `daemon.shutdown` — with the root token and the daemon behind it — waits with it.
/// A `--detach`ed daemon on Windows has no console event to fall back on either. The start path
/// now waits on that token beside the read, and is bounded by [`START_ENVIRONMENT`] instead.
///
/// Generous against an unlocked store, which answers in milliseconds, and short against a person who
/// is not at the machine.
///
/// **And shortened again on the stop path, because three seconds is not always three seconds this
/// daemon has** — see [`Runner::within_budget`]. Adopted services resolve this *while stopping*, one
/// read each, after which the walk still has to ask them to stop; a Windows shutdown is working
/// inside two and a half seconds altogether. So the constant is what the read is allowed when
/// nothing is counting, and the budget is what it gets when something is — the same arrangement
/// [`GONE`] and [`FLUSH`] are in, and for the same reason.
const ENVIRONMENT: Duration = Duration::from_secs(3);

/// How long a start waits for its environment before it fails.
///
/// **Long enough for a person to answer the OS.** On macOS the first read of a Keychain item by a
/// build the item does not trust yet raises a dialog, and the read returns only once somebody has
/// clicked it; a locked keyring on Linux asks for a password the same way. Bounded by
/// [`ENVIRONMENT`], the start failed while the dialog was still on screen, and the person who then
/// allowed it found a `Failed` service and had to ask again.
///
/// A start can afford what a stop cannot, because a stop or a shutdown is not kept waiting by it:
/// [`Runner::environment`] waits on the runner's cancellation as well, and leaves the moment it
/// fires. What is left to bound is a person who is not at the machine, and the read is joined
/// by the next attempt rather than begun again either way — see [`Runner::reading`].
///
/// Three seconds under test, where the keyring is a mock with a set delay and a test that waited
/// two minutes for a deadline would be a test nobody runs.
const START_ENVIRONMENT: Duration = if cfg!(test) {
    ENVIRONMENT
} else {
    Duration::from_secs(120)
};

/// How long the last lines of a stopped service are waited for.
///
/// Bounded because end of file is not the process exiting but the *last holder of the pipe*
/// exiting — see [`Capture::finish`], which explains why an unbounded wait here would hang the
/// supervisor at the one moment it has something to report.
///
/// **Really paid, and paid per service after that service's grace period is spent** — which is what
/// makes it a shutdown's business and not only a stop's. Killing the group is not killing a
/// grandchild that left it, and a leftover holding a copy of the service's stdout keeps the pipe
/// open for the whole of this; eight services doing that is sixteen seconds after the last grace
/// period. So a shutdown shortens it too — see [`Runner::within_budget`].
const FLUSH: Duration = Duration::from_secs(2);

/// What a walk of the spec's environment came back with: every entry that resolved, and the error of
/// each that did not.
///
/// Both halves, because the two callers of [`Runner::walk_environment`] disagree about what a
/// failure means — a start refuses anything less than all of it, a stop command runs with what there
/// is.
///
/// **`anyhow::Error` here on purpose, unlike [`SpecSource`](super::SpecSource)** — R8 changed one of
/// them and deliberately not the other. Nothing downcasts this or maps it to a code: an entry that
/// will not resolve is logged and the service is given `StateReason::SpawnFailed`, so what this type
/// has to carry is a sentence for `daemon.log`. Three unrelated things produce one — a
/// `mixengine_platform` keyring failure, a credential the store simply does not hold, and the
/// blocking task not finishing — and the `"the environment entry …: <cause>"` that names which
/// variable it was, and why, is the part a reader needs. A typed enum here would be three variants that exist
/// to be `Display`ed and one place where a `From` has to be written.
type Resolved = (BTreeMap<String, String>, Vec<(String, anyhow::Error)>);

/// What a walk of the spec's environment does with an entry that will not resolve.
///
/// The difference is not bookkeeping: resolving a `Keyring` entry can put a prompt on the user's
/// screen, so how far the walk goes decides how many of them a single start or stop puts there.
#[derive(Clone, Copy, Debug)]
enum OnFailure {
    /// Stop there, with that entry's error.
    ///
    /// What a start uses. It refuses anything less than the whole environment, so every entry after
    /// the first failure is one whose value will not be used — and on a locked keyring, walking on
    /// asks the user to unlock it once per credential for a start that has already failed.
    Stop,

    /// Write it down and keep walking.
    ///
    /// What a stop command uses: it runs with whatever entries there are, so the ones after a
    /// failure are still worth having.
    Record,
}

/// Whether the service this runner supervises is somewhere traffic can go.
///
/// **The registry reads this and never the task's liveness**, which is the distinction that makes a
/// tiered walk mean anything: a runner is alive through a restart backoff, through a stop and
/// through a start that has not finished, as well as through a healthy hour. "Something is
/// supervising it" is not "it is up", and a walk that took the one for the other would start a site
/// against a database that is in its fourth crash.
///
/// Derived from the transition [`Runner::move_to`] has just persisted rather than described a second
/// time beside it, on the same reasoning as the event: two descriptions of one move drift.
#[derive(Debug, Clone)]
pub(super) enum Readiness {
    /// A start is in flight and has not been decided. The value a runner begins with, and the one
    /// it returns to whenever a backoff releases it.
    Deciding,

    /// The ready check passed and the process is there.
    Up,

    /// It is not usable, and this is the [`super::Fault`] persisted about why.
    ///
    /// A fault with no reason is never produced here: it is what the registry reports for a runner
    /// that ended without deciding at all — see [`super::start::settled`].
    Down(super::Fault),
}

impl Readiness {
    /// What a service that has just reached `state` under `restart` is, to a walk that wants to know
    /// whether it may go on.
    ///
    /// Exhaustive over [`ServiceState`], which is closed for readers exactly like this one: a state
    /// added without a decision here would be a service the walk silently misjudges.
    fn of(
        state: ServiceState,
        reason: StateReason,
        restart: RestartPolicy,
        detail: Option<&str>,
    ) -> Self {
        let down = |reason: StateReason| {
            Self::Down(super::Fault {
                reason: Some(reason),
                detail: detail.map(str::to_owned),
            })
        };

        match state {
            // `Degraded` is up on purpose: a service answering badly is the amber case the GUI
            // shows and `mix doctor` explains, not an absent one, and a dependent that refused to
            // start against it would turn one slow database into a machine with nothing running.
            ServiceState::Running | ServiceState::Degraded => Self::Up,

            // A start in flight, whether it is the first or the one a backoff has just released. A
            // second walk that arrives here waits for the same answer rather than inventing one.
            ServiceState::Starting => Self::Deciding,

            // **`Restarting` is the one the policy decides**, and it is what keeps a walk finite.
            //
            // A bounded policy arrives somewhere by itself: `Running` when an attempt takes,
            // `Failed` when the ceiling is reached. A walk under one waits the backoff out and is
            // answered by whichever the policy reached — which is what the default `OnFailure` needs,
            // because its first crash is a transient the runner recovers from half a second later,
            // and a walk that read that one crash as `Down` would leave the tier below `Failed` and
            // unsupervised beside a service that came up fine.
            //
            // `RestartPolicy::Always` is the one with no ceiling. Nothing under it ever reaches
            // `Failed`, so a walk that waited for it to *give up* would wait for ever, and the first
            // attempt is the only answer there is.
            ServiceState::Restarting => match restart {
                RestartPolicy::Always { .. } => down(reason),
                _ => Self::Deciding,
            },

            ServiceState::Stopping | ServiceState::Stopped | ServiceState::Failed => down(reason),
        }
    }
}

/// What the memory watchdog last concluded about one service — roadmap task **T71a**.
///
/// Carried as [`None`] where the service is under its ceiling *or* was not measured at all: the two
/// are the same fact to a reader of state, and the difference between them is spent inside
/// `services::watchdog` where it decides a count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Over {
    /// What it was measured holding, in bytes: the finished minute's average.
    pub(crate) rss_bytes: u64,

    /// The ceiling it was judged against, in megabytes.
    pub(crate) limit_mb: u32,
}

/// One state from two independent readings — roadmap task **T71a**.
///
/// **The runner is the only writer of the `Running`/`Degraded` edge, and this is why.** Health and
/// size are measured by different things at different rates: a health verdict comes from a probe
/// this loop makes, and a memory verdict from a watchdog reading minutes the sampler finished. Two
/// writers would overwrite each other in both directions — the next healthy probe clearing a warning
/// about a service still over its ceiling, and a watchdog seeing memory drop erasing a genuine
/// `Unhealthy`.
///
/// **Illness is reported ahead of size when both hold.** A service failing its probe needs attention
/// whatever it weighs, and telling somebody that their database is *large* while it is refusing
/// connections would send them to the wrong problem. It is still restarted for its size at the third
/// minute, with `OverMemory` on that transition — the one place this design says two things about
/// one episode, and it is deliberate: a leak is most likely to be the cause exactly when a service
/// has also stopped answering.
fn fold(healthy: bool, over: Option<Over>) -> (ServiceState, StateReason) {
    match (healthy, over) {
        (false, _) => (ServiceState::Degraded, StateReason::Unhealthy),

        (true, Some(over)) => (
            ServiceState::Degraded,
            StateReason::OverMemory {
                rss_bytes: over.rss_bytes,
                limit_mb: over.limit_mb,
            },
        ),

        (true, None) => (ServiceState::Running, StateReason::Healthy),
    }
}

/// Whether a fold is a change worth writing down.
///
/// **The state, and deliberately not the reason.** [`ServiceState::can_become`] has no self-loops, so
/// a move to the state a service is already in is an `IllegalTransition` — which `record` logs an
/// `error!` for, once per minute, for as long as the service stays over its ceiling.
///
/// The cost is a reason that can lag: a service that recovers its health while still over its
/// ceiling goes on reading `degraded — unhealthy`, although what is now wrong with it is its size.
/// The alternative is publishing a `Running` it never was, to every client watching, in order to
/// correct one word.
fn worth_recording(current: ServiceState, next: ServiceState) -> bool {
    current != next
}

/// Everything one supervised service needs, and nothing about any other.
#[derive(Debug)]
pub(super) struct Runner {
    /// What to run and how to judge it.
    pub(super) spec: ServiceSpec,

    /// Where the state row lives. Every move is written here before it is published.
    pub(super) store: Store,

    /// `logs/services/<service-id>/`, which `Capture` writes `current.log` into.
    pub(super) directory: PathBuf,

    /// Where this service's output is put for clients to read — roadmap task **T16b**.
    ///
    /// **Outlives every [`Capture`] this runner makes**, which is the whole reason it is a field
    /// here rather than something built beside each one: a capture belongs to one run of the
    /// process and dies with it, and a `mix service logs -f` open across a crash, a backoff and a
    /// restart must not end three times. Each attempt attaches its capture to this — see
    /// [`Runner::relay`] — and what a connected client holds is a subscription to *this*.
    pub(super) log: Arc<ServiceLog>,

    /// The OS, for the one thing a spawn needs from it that the spec cannot carry: a credential.
    pub(super) host: Arc<dyn Host>,

    /// The account a refusal in this service's own log would have to name — roadmap task **T127a**.
    ///
    /// [`None`] for everything that is not a database, which is the gate and not a nicety: a
    /// php-fpm pool captures its workers' output, and an application that cannot reach its database
    /// prints `Access denied for user …` into exactly that stream. A pool whose start failed for
    /// some other reason would then be reported as a pool whose superuser password is wrong — and
    /// the repair that reason names answers `Unsupported` for it, because a pool keeps no credential
    /// of its own.
    ///
    /// Read once, when this runner is built, out of the map [`Registry::graph`](super::Registry)
    /// fills: the same answer `database.create` gets, and a service's recipe does not change under a
    /// supervisor that is already running it.
    pub(super) superuser: Option<&'static str>,

    /// Where a persisted transition is announced.
    pub(super) events: Events,

    /// Cancelled to stop this service. A child of the daemon's root token, so a daemon on its way
    /// out stops its services rather than dropping them.
    pub(super) cancel: CancellationToken,

    /// Notified when somebody asks for this service to start *now*, rather than when its restart
    /// policy next comes round. Roadmap task **T19c**, and the only edge that runs from the registry
    /// into a runner: everything else it does to a live runner is a read.
    ///
    /// A [`Notify`] because the question carries nothing but the asking, and because a request that
    /// arrives while nothing is waiting is kept as one permit — so an explicit start is honoured by
    /// the backoff this runner is in, or by the next one it enters, and two of them arriving together
    /// are one restart rather than two.
    pub(super) asked_to_start: Arc<Notify>,

    /// Notified when this service's generated configuration has been rewritten under it — roadmap
    /// task **T31**, and the second edge from the registry into a runner.
    ///
    /// A [`Notify`] for [`Runner::asked_to_start`]'s reasons and one of its own: a walk that renders
    /// while this runner is inside a health probe leaves a permit rather than a message, so the
    /// reload happens at the top of the next turn — and two walks that both find a change before it
    /// gets there collapse into the one reload that was needed, because what a reload delivers is
    /// the file as it is now and not the edit that produced it.
    pub(super) asked_to_reload: Arc<Notify>,

    /// A new set of ceilings, pushed by [`Registry::set_limits`](super::Registry::set_limits).
    ///
    /// Carries the value rather than only waking this task, unlike its two neighbours — see the
    /// sender's own documentation for why a limit is the message and not a pointer to one.
    pub(super) limits_asked: watch::Receiver<mixengine_proto::ResourceLimits>,

    /// Why the stop this runner is about to perform is happening — roadmap task **T69**.
    ///
    /// [`None`], which is nearly always, means somebody asked: `service.stop`, a walk, a shutdown.
    /// The idle sweeper sets it before it cancels, so a service that stopped because nothing was
    /// using it says so on the transition rather than claiming a person asked for it.
    ///
    /// A `watch` beside [`Runner::limits_asked`] and for its reason — the value *is* the message —
    /// running the same way. Read at the moment the stop begins rather than subscribed to, because
    /// there is exactly one moment it matters.
    pub(super) stopping_because: watch::Receiver<Option<StateReason>>,

    /// What the memory watchdog last concluded about this service — roadmap task **T71a**.
    ///
    /// Read by `supervise`, folded with its own health verdict, and never acted on alone. See
    /// [`fold`].
    pub(super) over_memory: watch::Receiver<Option<Over>>,

    /// What is left of the whole daemon's shutdown, when one is under way — roadmap task **T9a**.
    ///
    /// Read at every wait a stop is about to make, and it can only shorten them: the seconds a
    /// service needs to flush are a fact about the service, and the total a user waits for a daemon
    /// they told to stop is a different fact that nothing else owns. See [`Budget`](super::Budget).
    ///
    /// **Every wait and not only the grace period**, which is what makes it a bound on the total.
    /// The grace period is where the spec has something to say, so it goes through
    /// [`Runner::grace_for`]; the kill's log drain, the watch for an adopted survivor and the
    /// environment an adopted service's stop command has to be run in are constants this module
    /// chose, and they go through [`Runner::within_budget`]. A stop is the sum of the four, and a
    /// budget that clamped only the first was a promise the walk did not keep — as was one that
    /// clamped the two that come after the grace period and not the one that comes before it.
    pub(super) budget: super::Budget,

    /// Where this service's own commands are run — a health probe, a shutdown command.
    ///
    /// Resolved once, at the spawn that begins each life of the process, and kept: the environment
    /// it holds is the one the *service* was given, credentials and all, and re-deriving it would
    /// mean an OS keyring read on every health probe of every service for as long as the machine is
    /// up. See [`Surroundings`] for why a probe run anywhere else would be asking about a different
    /// server.
    ///
    /// [`None`] until this runner has spawned something, which is the adopted case (roadmap task
    /// T18): nothing in *this* process ever built that environment, so a stop command there resolves
    /// one when it is needed — see [`Runner::where_commands_run`].
    pub(super) surroundings: Option<Surroundings>,

    /// The environment read a previous start stopped waiting for, while it is still going.
    ///
    /// **Kept so that the next start joins it instead of beginning a second one.** Giving up on the
    /// read is [`ENVIRONMENT`]'s whole purpose and it cannot abort what it gave up on: the walk runs
    /// on a blocking task, `spawn_blocking` has no cancellation, and dropping the handle only stops
    /// *this* task waiting — the thread stays parked in the keyring until the OS answers. One of
    /// those is the price the bound was worth paying; one per attempt is not, and a start is
    /// attempted again by every restart the policy grants and by every `service.start` a client
    /// sends. Against a keyring that never answers, that filled tokio's blocking pool — at which
    /// point [`Runner::kill`], which is also a blocking task, could no longer run, and a daemon that
    /// had bounded a hanging read to keep stopping services could not stop any.
    ///
    /// So the leak is bounded at one per runner, which is the same bound the stop path already has,
    /// and a keyring that unlocks half an hour later is answered by the attempt that finds this
    /// finished rather than by one that starts the read over.
    ///
    /// [`None`] whenever no read is outstanding, which is every start that got its answer.
    pub(super) reading: Option<tokio::task::JoinHandle<Resolved>>,

    /// Where this runner says whether its service is usable.
    ///
    /// A [`watch`] rather than a one-shot, because the question is asked more than once and by more
    /// than the walk that started the service: the answer has to be *current* for whoever asks next,
    /// where a one-shot would leave the walk after it reading the outcome of a start that ended an
    /// hour ago. Dropped with this runner, which is how the registry learns that a task ended
    /// without deciding.
    pub(super) readiness: watch::Sender<Readiness>,
}

/// What the runner does once a life of the process is over.
#[derive(Debug)]
enum After {
    /// Nothing more. The service is `Stopped` or `Failed` and this task ends.
    Done,

    /// Start it again, once this has elapsed.
    Again {
        /// The backoff the restart policy chose.
        after: Duration,
        /// Which attempt the next start is, counted from 1 since the service was last healthy.
        attempt: u32,
    },
}

/// What let a runner out of a backoff.
#[derive(Debug)]
enum Released {
    /// The wait the restart policy asked for is over.
    Elapsed,

    /// Somebody asked for this service to start now — see [`Runner::asked_to_start`].
    Asked,

    /// The service was asked to stop while it was waiting. It is `Stopped` and the task is over.
    Stopped,
}

impl Runner {
    /// Supervise this service until it is stopped, gives up, or the daemon does.
    ///
    /// **What a walk waits on is [`Runner::readiness`] and not this task**, which is what makes a
    /// tiered walk both possible and finite: the tier below may begin the moment this service is
    /// ready, a service that does not come up stops the walk rather than leaving it waiting for a
    /// process that is not coming, and a service being put back by a policy with no ceiling is
    /// answered after the first attempt rather than after a `Failed` that is never coming either.
    /// `because` is why this first life began: [`StateReason::Requested`] for every start a person
    /// or a client asked for, and [`StateReason::Autostart`] for the walk the daemon performs at
    /// boot — roadmap task **T113**. Every life *after* the first one has a reason of its own,
    /// decided by the restart policy, so this is the one move the caller gets to name.
    pub(super) async fn run(mut self, because: StateReason) {
        let mut restarts = Restarts::under(self.spec.restart());

        self.live(&mut restarts, because).await;
    }

    /// Supervise a process that was **already running** when this daemon started — roadmap task
    /// **T18**.
    ///
    /// The other way into this runner, and the difference is only how the first life of the process
    /// began: a daemon that was killed left this one behind, and its row, its pid and the moment it
    /// began are what the registry identified it by. From the moment that process ends, everything
    /// is ordinary again — the restart policy decides, and a service it puts back is spawned by
    /// [`Runner::attempt`] as a child of this daemon, with its pipes, its group and its log capture
    /// restored.
    ///
    /// **No transition is written on the way in**, and that is the point of adopting rather than
    /// restarting: nothing happened to the service. Its row said `running` before this daemon
    /// existed and says `running` still, so a state change here would be an event announcing that a
    /// service somebody has been using all along has just started. What *is* published is the
    /// readiness, because that lives in this process and this process has only just learned it.
    ///
    /// What the adopted life is missing is stated where a user pays for it: its output is not
    /// captured — the pipes belong to a daemon that is gone — so `current.log` has a hole in it from
    /// the moment that daemon died until the service is next started properly, and a crash loop
    /// decided during this life carries no tail to explain itself with. `mix doctor` owes the
    /// sentence (T47).
    pub(super) async fn adopt(mut self, adopted: Adopted) {
        tracing::info!(
            service = self.spec.id().as_str(),
            pid = adopted.pid(),
            "adopted a process that outlived the daemon supervising it"
        );

        // The row already says where this service is; what nothing in *this* process knows yet is
        // that it is usable, which is what a walk waiting on it is waiting for.
        self.readiness.send_replace(Readiness::Up);

        let mut restarts = Restarts::under(self.spec.restart());

        if let After::Again { after, attempt } = self.watch_adopted(adopted, &mut restarts).await
            && let Some(reason) = self
                .wait_before_starting_again(after, attempt, &mut restarts)
                .await
        {
            self.live(&mut restarts, reason).await;
        }
    }

    /// One life of the process after another, for as long as the policy keeps putting it back.
    ///
    /// `reason` is what the *first* start of this loop is for: somebody asking, a backoff that
    /// elapsed, or — for a service this runner adopted — the crash of the process it took over.
    async fn live(&mut self, restarts: &mut Restarts, mut reason: StateReason) {
        loop {
            // A `Starting` that will not persist ends this task with the readiness still undecided,
            // deliberately: the failure is the daemon's own, it is in `daemon.log`, and it is not a
            // state a client could render — which is what the registry reports for it.
            if !self.move_to(ServiceState::Starting, reason).await {
                return;
            }

            match self.attempt(restarts).await {
                After::Done => return,

                After::Again { after, attempt } => {
                    match self
                        .wait_before_starting_again(after, attempt, restarts)
                        .await
                    {
                        None => return,
                        Some(next) => reason = next,
                    }
                }
            }
        }
    }

    /// Wait out a backoff and say what the start after it is for. [`None`] if there is not going to
    /// be one.
    async fn wait_before_starting_again(
        &self,
        after: Duration,
        attempt: u32,
        restarts: &mut Restarts,
    ) -> Option<StateReason> {
        match self.wait_out(after).await {
            Released::Stopped => None,

            Released::Elapsed => Some(StateReason::BackoffElapsed { attempt }),

            // **A person asking is not the policy coming round again**, and the difference is what
            // `Restarts::recovered` records: the wait goes back to the shortest the policy allows,
            // while the failure history stays, because a service somebody has restarted four times
            // is still a service that has crashed four times.
            Released::Asked => {
                restarts.recovered();

                Some(StateReason::Requested)
            }
        }
    }

    /// Watch a service this daemon adopted: for the daemon asking it to stop, and for it ending.
    ///
    /// **Those two and nothing else, which is what adoption costs.** A health check is not run here
    /// even where the probe would work, because a service that went `Degraded` under it would be put
    /// back by its policy on the strength of a check this daemon has no log to explain the failure
    /// with; and readiness is not re-decided, because the process was proved ready by the daemon
    /// that started it and the check that proved it — a log pattern, most of the time — needs pipes
    /// this one does not have. What the user gets is a service that keeps running, is stopped
    /// properly, and is put back by its policy the moment it crashes, at which point everything is
    /// ordinary again.
    ///
    /// The poll is [`WATCH`], the same one a supervised service is asked at, and the question is the
    /// identity rather than a status: see [`Adopted::exited`].
    async fn watch_adopted(&mut self, adopted: Adopted, restarts: &mut Restarts) -> After {
        loop {
            tokio::select! {
                // Biased towards the stop for the reason the supervised loop is: a daemon on its way
                // out should not spend a whole poll interval on a service it is shutting down.
                biased;

                () = self.cancel.cancelled() => return self.stop_adopted(adopted).await,

                () = tokio::time::sleep(WATCH) => {}
            }

            match adopted.exited() {
                Ok(Some(exit)) => {
                    // The same window the supervised loop publishes `Deciding` for, and for the same
                    // reason: between the process going and `after_exit` persisting what that meant,
                    // a walk must not read the `Up` this service has stopped being.
                    self.readiness.send_replace(Readiness::Deciding);

                    // **An empty capture, not an absent one.** A crash loop decided during an
                    // adopted life has no tail to attach, because the lines went to a pipe that
                    // belonged to a daemon that is gone — which is a fact about this life of the
                    // process and not a fault in the reason, so it is reported as the empty evidence
                    // it is rather than by omitting the reason.
                    let decision =
                        restarts.ended(&exit, std::time::Instant::now(), &Capture::detached());

                    // As the supervised loop above: a service that was up held its own port.
                    return self.after_exit(decision, exit.code(), None).await;
                }

                Ok(None) => {}

                // The OS will not say. Nothing to decide on, and the next tick asks again — the same
                // answer the supervised loop gives, and it matters more here: this question is asked
                // of the OS about somebody else's process, so a transient refusal must not be read
                // as the service having ended.
                Err(error) => tracing::warn!(
                    service = self.spec.id().as_str(),
                    error = %error,
                    "cannot tell whether the adopted process is still running"
                ),
            }
        }
    }

    /// Wait out a backoff, unless something happens that is worth more than the rest of the wait.
    ///
    /// **The bias is the priority order.** A stop beats a start request, because a daemon on its way
    /// out is not going to spawn one more process; a start request beats the remaining wait, which is
    /// the whole of T19c — a person who has just typed `mix service start` at a service in its thirty
    /// second backoff is asking for something the runner would otherwise make them sit through.
    async fn wait_out(&self, after: Duration) -> Released {
        tokio::select! {
            biased;

            () = self.cancel.cancelled() => {
                self.stopped_before_running().await;

                Released::Stopped
            }

            () = self.asked_to_start.notified() => Released::Asked,

            () = tokio::time::sleep(after) => Released::Elapsed,
        }
    }

    /// A stop that found no process to stop — one that arrived during a backoff, or while a start
    /// was still reading its environment — goes straight through `Stopping` to the state a user
    /// asked for.
    async fn stopped_before_running(&self) {
        let reason = self.stopping_because();

        self.move_to(ServiceState::Stopping, reason.clone()).await;
        self.move_to(ServiceState::Stopped, reason).await;
    }

    /// Record that no process belongs to this service any more.
    async fn record_exit(&self, code: Option<i32>) {
        if let Err(error) = services::ended(&self.store, self.spec.id(), code).await {
            tracing::error!(
                service = self.spec.id().as_str(),
                error = %error,
                "cannot record that this service's process has ended; the row still names a pid \
                 that has gone"
            );
        }
    }

    /// Persist a state change, publish the value that was persisted, and say what the service now is
    /// to anything waiting on it. `false` if the change did not land.
    ///
    /// The three in that order, and the readiness last for the same reason the event is not first:
    /// nothing may be told about a move that did not happen. A state that would not persist leaves
    /// the readiness as it was — the service really is still whatever the row still says.
    async fn move_to(&self, to: ServiceState, reason: StateReason) -> bool {
        self.move_to_noting(to, reason, None).await
    }

    /// [`Runner::move_to`], with the sentence that explains a move into `failed` — roadmap task
    /// **T200b**, D5. Written in the transition's own transaction, by `transition_noting`.
    async fn move_to_noting(
        &self,
        to: ServiceState,
        reason: StateReason,
        detail: Option<&str>,
    ) -> bool {
        let persisted = super::record_noting(
            &self.store,
            &self.events,
            self.spec.id(),
            to,
            reason.clone(),
            detail,
        )
        .await;

        if !persisted {
            return false;
        }

        // Sent whether or not anything is listening: `send_replace` keeps the value for whoever asks
        // next, where `send` would report a walk that has already had its answer as a failure.
        self.readiness
            .send_replace(Readiness::of(to, reason, self.spec.restart(), detail));

        true
    }
}

/// Poll an adopted process until it has gone. `false` if it had not within [`GONE`].
///
/// **A free function because both halves of T18 need it and neither owns the other**: this runner
/// waits here when it is asked to stop a service it took over, and [`Registry::discard`] waits here
/// before it clears the row of a survivor it refused. The two are the same claim — nothing may be
/// written down as stopped while the process it names is still running — and writing it twice is how
/// they would come to disagree.
///
/// **The ceiling here is the one a caller with no shutdown to answer to gets**, which is crash
/// recovery's case exactly: `Registry::discard` runs before the first client is served, so there is
/// no budget for it to consult and [`GONE`] entire is the right answer. The runner's own call is
/// shortened by whatever a shutdown has left — done at that call site rather than by a parameter
/// here, for the reason given there.
///
/// [`Registry::discard`]: super::Registry
pub(super) async fn gone(service: &ServiceId, adopted: &Adopted) -> bool {
    let deadline = Instant::now() + GONE;

    loop {
        match adopted.exited() {
            Ok(Some(_)) => return true,

            Ok(None) => {}

            // Unanswerable is not gone. The deadline below is what ends this either way.
            Err(error) => tracing::warn!(
                service = service.as_str(),
                error = %error,
                "cannot tell whether the adopted process has stopped"
            ),
        }

        if Instant::now() >= deadline {
            return false;
        }

        tokio::time::sleep(POLL).await;
    }
}

#[cfg(test)]
mod tests {
    use mixengine_core::Paths;
    use mixengine_proto::LogSubject;

    use super::super::Budget;
    use super::*;

    /// A verdict about a service holding more than it was allowed.
    fn over() -> Over {
        Over {
            rss_bytes: 600 * 1024 * 1024,
            limit_mb: 512,
        }
    }

    /// The four rows of the T71a design's D4 table, as a table.
    #[test]
    fn health_and_memory_fold_into_one_state() {
        assert_eq!(
            fold(true, None),
            (ServiceState::Running, StateReason::Healthy)
        );

        assert_eq!(
            fold(true, Some(over())),
            (
                ServiceState::Degraded,
                StateReason::OverMemory {
                    rss_bytes: 600 * 1024 * 1024,
                    limit_mb: 512
                }
            )
        );

        assert_eq!(
            fold(false, None),
            (ServiceState::Degraded, StateReason::Unhealthy)
        );

        assert_eq!(
            fold(false, Some(over())),
            (ServiceState::Degraded, StateReason::Unhealthy),
            "illness is the more urgent sentence to put in front of a person"
        );
    }

    /// The bug this design was rewritten to avoid.
    ///
    /// A healthy probe must not clear a memory warning: the service is still over its ceiling, and
    /// the runner owns this edge alone precisely so that two inputs cannot overwrite each other.
    #[test]
    fn a_healthy_probe_does_not_clear_an_over_memory_warning() {
        assert_eq!(
            fold(true, Some(over())).0,
            ServiceState::Degraded,
            "health recovering says nothing about size"
        );
    }

    /// Nothing is written when the fold reaches the state the service is already in.
    ///
    /// `can_become` has no self-loops, so a second identical move is an `IllegalTransition` and one
    /// `error!` per minute in `daemon.log`.
    #[test]
    fn an_unchanged_state_produces_no_transition() {
        assert!(!worth_recording(
            ServiceState::Degraded,
            fold(false, None).0
        ));

        assert!(worth_recording(ServiceState::Degraded, fold(true, None).0));
    }

    /// A service that recovers its health while still over its ceiling stays where it is.
    ///
    /// **The state is right and the reason is stale**, and that is the deliberate trade: reaching
    /// `Degraded`/`OverMemory` from `Degraded`/`Unhealthy` is not a legal transition, and the only
    /// route to it would be a `Running` this service never was. Named here so that the day somebody
    /// reads `unhealthy` on a service whose probe is passing, this test says why.
    #[test]
    fn a_reason_may_lag_while_the_state_does_not_change() {
        assert!(!worth_recording(
            ServiceState::Degraded,
            fold(true, Some(over())).0
        ));
    }

    /// The margin every wall-clock assertion here allows itself.
    ///
    /// Only ever weighed against gaps of whole seconds — the one that matters most below is a
    /// second against six — so nothing but the clamp under test failing can close one. Generous
    /// because every number here contains a process spawn, and starting one on a loaded Windows
    /// runner is measured in hundreds of milliseconds.
    pub(super) const SLACK: Duration = Duration::from_secs(2);

    /// A runner over `spec`, supervising nothing.
    ///
    /// Enough for [`Runner::ask_to_stop`], which reaches neither a process nor the row: what it
    /// touches is the spec, the budget and the place its commands run.
    ///
    /// `surroundings` is set, and that is what keeps these tests about the arithmetic. Left [`None`]
    /// it would send [`Runner::where_commands_run`] through a keyring walk on a blocking task, and
    /// what the clock would then be measuring is a mock host rather than a clamp.
    pub(super) fn runner(spec: ServiceSpec, paths: &Paths, store: &Store) -> Runner {
        let place = Surroundings::new(spec.cwd(), BTreeMap::new());
        let directory = paths.service_logs(spec.id());
        let (readiness, _) = watch::channel(Readiness::Deciding);

        Runner {
            spec,
            store: store.clone(),
            directory,
            log: crate::services::logs::Logs::new().reading(&LogSubject::Service {
                id: ServiceId::parse("fake").expect("a usable id"),
            }),
            host: Arc::new(mixengine_platform::mock::Host::with_home(paths.root())),
            superuser: None,
            events: Events::new(),
            cancel: CancellationToken::new(),
            asked_to_start: Arc::new(Notify::new()),
            asked_to_reload: Arc::new(Notify::new()),
            limits_asked: watch::channel(mixengine_proto::ResourceLimits::default()).1,
            stopping_because: watch::channel(None).1,
            over_memory: watch::channel(None).1,
            budget: Budget::default(),
            surroundings: Some(place),
            reading: None,
            readiness,
        }
    }

    /// A runner whose commands have nowhere to run *yet*, over a keyring that will not answer.
    ///
    /// **The adopted-service case, and the only one that pays for this read on the stop path.** Every
    /// runner is built with `surroundings: None` and only a spawn fills it in, so a service this
    /// daemon adopted from a previous one — never spawned here — resolves its environment when it is
    /// asked to stop, which is inside a shutdown.
    pub(super) fn adopted_runner(
        spec: ServiceSpec,
        paths: &Paths,
        store: &Store,
        keyring_takes: Duration,
    ) -> Runner {
        let directory = paths.service_logs(spec.id());
        let (readiness, _) = watch::channel(Readiness::Deciding);

        Runner {
            spec,
            store: store.clone(),
            directory,
            log: crate::services::logs::Logs::new().reading(&LogSubject::Service {
                id: ServiceId::parse("fake").expect("a usable id"),
            }),
            host: Arc::new(mixengine_platform::mock::Host::stalling_on_the_keyring(
                paths.root(),
                keyring_takes,
            )),
            superuser: None,
            events: Events::new(),
            cancel: CancellationToken::new(),
            asked_to_start: Arc::new(Notify::new()),
            asked_to_reload: Arc::new(Notify::new()),
            limits_asked: watch::channel(mixengine_proto::ResourceLimits::default()).1,
            stopping_because: watch::channel(None).1,
            over_memory: watch::channel(None).1,
            budget: Budget::default(),
            surroundings: None,
            reading: None,
            readiness,
        }
    }
}
