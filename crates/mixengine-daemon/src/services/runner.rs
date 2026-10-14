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
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use mixengine_core::{Store, services};
use mixengine_platform::Host;
use mixengine_platform::process::{self, Adopted, Supervised};
use mixengine_proto::{RestartPolicy, ServiceId, ServiceSpec, ServiceState, StateReason};
use mixengine_supervisor::logs::Capture;
use mixengine_supervisor::restart::TAIL_LINES;
use mixengine_supervisor::{Decision, Health, Restarts, Surroundings, Verdict, ready};
use tokio::sync::{Notify, broadcast, watch};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use super::logs::ServiceLog;
use super::now;
use crate::api::Events;

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

    /// One life of the process: spawn it, wait for readiness, then watch it until it ends.
    /// Put everything this capture reads onto the log clients are connected to — roadmap task
    /// **T16b**.
    ///
    /// **A relay and not a fourth sink inside the capture**, deliberately. The reader threads run
    /// outside the runtime and their one obligation is to drain a pipe the service blocks on; making
    /// them also reach into a daemon-side structure would put the daemon's locks on the path of
    /// every line a service prints, where a moment's contention stalls the process itself. What this
    /// task does instead costs the reader threads nothing — the send they already make has one more
    /// subscriber — and everything after that happens on the runtime, where being slow is only slow.
    ///
    /// **It ends on its own**, when the last sender inside the capture is dropped: that is the
    /// capture going away with this run of the process, or the last reader thread reaching end of
    /// file. Neither needs the runner to remember it, which is what keeps this out of every stop
    /// path — a restart makes a new capture and a new relay, and the log they both write into is the
    /// same one the client has been reading since before either existed.
    ///
    /// A relay that falls behind is a gap in the client's stream and says so. It should not happen:
    /// this loop does nothing but move a line from one channel to another, so falling behind means
    /// the runtime itself is starved — but a hole nobody mentions is exactly what
    /// [`LogFrame::Gap`](mixengine_proto::LogFrame::Gap) exists to prevent, and the alternative is a
    /// log panel quietly missing the lines that explain a failure.
    ///
    /// **What the capture already holds is taken here, and not inside the task** — roadmap task
    /// **T16c**. `Capture::start` puts the reader threads on the pipes before it returns, and the
    /// task below is spawned rather than run, so a service can print before the runtime first polls
    /// it. A subscription delivers nothing that was sent before it existed, so every line in that
    /// window used to reach `current.log` and never the ring — permanently, not until something
    /// caught up, and those are a start's first lines. [`Capture::read`] hands over the tail and the
    /// subscription under one lock, so what is taken here is complete and holds no line twice; both
    /// are moved into the task, which records the tail before it pumps anything.
    fn relay(&self, capture: &Capture) {
        let (already_said, mut lines) = capture.read();
        let log = Arc::clone(&self.log);
        let service = self.spec.id().clone();

        tokio::spawn(async move {
            for line in already_said {
                log.record(line);
            }

            loop {
                match lines.recv().await {
                    Ok(line) => log.record(line),

                    Err(broadcast::error::RecvError::Lagged(missed)) => {
                        tracing::warn!(
                            service = service.as_str(),
                            missed,
                            "this daemon fell behind a service's output; the lines it lost are \
                             reported as a gap to everything reading its log"
                        );

                        log.missed(missed);
                    }

                    // Every sender is gone: this run of the process is over and its pipes have been
                    // read to the end.
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
    }

    async fn attempt(&mut self, restarts: &mut Restarts) -> After {
        let env = match self.environment().await {
            Ok(Some(env)) => env,

            // Asked to stop while the keyring was still being read. Nothing was started, so this
            // goes straight through `Stopping`, as a stop during a backoff does.
            Ok(None) => {
                self.stopped_before_running().await;

                return After::Done;
            }

            // A credential the spec names and the keyring does not hold, or one it will not answer
            // for inside `START_ENVIRONMENT`. The process was never started, which is exactly what
            // `SpawnFailed` says. The event stays the reason alone; the sentence naming the entry —
            // its address, never its value — goes on the row (T200b, D5), which is what lets this
            // machine's own MixLab and `mix` say why instead of `daemon.log` alone.
            Err(error) => {
                tracing::error!(
                    service = self.spec.id().as_str(),
                    error = %error,
                    "cannot resolve the environment this service is to be started with"
                );

                return self
                    .give_up(StateReason::SpawnFailed, Some(format!("{error:#}")))
                    .await;
            }
        };

        let args: Vec<OsString> = self.spec.args().iter().map(OsString::from).collect();

        let mut supervised = match process::spawn_supervised(
            self.spec.program(),
            &args,
            self.spec.cwd(),
            &env,
            &super::limits::from_proto(self.spec.limits()),
        ) {
            Ok(supervised) => supervised,

            Err(error) => {
                tracing::error!(
                    service = self.spec.id().as_str(),
                    program = %self.spec.program().display(),
                    error = %error,
                    "cannot start this service"
                );

                return self
                    .give_up(StateReason::SpawnFailed, Some(format!("{error:#}")))
                    .await;
            }
        };

        // Kept for the life of this process rather than rebuilt: what a health probe and a shutdown
        // command need is the environment the service is *running* with, and resolving it again
        // would be an OS keyring read every ten seconds for as long as the service is up.
        self.surroundings = Some(Surroundings::new(self.spec.cwd(), env));

        // Cloned rather than borrowed out of `self`, because the wait below also holds `&mut
        // supervised` and the two would be a borrow of `self` and a borrow inside it at once. Two
        // owned fields, cloned once per start, against a ready check that may run for a minute.
        let place = self
            .surroundings
            .clone()
            .expect("the surroundings were just set");

        // Before anything waits on the process: a pipe nobody drains stops the service writing to
        // it, and a ready check that matches a log pattern has nothing to match against until this
        // exists.
        let capture = Capture::start(
            &mut supervised,
            self.spec.id().as_str(),
            self.spec.logs(),
            Some(&self.directory),
        );

        self.relay(&capture);

        // The three columns nothing wrote before T19, and the pair T18 adopts on. The start time is
        // read while the handle is still held, which is what makes it this child's: an unreaped
        // child keeps its pid reserved on Unix, and on Windows this process holds a handle to it, so
        // the number cannot have been given away in between. A reading that fails leaves the column
        // null rather than guessing — the process is supervised either way, and what a null costs is
        // only that a daemon restart will not adopt it.
        let started_at = match supervised.started_at() {
            Ok(started_at) => started_at,

            Err(error) => {
                tracing::warn!(
                    service = self.spec.id().as_str(),
                    error = %error,
                    "cannot read when this service's process began; a daemon restart will not adopt \
                     it"
                );

                None
            }
        };

        if let Err(error) = services::started(
            &self.store,
            self.spec.id(),
            supervised.pid(),
            started_at.map(process::StartTime::stored),
            now(),
        )
        .await
        {
            tracing::error!(
                service = self.spec.id().as_str(),
                error = %error,
                "cannot record the process this service is running as; it is supervised but a \
                 daemon restart will not adopt it"
            );
        }

        // Raced against the stop for the same reason the watch loop is, and it matters more here:
        // a ready check is allowed tens of seconds, and a daemon that only looked at its token
        // after `ready::wait` returned would sit through all of them on a service it is shutting
        // down. The future is dropped rather than resumed, so nothing here needs to be cancel safe.
        let outcome = tokio::select! {
            biased;

            () = self.cancel.cancelled() => None,

            outcome = ready::wait(self.spec.ready(), &mut supervised, &capture, &place) => Some(outcome),
        };

        // Asked to stop before it was ever ready. Through the same stop the spec asks for: the
        // process is up, `Starting` reaches `Stopping`, and a service that is mid-start is exactly
        // the one whose data directory should not be left to a destructor that kills.
        let Some(outcome) = outcome else {
            return self.stop(supervised, capture).await;
        };

        match outcome {
            Ok(ready::Ready::Ready) => {
                if !self
                    .move_to(ServiceState::Running, StateReason::Ready)
                    .await
                {
                    self.kill(supervised, capture).await;
                    self.record_exit(None).await;

                    return After::Done;
                }

                self.answered_by_this_start();

                self.supervise(supervised, capture, restarts).await
            }

            // The most common way a service fails to start, and the reason `ready::wait` races the
            // exit rather than polling the probe: this is the same path a crash an hour from now
            // takes, and it is the restart policy that decides between them.
            Ok(ready::Ready::Exited(exit)) => {
                // Asked before the process is reaped, and only about a failure: an exit of zero is
                // a service that did what it was told, and nothing about a port could improve on
                // that sentence.
                let conflict = if exit.is_success() {
                    None
                } else {
                    self.port_conflict(Some(supervised.pid())).await
                };

                let capture = self.kill(supervised, capture).await;
                let decision = restarts.ended(&exit, std::time::Instant::now(), &capture);

                self.after_exit(decision, exit.code(), conflict).await
            }

            // Running, and never going to be usable. Killed rather than left: the next attempt would
            // collide with the port and the data directory this one is holding.
            Ok(ready::Ready::TimedOut) => {
                let after = self.spec.ready().timeout();
                // Asked while the process is still up, so that its own pid is there to be
                // recognised: a service that bound its port and then failed its ready check is
                // holding that port itself, and is not in conflict with anybody.
                let conflict = self.port_conflict(Some(supervised.pid())).await;

                // Kept rather than dropped since roadmap task **T127a**: `kill` is also the drain,
                // so what comes back holds the server's last words instead of leaving them in a
                // pipe.
                let capture = self.kill(supervised, capture).await;
                let refused = self.refused_superuser(&capture);

                // **And what it last said, in the daemon's own log.** The service's own log file
                // holds these lines as well, but a ready timeout is the failure a reader meets
                // furthest from that file — in a CI log, in a bug report, in a screenshot — and
                // "not ready within 15s" on its own says only that something was slow. Measured on
                // run 35470533602, where a php-fpm pool timed out on Windows and the report held
                // no line of what the pool had printed.
                let said = capture.recent(TAIL_LINES);
                if !said.is_empty() {
                    tracing::warn!(
                        service = self.spec.id().as_str(),
                        said = said
                            .iter()
                            .map(|line| line.text.as_str())
                            .collect::<Vec<_>>()
                            .join(" / "),
                        "this service was not ready in time; these are its last lines"
                    );
                }

                self.record_exit(None).await;

                // **A conflict outranks a refusal, and the order is not arbitrary.** If another
                // program holds the port, the client this check runs is talking to *that program* —
                // a refusal it printed is not this service's, and who holds the port is the fact
                // worth reporting.
                self.give_up(
                    conflict
                        .or(refused)
                        .unwrap_or(StateReason::ReadyTimeout { after }),
                    None,
                )
                .await
            }

            // A spec this build or this machine cannot check. Not a timeout, and reported as what it
            // is — see `StateReason::Uncheckable`.
            Err(error) => {
                let reason = uncheckable(&error);
                tracing::error!(
                    service = self.spec.id().as_str(),
                    error = %error,
                    "this service cannot be checked for readiness here"
                );

                self.kill(supervised, capture).await;
                self.record_exit(None).await;

                self.give_up(reason, None).await
            }
        }
    }

    /// Watch a running service: for the daemon asking it to stop, for it ending, for it going sick.
    ///
    /// One timer rather than two, and it is the cheaper one that sets the pace: the health check's
    /// interval is measured in seconds and the liveness poll in milliseconds, so the loop wakes on
    /// whichever is next and asks only the question that is due.
    async fn supervise(
        &mut self,
        mut supervised: Supervised,
        capture: Capture,
        restarts: &mut Restarts,
    ) -> After {
        let mut health = self.spec.health().map(Health::watching);
        let mut due = health
            .as_ref()
            .map(|health| Instant::now() + health.interval());

        // Whether the current run of unmakeable probes has already been reported. See the `Err` arm
        // below: a transient fault is worth one line, not one line every interval.
        let mut complained = false;

        // **The two halves of the state this loop owns** — roadmap task T71a. `healthy` starts true
        // because a service with no health check is one nothing has ever said is ill, and `state`
        // starts `Running` because `supervise` is entered from the `Running`/`Ready` transition
        // immediately above it.
        let mut healthy = true;
        let mut state = ServiceState::Running;

        // Taken once, before the loop: the borrow checker's reason is that the loop reaches `&mut
        // self`, and the better one is that a probe every ten seconds must not be a keyring read
        // every ten seconds. Only `HealthProbe::Command` reads it.
        let place = self.where_commands_run().await;

        loop {
            let watch = Instant::now() + WATCH;
            let wake = due.map_or(watch, |due| due.min(watch));

            tokio::select! {
                // Biased towards the stop: a service the daemon has been asked to shut down should
                // not spend one more health interval being probed.
                biased;

                () = self.cancel.cancelled() => {
                    return self.stop(supervised, capture).await;
                }

                // Ahead of the timer and behind the stop. A configuration that has already been
                // written is one the next liveness poll can wait for, and a daemon on its way out
                // should not spend a reload's patience on a service it is about to stop.
                () = self.asked_to_reload.notified() => {
                    self.reload(&place, &supervised).await;

                    continue;
                }

                // Beside the reload and for its reason: something has been written down and the
                // running process has not been told yet. Cheaper than a reload — no patience, no
                // signal, no command — because both mechanisms accept a rewrite with processes
                // already inside them.
                Ok(()) = self.limits_asked.changed() => {
                    let asked = *self.limits_asked.borrow_and_update();

                    if let Err(error) = supervised.set_limits(&super::limits::from_proto(asked)) {
                        // **Logged, and the service is left running.** A mechanism that refused a
                        // value is not a service that has stopped working, and a daemon that killed
                        // a healthy database because a cap could not be written would be doing more
                        // damage than the uncapped service ever could.
                        tracing::warn!(
                            service = self.spec.id().as_str(),
                            error = %error,
                            "cannot apply this service's new limits to the running process"
                        );
                    }

                    continue;
                }

                // **Nothing is done here, deliberately.** This arm exists to wake the loop; what
                // to do about the new value is the same decision a health probe reaches, and it is
                // made once, below, from both inputs.
                Ok(()) = self.over_memory.changed() => {}

                () = tokio::time::sleep_until(wake) => {}
            }

            match supervised.exited() {
                Ok(Some(exit)) => {
                    // Said before the kill rather than after, and this is the only place a readiness
                    // is published without a row behind it. Between a process ending and
                    // `after_exit` persisting what that meant lie a drain bounded by `FLUSH` and a
                    // write, and a walk arriving inside that window would otherwise read the `Up`
                    // this service stopped being — and start the tier below against a database that
                    // has gone. `Deciding` and not `Down` because what happens next is the restart
                    // policy's to say: whoever is waiting waits a moment longer for the answer
                    // rather than being handed a failure that a restart is about to contradict.
                    self.readiness.send_replace(Readiness::Deciding);

                    let capture = self.kill(supervised, capture).await;
                    let decision = restarts.ended(&exit, std::time::Instant::now(), &capture);

                    // No port diagnosis here, and the difference from a failed *start* is the whole
                    // reason: this process had the port. Whatever ended it, nobody took it away —
                    // and if something claims it before the restart, that start is where it will be
                    // met and named.
                    return self.after_exit(decision, exit.code(), None).await;
                }

                Ok(None) => {}

                // The OS will not say. Nothing to decide on, and the next tick asks again.
                Err(error) => tracing::warn!(
                    service = self.spec.id().as_str(),
                    error = %error,
                    "cannot tell whether this service is still running"
                ),
            }

            // **Folded before the health guard below, not inside it** — roadmap task T71a. A
            // service whose recipe declares no `HealthCheck` never reaches that branch, and a fold
            // living there would watch such a service's ceiling and never say a word about it.
            let over = *self.over_memory.borrow_and_update();
            let folded = fold(healthy, over);

            if worth_recording(state, folded.0) && self.move_to(folded.0, folded.1).await {
                state = folded.0;
            }

            let Some(watching) = health.as_mut() else {
                continue;
            };

            if due.is_some_and(|due| Instant::now() < due) {
                continue;
            }

            // Raced against the stop for the same reason the sleep above is: the probe is a program
            // the spec named or an HTTP request, and both are bounded by `HealthCheck::timeout`
            // rather than by anything short. A `mariadb-admin ping` against a database that has
            // stopped answering hangs for the whole of it, and awaiting that outright would make a
            // `mix stop` arriving mid-probe wait out a deadline set for judging health, not for
            // shutting down. Dropping the future is the cancellation: `run_once` kills the child it
            // spawned, and the fold this verdict would have gone into belongs to a loop that is
            // ending anyway.
            let examined = tokio::select! {
                biased;

                () = self.cancel.cancelled() => {
                    return self.stop(supervised, capture).await;
                }

                examined = watching.examine(&place) => examined,
            };

            match examined {
                // **Recorded, not applied** — roadmap task T71a. The verdict changes what this loop
                // believes about the service's health; what state that adds up to is the fold's, at
                // the top of the next turn, because size has a say in it too.
                Ok(Some(Verdict::Degraded)) => {
                    healthy = false;
                }

                Ok(Some(Verdict::Recovered)) => {
                    // The backoff, not the failure history: a service that recovers between crashes
                    // is still crashing, and `Restarts` is what remembers that.
                    restarts.recovered();
                    healthy = true;
                }

                Ok(None) => {}

                // A probe that could not be made *this time*: the binary was being replaced by an
                // upgrade, the machine was out of process slots. **Not a verdict about the service**
                // — nothing was measured, so degrading it would report a bad moment as a sick
                // database — and not a reason to stop asking either, because the next interval is
                // entitled to a different answer. Said once and then only counted, so a fault that
                // lasts an hour is one line in `daemon.log` rather than three hundred.
                Err(error) if error.might_work_later() => {
                    if !complained {
                        complained = true;

                        tracing::warn!(
                            service = self.spec.id().as_str(),
                            error = %error,
                            "this service's health probe could not be made; it will be tried again \
                             at the next interval"
                        );
                    }

                    due = Some(Instant::now() + watching.interval());

                    continue;
                }

                // A probe this build or this machine cannot make. **The service is left alone**,
                // deliberately: it is running and its readiness was proved, and degrading it for a
                // check nobody can make would report a fault in the spec as a fault in the service.
                // Said once, because the answer will not change, and then never probed again.
                Err(error) => {
                    tracing::warn!(
                        service = self.spec.id().as_str(),
                        error = %error,
                        "this service cannot be health-checked here; it will be watched for exiting \
                         and nothing else"
                    );

                    health = None;
                    due = None;

                    continue;
                }
            }

            // Cleared by any probe that was actually made, so a fault that comes back after an hour
            // of working is reported again rather than swallowed by the first one.
            complained = false;

            due = Some(Instant::now() + watching.interval());
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

    /// What happens after the process ended by itself.
    /// Who else is on a port this service was going to listen on — roadmap task **T38**.
    ///
    /// On a blocking task because every implementation of the capability is a synchronous read of
    /// the machine, and one of them starts a process to do it (see `mixengine_platform`'s macOS
    /// module). A join that fails is treated as no diagnosis, which is the same answer every other
    /// failure to ask gets.
    async fn port_conflict(&self, ours: Option<u32>) -> Option<StateReason> {
        if self.spec.ports().is_empty() {
            return None;
        }

        let host = Arc::clone(&self.host);
        let ports = self.spec.ports().to_vec();

        tokio::task::spawn_blocking(move || super::ports::conflict(host.as_ref(), &ports, ours))
            .await
            .ok()
            .flatten()
    }

    /// What this service's last words say about a refused superuser, if anything — task **T127a**.
    ///
    /// **The same shape as [`Runner::port_conflict`], and asked at the same moment**: a start that
    /// has already failed, and a question that can turn an honest but useless reason into a useful
    /// one. Nothing here can fail a start that was going to succeed, which is the whole reason this
    /// reads a failure rather than racing one — a reader that aborted a start on one log line could
    /// take down a cluster somebody's application was merely polling with a stale password.
    ///
    /// The capture must already be drained — [`Runner::kill`] is what does that — or the server's
    /// last line is still sitting in a pipe.
    fn refused_superuser(&self, capture: &Capture) -> Option<StateReason> {
        let superuser = self.superuser?;
        let recent = capture.recent(TAIL_LINES);

        super::databases::refusal_in_log(recent.iter().map(|line| line.text.as_str()), superuser)
            .map(|said| StateReason::SuperuserRefused { said })
    }

    async fn after_exit(
        &self,
        decision: Decision,
        code: Option<i32>,
        conflict: Option<StateReason>,
    ) -> After {
        self.record_exit(code).await;

        match decision {
            // It did what it was asked to. Through `Stopping`, which is the only edge the machine
            // has into `Stopped` — a service that exited cleanly is stopping for exactly as long as
            // these two writes take.
            Decision::Rest { reason } => {
                self.move_to(ServiceState::Stopping, reason.clone()).await;
                self.move_to(ServiceState::Stopped, reason).await;

                After::Done
            }

            // **A conflict replaces an exit and never a crash loop.** `StateReason::Exited` says
            // only what the OS said, and a port held by somebody else is the same fact with the
            // useful half attached; `StateReason::CrashLoop` carries its own count and the lines
            // the service printed, which is more than this could add.
            Decision::GiveUp { reason } => {
                let reason = match (reason, conflict) {
                    (StateReason::Exited { .. }, Some(conflict)) => conflict,
                    (reason, _) => reason,
                };

                self.give_up(reason, None).await
            }

            // The `Restarting` is what answers a walk that is waiting on this service, which is why
            // a failure to persist it ends the task: a runner that went on restarting from a row
            // nobody could read would leave that walk with nothing to wait for.
            Decision::Restart { after, attempt } => {
                if !self
                    .move_to(
                        ServiceState::Restarting,
                        conflict.unwrap_or(StateReason::Exited { code }),
                    )
                    .await
                {
                    return After::Done;
                }

                After::Again { after, attempt }
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

    /// Take the request this start has just answered, so that no later crash is released by it.
    ///
    /// **A permit is only ever consumed by [`Runner::wait_out`], and a runner that is mid-start is
    /// not in one.** A request that arrives while `ready::wait` is running — two walks sharing a
    /// dependency, which is the ordinary case and not the rare one — is kept by the [`Notify`] until
    /// something waits on it. If this start then *succeeds*, nothing does for as long as the service
    /// stays up: the permit outlives the request entirely, and the next crash — hours later and
    /// asked for by nobody — leaves its backoff the instant it enters it, with the ladder reset by
    /// [`Restarts::recovered`] and the move published as [`StateReason::Requested`].
    ///
    /// Reaching `Running` is what makes such a request *answered* rather than dropped: whoever asked
    /// for this service to be started now has it started now, which is the whole of what they asked
    /// for. A request that arrives after this is about the life that follows, and [`Runner::wait_out`]
    /// is where it belongs.
    fn answered_by_this_start(&self) {
        let asked = std::pin::pin!(self.asked_to_start.notified());

        // `enable` is how a stored permit is taken without waiting for one: it registers this
        // future's interest and says whether there was already something to receive. The future is
        // dropped on the next line, which is what makes this a read and not a wait.
        if asked.enable() {
            tracing::debug!(
                service = self.spec.id().as_str(),
                "a start asked for while this service was starting is answered by that start"
            );
        }
    }

    /// Move to `Failed` for `reason`. Whoever is waiting on this service is answered by that move.
    async fn give_up(&self, reason: StateReason, detail: Option<String>) -> After {
        self.move_to_noting(ServiceState::Failed, reason, detail.as_deref())
            .await;

        After::Done
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

/// Turn a supervisor refusal into the reason a user reads.
///
/// [`Error::UnsupportedCheck`](mixengine_supervisor::Error::UnsupportedCheck) already carries both
/// halves in the words the spec's author needs, so they are passed through rather than re-worded.
/// Everything else — a pattern that will not compile, a socket this OS does not have — is described
/// by its own chain, which is where `mixengine-platform` writes what it refused and why.
fn uncheckable(error: &mixengine_supervisor::Error) -> StateReason {
    match error {
        mixengine_supervisor::Error::UnsupportedCheck { check, reason } => {
            StateReason::Uncheckable {
                check: (*check).to_owned(),
                reason: reason.clone(),
            }
        }

        // A pattern that will not compile, a socket this OS does not have. Flattened rather than
        // printed, because these types carry the cause as a `source` precisely so that no layer
        // repeats it — and the layer showing it to a person is the one that has to join it back up.
        other => StateReason::Uncheckable {
            check: "the readiness check this service declares".to_owned(),
            reason: mixengine_proto::flatten(other),
        },
    }
}

#[cfg(test)]
mod tests {
    use mixengine_core::Paths;
    use mixengine_proto::LogSubject;

    use super::super::Budget;
    use super::super::fixture::{home, spec};
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

    /// **A spawn that failed keeps its sentence on the row** — T200b, D5. Before it, `daemon.log`
    /// was the only place a missing credential was named.
    #[tokio::test]
    async fn a_spawn_that_failed_leaves_its_sentence_on_the_row() {
        let (_home, paths, store) = home(&["mariadb"]).await;
        sqlx::query("UPDATE services SET state = 'starting' WHERE id = 'mariadb'")
            .execute(store.pool())
            .await
            .expect("the row");

        let mut runner = adopted_runner(
            spec("mariadb")
                .env_from_keyring("MARIADB_ROOT_PASSWORD", "mixengine", "mariadb@main/root")
                .build()
                .expect("a usable spec"),
            &paths,
            &store,
            Duration::ZERO,
        );
        let mut restarts = Restarts::under(runner.spec.restart());

        runner.attempt(&mut restarts).await;

        let note =
            mixengine_core::services::record(&store, &ServiceId::parse("mariadb").expect("an id"))
                .await
                .expect("the row")
                .last_failure
                .expect("a note");
        assert_eq!(note.reason, StateReason::SpawnFailed);
        assert!(
            note.detail
                .contains("no credential is stored at mixengine/mariadb@main/root"),
            "{}",
            note.detail
        );
    }
}
