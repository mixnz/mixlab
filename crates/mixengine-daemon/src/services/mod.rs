//! The registry of running services: what the daemon is supervising, and how a plan is walked.
//!
//! **This is where the timing lives.** `mixengine-supervisor` has the mechanisms and no loop;
//! `mixengine-core` has the graph, the state machine and the row; the daemon is what holds a task
//! per service, the [`CancellationToken`] it stops on, and the clock both of those are measured by.
//! Roadmap task **T19**.
//!
//! It is also where a daemon's first act lives: [`Registry::recover`] reconciles the rows the last
//! daemon left behind — adopting the processes that survived it, stopping the ones nothing can
//! supervise, and clearing the rows whose process is gone. Roadmap task **T18**, and the reason this
//! module reads a `services` row it did not write.
//!
//! A walk is **sequential over [`Plan::flat`]**, which is what T17 left this free to be: the tiers
//! are already computed, so M3's ten-second budget buys concurrency by changing this walker and
//! nothing else. A tier that fails stops the walk, and everything below it is marked
//! [`StateReason::DependencyFailed`] rather than spawned against a dependency that is not there.

pub(crate) mod activate;
pub(crate) mod autostart;
pub(crate) mod databases;
// `fakeservice` is a supervised program this build ships only in a debug binary — the gate belongs
// to it and to nothing above it.
#[cfg(debug_assertions)]
mod fakeservice;
mod first_run;
#[cfg(test)]
pub(crate) mod fixture;
mod graph;
pub(crate) mod hold;
pub(crate) mod idle;
pub(crate) mod limits;
pub(crate) mod logs;
mod ports;
pub(crate) mod reset;
mod runner;
mod shutdown;
pub(crate) mod spec;
mod start;
mod step;
mod stop;
pub(crate) mod watchdog;

pub(crate) use spec::generator;

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

use mixengine_core::services::{self, Plan, ServiceGraph};
use mixengine_core::{Paths, Store};
use mixengine_platform::Host;
use mixengine_platform::process::{Adopted, StartTime};
use mixengine_proto::{DaemonEvent, ServiceId, ServiceState, StateReason, Timestamp};
use tokio::sync::{Notify, watch};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::api::Events;
use logs::Logs;
use runner::{Readiness, gone};
pub(crate) use shutdown::Budget;

pub(crate) use spec::{SpecSource, catalogue, declared};

/// Everything the daemon is supervising, and the only thing that starts or stops one.
#[derive(Debug)]
pub(crate) struct Registry {
    /// Where a service's `current.log` goes.
    paths: Paths,

    /// The state rows. Every move is written here before it is announced.
    store: Store,

    /// The OS, for the credentials a spec names and cannot carry.
    host: Arc<dyn Host>,

    /// Whether a site with nothing behind it answers with MixEngine's page — roadmap task **T124**.
    ///
    /// **Here rather than beside every generator that needs it.** `config.sites.welcome_page` is
    /// read once at boot, and four call sites build a generator from `paths`, `store` and `host` —
    /// `doctor`, `repair`, the activator walk and an extension's `would_serve`. Every one of them
    /// already holds this registry, and `Doctor::new` is at the seven arguments clippy allows, so a
    /// parameter beside the other three would have been the one that broke it. A drift check must
    /// render exactly what an install renders, so all five must read one answer.
    welcome: bool,

    /// Where a persisted transition is published.
    events: Events,

    /// Where a [`ServiceSpec`] comes from — see [`spec`].
    ///
    /// [`ServiceSpec`]: mixengine_proto::ServiceSpec
    specs: Arc<dyn SpecSource>,

    /// The daemon's root token. Every runner's token is a child of this one, so nothing this
    /// registry spawns can outlive the daemon.
    shutdown: CancellationToken,

    /// What a shutdown has left to spend, once one is under way — see [`Budget`].
    budget: Budget,

    /// Whether a shutdown has begun, which is what [`Registry::begin`] refuses new work on.
    ///
    /// **Separate from [`Registry::budget`] rather than derived from it**, which is the tempting
    /// shape and the wrong one: a budget is what a shutdown was *granted*, and a shutdown can begin
    /// without one being expressible — see [`Registry::stopping_within`], where a grace this
    /// machine's clock cannot name leaves the budget as it was. Deriving the question from the
    /// answer would make that exact shutdown the one that goes on starting services.
    shutting_down: AtomicBool,

    /// One entry per service with a task supervising it.
    ///
    /// A `std` mutex rather than tokio's: nothing awaits while holding it, and the alternative
    /// would make every reader of "what is running" an async function for no reason.
    running: Arc<Mutex<HashMap<ServiceId, Running>>>,

    /// The addresses being held on stopped services' behalf — roadmap task **T70a**.
    ///
    /// **Beside [`Registry::running`] rather than inside it**, although the two are the same idea
    /// turned over: that map is one entry per service something is supervising, and this is one
    /// entry per service nothing is supervising and something may still connect to.
    holder: Arc<hold::Holder>,

    /// What a client reads on `GET /logs/{id}` — roadmap task **T16b**.
    ///
    /// **Deliberately not part of [`Registry::running`]**, although every line in it comes from
    /// something that is: a `follow` has to survive the run it started in, and an entry that lived
    /// in the map above would be dropped by the crash it is most worth watching. See
    /// [`logs::Logs`], and
    /// `docs/decisions/0009-logs-travel-on-their-own-stream.md` for why output is not an event.
    logs: Arc<Logs>,

    /// One entry per stop in flight, so a second caller waits for it instead of racing it — see
    /// [`Stopping`] and [`Registry::stop_one`].
    ///
    /// **Taken after [`Registry::running`] wherever both are held**, which is [`Registry::stop_one`]
    /// and [`Registry::shut_down`] and nowhere else. The order is what the pairing needs rather than
    /// merely what avoids a deadlock: claiming a stop and taking its entry out of the map have to be
    /// one decision, or a shutdown draining between the two leaves a claim nobody can wait on.
    stopping: Arc<Mutex<HashMap<ServiceId, watch::Receiver<bool>>>>,

    /// Hands out the generation below.
    generations: AtomicU64,

    /// The job registry, for the one thing a start may have to do that is too long to answer inline:
    /// a first-run ritual — roadmap task **T33**.
    jobs: Arc<crate::jobs::Jobs>,

    /// What each declared service still has to have done to it once, refreshed at every walk.
    ///
    /// **Held here rather than carried through [`Registry::start`]'s signature**, beside
    /// [`Registry::hand_over`] and for its reason: [`Registry::graph`] is the only place both halves
    /// are visible — the source knows what it generated, and this registry knows what it is about to
    /// begin. A [`ServiceGraph`] carries specs, and a ritual is not one.
    rituals: Arc<Mutex<HashMap<ServiceId, mixengine_core::generate::FirstRun>>>,

    /// How each declared service's databases are administered, if it has any — roadmap task
    /// **T77a**.
    ///
    /// Beside `rituals` and filled on the same walk, for the same reason: a
    /// [`Provisioning`](mixengine_core::generate::Provisioning) holds a cloned
    /// [`Context`](mixengine_core::generate::Context), which only the generator can build.
    provisioning: Arc<Mutex<HashMap<ServiceId, mixengine_core::generate::Provisioning>>>,

    /// The graph the last [`Registry::graph`] produced, and when — roadmap task **T190b**.
    ///
    /// Read only by the idle sweep, through [`Registry::graph_for_sweep`]. Every other caller
    /// renders, because every other caller either changes this home or answers somebody who just
    /// asked; the sweep asks every thirty seconds whether anything is idle, and rendering and
    /// installing every service to answer that was half of what the daemon spent at rest.
    last_graph: Mutex<Option<(tokio::time::Instant, ServiceGraph)>>,
}

/// One service being supervised.
#[derive(Debug)]
struct Running {
    /// Cancel it to stop the service the way its spec asks.
    cancel: CancellationToken,

    /// Notify it to ask the runner to start the service *now*.
    ///
    /// **The one thing the registry can do to a live runner besides read it**, and the whole of
    /// T19c: a runner sitting out a restart backoff is not reachable through [`Running::readiness`],
    /// which is a report and not a request, so an explicit start had nothing to act on. See
    /// [`Registry::begin`].
    asked_to_start: Arc<Notify>,

    /// Notify it to hand the running process the configuration that has just been rewritten under
    /// it — roadmap task **T31**, and see [`Registry::hand_over`].
    ///
    /// The sibling of [`Running::asked_to_start`] in mechanism and its opposite in what it means: a
    /// start is asked of a service that is *not* up, and a reload only ever of one that is.
    asked_to_reload: Arc<Notify>,

    /// Push a new set of ceilings at the runner, which writes them into the live process.
    ///
    /// **A `watch` rather than a [`Notify`] like its two neighbours**, and the difference is that
    /// this one carries a value: `asked_to_start` and `asked_to_reload` ask the runner to go and look
    /// at something that has already been written down, while a limit *is* the message. The same
    /// channel type [`Running::readiness`] uses, running the other way.
    limits: watch::Sender<mixengine_proto::ResourceLimits>,

    /// Tell the runner why the stop it is about to perform is happening — roadmap task **T69**.
    ///
    /// [`None`] is *somebody asked*, and it is what every stop but one is. Set by
    /// [`Registry::stopping_because`] immediately before the cancel, so the transition a client
    /// reads says `idle` rather than claiming a person asked for it.
    stopping_because: watch::Sender<Option<StateReason>>,

    /// What the memory watchdog last concluded about this service — roadmap task **T71a**.
    ///
    /// **A `watch` like [`Running::limits`] rather than a [`Notify`] like its other neighbours**,
    /// and for that field's reason: this one carries a value. The runner folds it with its own
    /// health verdict instead of acting on it, because the two are readings of different things at
    /// different rates and only one of them may write the state.
    over_memory: watch::Sender<Option<runner::Over>>,

    /// The runner, so a stop can wait for it rather than assume.
    task: JoinHandle<()>,

    /// Which run of this service this is.
    ///
    /// What keeps a task that is ending from removing an entry that is no longer its own: a service
    /// that fails and is started again by the same walk has two tasks alive for an instant, and
    /// without this the older one's tidy-up would deregister the newer one.
    generation: u64,

    /// What that runner last decided about the service, which is the only thing that says whether it
    /// is up. **Not the task's liveness** — see [`Readiness`].
    readiness: watch::Receiver<Readiness>,
}

/// One stop of one service, held for exactly as long as that stop is in flight.
///
/// **A guard rather than two statements, because a stop does not only end by returning.** An
/// `api/rpc.rs` handler is a `tokio::spawn` nothing joins, so the future performing a stop can be
/// dropped where it stands when its connection goes — and a runner task that panicked mid-tidy-up
/// takes its caller's frame with it. Whatever ends the stop, the entry has to leave the map and
/// everybody waiting on it has to be released: a marker left behind would make every later stop of
/// that service wait for a stop that is not happening, and then report it as done.
///
/// The value published is *"no longer in flight"* and never *"it worked"*. Which is a distinction
/// with a reason: the answer to a stop comes from the row and from nothing else — see
/// [`Registry::stop_one`] — so what a second caller needs from this is the moment at which reading
/// that row is honest, not a second opinion travelling beside it.
#[derive(Debug)]
struct Stopping {
    /// Which service's stop this is.
    service: ServiceId,

    /// The map to take it out of, shared with the registry that handed it out.
    stopping: Arc<Mutex<HashMap<ServiceId, watch::Receiver<bool>>>>,

    /// What releases whoever is waiting for this stop.
    finished: watch::Sender<bool>,
}

impl Drop for Stopping {
    fn drop(&mut self) {
        lock(&self.stopping).remove(&self.service);

        // Ignored, and the two ways it fails are both ordinary: nobody is waiting, or everybody who
        // was has gone. Neither is news, and a stop that has ended cannot do anything about either.
        let _ = self.finished.send(true);
    }
}

/// How a start of one service ended, for the walk that is waiting on it.
/// What was persisted about a service that did not get where a walk wanted it — roadmap task
/// **T202a**, D3: the reason the state machine recorded, and the sentence the runner wrote beside
/// it (T200b's `last_failure`), which is the one a person reads first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Fault {
    /// [`None`] when the failure was the daemon's own — a database that would not take the write,
    /// a runner task that panicked — which is in `daemon.log` and is not a state a client could
    /// render.
    pub(crate) reason: Option<StateReason>,

    /// What the runner wrote, where it wrote anything.
    pub(crate) detail: Option<String>,
}

impl Fault {
    /// A failure nothing was persisted about.
    pub(crate) fn unexplained() -> Self {
        Self::default()
    }

    /// The sentence a caller prints: the detail, else the reason, else nothing after the colon.
    pub(crate) fn said(&self, failed: &ServiceId) -> String {
        match (&self.detail, &self.reason) {
            (Some(detail), _) => format!("{failed} did not start: {detail}"),
            (None, Some(reason)) => format!("{failed} did not start: {reason}"),
            (None, None) => format!("{failed} did not start"),
        }
    }
}

#[derive(Debug)]
enum Start {
    /// The service is up. Traffic can be routed to it, and the next tier may start.
    Ready,

    /// It is not, and this is the [`Fault`] persisted about why.
    Failed(Fault),
}

/// What a walk did.
///
/// Not a `Result`: a plan of six services where the fourth fails has three that are running, one
/// that failed and two that were never tried, and a caller that has to render that needs all three
/// lists. T19a's `service.start` is the first such caller.
#[derive(Debug, Default)]
pub(crate) struct Walk {
    /// Services that reached what the walk was aiming for, in the order they got there.
    pub(crate) reached: Vec<ServiceId>,

    /// The service that stopped the walk, and the [`Fault`] persisted about it.
    ///
    /// An unexplained fault when the failure was the daemon's own — see [`Start::Failed`] — and
    /// for the one failure a *stop* has, which is a survivor that would not die: there is no
    /// persisted reason to quote there, because the row is deliberately left in the state it was
    /// already in. See [`Registry::stop`].
    pub(crate) failed: Option<(ServiceId, Fault)>,

    /// Services never tried, because something they depend on failed.
    pub(crate) blocked: Vec<ServiceId>,
}

/// What one boot's reconciliation found — roadmap task **T18**.
///
/// Lists rather than counts, because the one caller that is not a test writes them into
/// `daemon.log`: "adopted mariadb@main" is the line somebody reads a week later to understand why a
/// database had been up for longer than the daemon watching it, and a number would not answer that.
///
/// **Every row this touched is in exactly one of them**, including the ones it could not finish
/// with. A reconciliation that reported only its successes would let the summary line say nothing
/// happened in the very boot where a survivor refused to die — which is the boot somebody is reading
/// the log for.
#[derive(Debug, Default)]
pub(crate) struct Recovery {
    /// Services whose process survived and is now supervised again.
    pub(crate) adopted: Vec<ServiceId>,

    /// Survivors this daemon stopped rather than adopt: nothing declares them, or they were left in
    /// a state adoption cannot resume.
    pub(crate) stopped: Vec<ServiceId>,

    /// Rows that claimed a process which was not there. Nothing was signalled for these.
    pub(crate) cleared: Vec<ServiceId>,

    /// Survivors this daemon meant to stop and could not, whose rows are therefore **left as they
    /// were found** — still naming the process, still in a supervised state.
    ///
    /// Not a failure of the boot: the daemon serves clients either way, and the next one meets the
    /// same case and tries again. It is here because it is the one outcome that leaves the machine
    /// holding a port nothing supervises, and it must not be reported as quiet.
    pub(crate) refused: Vec<ServiceId>,
}

impl Recovery {
    /// Whether there was anything to reconcile at all, which is the ordinary case.
    pub(crate) fn is_empty(&self) -> bool {
        self.adopted.is_empty()
            && self.stopped.is_empty()
            && self.cleared.is_empty()
            && self.refused.is_empty()
    }
}

/// Why the declared services could not be assembled into a graph.
///
/// **Two variants because they are two different people's problem**, and the wire mapping has to
/// tell them apart: a source that failed is the daemon's and is an internal error, while a set that
/// is not a graph is what the user declared and is `invalid_argument` — the mapping T17 fixed, which
/// [`crate::error::ToWire`] already applies to [`mixengine_core::Error::Graph`].
///
/// **Both halves carry the same type, and the variant is still the whole point.** What differs is
/// where the failure came from and therefore who has to act on it, not what it is made of: a source
/// failure and a bad graph are both [`mixengine_core::Error`], because that crate is the one with
/// the vocabulary. `Unavailable` used to hold an `anyhow::Error` instead, and the wire mapping had
/// to downcast back to this type to keep the code it had thrown away.
///
/// Not an [`std::error::Error`] itself: nothing wraps it, and the one caller matches it and hands
/// each half to the mapping that already exists for it — [`crate::error::ToWire`], where the
/// `service.*` handlers meet it.
///
/// Both halves are boxed: `mixengine_core::Error` is over 128 bytes, and the two `async fn`s that
/// answer with this type would otherwise carry it through one more frame, which is what
/// `clippy::result_large_err` flags. Every reader only displays it or hands it to `to_wire`.
#[derive(Debug)]
pub(crate) enum Undeclarable {
    /// The source could not produce them: a package that is not installed, a template that does not
    /// render, a database that cannot be read.
    Unavailable(Box<mixengine_core::Error>),

    /// They are not a graph: a cycle, a dependency on something that is not declared, two services
    /// with the same id.
    Invalid(Box<mixengine_core::Error>),
}

impl Registry {
    /// What `config.toml` says about the welcome page — roadmap task **T124**.
    ///
    /// A setter rather than an eighth argument to [`new`](Self::new), which is at the seven
    /// `clippy::too_many_arguments` allows. The daemon calls it once with
    /// `config.sites.welcome_page`; everything else keeps the default, which is the same answer a
    /// home with no `[sites]` section gives.
    pub(crate) fn with_welcome(mut self, welcome: bool) -> Self {
        self.welcome = welcome;
        self
    }

    /// A registry with nothing running.
    pub(crate) fn new(
        paths: &Paths,
        store: &Store,
        host: Arc<dyn Host>,
        events: Events,
        specs: Arc<dyn SpecSource>,
        shutdown: CancellationToken,
        jobs: Arc<crate::jobs::Jobs>,
    ) -> Self {
        Self {
            paths: paths.clone(),
            store: store.clone(),
            host,
            // **On unless this home said otherwise** — roadmap task T124, and a setter rather than
            // an eighth argument: `new` is at the seven clippy allows. The default is the product's
            // own, so a fixture that never mentions it renders what a home renders.
            welcome: true,
            events,
            specs,
            shutdown,
            budget: Budget::default(),
            shutting_down: AtomicBool::new(false),
            running: Arc::new(Mutex::new(HashMap::new())),
            holder: Arc::new(hold::Holder::default()),
            logs: Arc::new(Logs::new()),
            stopping: Arc::new(Mutex::new(HashMap::new())),
            generations: AtomicU64::new(0),
            jobs,
            rituals: Arc::new(Mutex::new(HashMap::new())),
            provisioning: Arc::new(Mutex::new(HashMap::new())),
            last_graph: Mutex::new(None),
        }
    }

    /// Ask one running service to re-read its configuration, and say whether there was one to ask.
    ///
    /// [`Registry::hand_over`]'s single-service half, for the one caller whose change is not a file
    /// this registry generated: a runtime's ini set is rewritten by `runtime.set_extension`, and the
    /// pool reading it is supervised here.
    /// Write a new set of ceilings into a service that is running now — roadmap task **T68**.
    ///
    /// `false` when nothing is supervising that id, which is not a failure: a stopped service's
    /// limits live in its row and are read by the next spawn. The caller has already written the row.
    pub(crate) fn set_limits(
        &self,
        id: &ServiceId,
        limits: mixengine_proto::ResourceLimits,
    ) -> bool {
        let running = lock(&self.running);

        let Some(entry) = running.get(id) else {
            return false;
        };

        entry.limits.send_replace(limits);
        true
    }

    /// Say why the next stop of this service is happening, before asking for it — task **T69**.
    ///
    /// **Set, then cancel, in that order**: the runner reads this at the moment it enters
    /// `Stopping`, so a value written after the cancel arrives too late to explain the stop it was
    /// meant for.
    ///
    /// **And cleared again if that stop does not happen**, with `None`, which is what the argument
    /// is an [`Option`] for. A reason left behind by a stop that failed would be read by whatever
    /// stopped the service next — a person asking, most likely — and told them their own
    /// `service.stop` was an idle timeout. The idle sweeper is the only caller and does both.
    ///
    /// [`false`] when nothing is supervising this service, which is a service that is not running
    /// and therefore not one anything is about to stop.
    pub(crate) fn stopping_because(&self, id: &ServiceId, reason: Option<StateReason>) -> bool {
        let running = lock(&self.running);

        let Some(entry) = running.get(id) else {
            return false;
        };

        entry.stopping_because.send_replace(reason);
        true
    }

    /// Tell the runner what the memory watchdog concluded — roadmap task **T71a**.
    ///
    /// [`None`] is *under its ceiling, or not measured*, which are the same fact to a reader of
    /// state. The runner folds this with its own health verdict rather than being told what state to
    /// be in: two writers of the `Running`/`Degraded` edge would overwrite each other in both
    /// directions.
    ///
    /// [`false`] when nothing is supervising this service, which is a service that is not running
    /// and therefore not one the sampler produced a minute for.
    pub(crate) fn over_memory(&self, id: &ServiceId, over: Option<runner::Over>) -> bool {
        let running = lock(&self.running);

        let Some(entry) = running.get(id) else {
            return false;
        };

        entry.over_memory.send_replace(over);
        true
    }

    pub(crate) fn ask_to_reload(&self, id: &ServiceId) -> bool {
        let running = lock(&self.running);

        let Some(entry) = running.get(id) else {
            return false;
        };

        entry.asked_to_reload.notify_one();
        true
    }

    /// Reconcile what the daemon before this one left behind — roadmap task **T18**.
    ///
    /// Called once, before anything is served, and it is the only thing in this registry that reads
    /// a `services` row it did not write. A daemon that was killed — or a machine that lost power —
    /// leaves rows claiming a supervisor that no longer exists, and every one of them is one of two
    /// things:
    ///
    /// - **a survivor**: the pid is still there *and* the process bearing it began at the moment
    ///   that was recorded. It is adopted, and from here on it is supervised like anything else. The
    ///   pair is the whole check, because a pid on its own is reused within minutes and signalling
    ///   somebody else's program is the one accident this product cannot have.
    /// - **gone**: nothing has that pid, or what does is a different process. The row is cleared and
    ///   the service is recorded as `stopped` with [`StateReason::Vanished`]. **Nothing is
    ///   signalled**, which is the point of failing the identity check rather than trusting the
    ///   number.
    ///
    /// A survivor is not always adoptable, and the third outcome is the one that stops it: a service
    /// nothing declares any more cannot be supervised at all, and one whose row says `starting`,
    /// `stopping` or `restarting` cannot be resumed from where it was — a readiness that was never
    /// decided cannot be re-decided without the pipes that went with the old daemon. Leaving either
    /// running would leave the port and the data directory held by a process the next start collides
    /// with, so they are stopped, and the reason says which it was.
    ///
    /// A survivor that will not go is the fourth outcome and the only one that leaves work undone:
    /// its row is left exactly as it was found, and it is reported in [`Recovery::refused`] so the
    /// boot does not summarise itself as quiet. The next daemon meets the same row and tries again.
    ///
    /// **A stopped survivor is killed rather than asked**, unlike every other stop in this crate.
    /// A boot is not the moment to spend a `StopBehaviour`'s grace period per service on processes
    /// this daemon has already decided it cannot supervise — the daemon would answer no client until
    /// the last of them had gone. For a database that means recovery on its next start, which is the
    /// same cost a stop command that fails carries — see [`Runner::ask_to_stop`](runner) — and it is
    /// stated in the row rather than hidden.
    ///
    /// Adoption writes **no transition** for the service it takes over: nothing happened to it. Its
    /// row said `running` before this process existed and says `running` still.
    pub(crate) async fn recover(&self) -> Recovery {
        let mut recovery = Recovery::default();

        // **First, and before any row is read** — roadmap task **T68**. On Linux a supervised group
        // is a *directory*, so a daemon that was killed leaves one behind for every service it was
        // capping. Nothing on the other two systems, where a group is a kernel object that goes when
        // the last handle to it closes.
        //
        // Safe to run before adoption rather than after, and that is the point: an empty cgroup is
        // removable and a non-empty one is not, so this takes exactly the ones whose processes are
        // already gone and leaves the ones belonging to survivors this reconciliation is about to
        // adopt. It needs no list of what to expect because the kernel is the list.
        mixengine_platform::process::sweep_stale_groups();

        let records = match services::records(&self.store).await {
            Ok(records) => records,

            // Nothing can be reconciled and nothing is: the daemon carries on with an empty registry
            // rather than refusing to start, because a `services` table that cannot be read is a
            // problem every later request will report for itself.
            Err(error) => {
                tracing::error!(
                    %error,
                    "cannot read what the last daemon left behind; no service will be adopted"
                );

                return recovery;
            }
        };

        // Asked once for the whole reconciliation rather than per row. A source that cannot answer
        // is not a reason to leave survivors running — it only means none of them can be adopted,
        // which is what an empty graph then says for every one of them.
        let declared = match self.graph().await {
            Ok(graph) => Some(graph),

            Err(error) => {
                let reason: &dyn std::fmt::Display = match &error {
                    Undeclarable::Unavailable(why) => why,
                    Undeclarable::Invalid(why) => why,
                };

                tracing::error!(
                    %reason,
                    "cannot tell which services are declared; anything that outlived the last \
                     daemon will be stopped rather than adopted"
                );

                None
            }
        };

        for (stored, record) in records {
            // A row in `stopped` or `failed` with no process named is already telling the truth
            // about a machine with no daemon on it. Everything else is either claiming a supervisor
            // or naming a pid, and both are this function's business.
            if !record.state.is_supervised() && record.pid.is_none() {
                continue;
            }

            let service = match ServiceId::parse(&stored) {
                Ok(service) => service,

                Err(error) => {
                    tracing::error!(
                        service = stored,
                        %error,
                        "the services table holds an id this build cannot read; leaving the row \
                         alone"
                    );

                    continue;
                }
            };

            self.reconcile(&service, &record, declared.as_ref(), &mut recovery)
                .await;
        }

        recovery
    }

    /// Reconcile only the rows this registry holds no runner for — roadmap task **T47b**.
    ///
    /// **[`recover`](Self::recover) with the set narrowed, and narrowed for a reason that is not
    /// tidiness.** That function walks *every* row, which is right at a boot where nothing is
    /// supervised yet and wrong on a running daemon: it would adopt or stop services this process is
    /// already watching. The rows a live daemon may decide anything about are the ones it holds no
    /// runner for.
    ///
    /// The decision per row is [`reconcile`](Self::reconcile)'s and is not restated here — adopt when
    /// the pid *and* the recorded start time both match, stop what cannot be resumed or is no longer
    /// declared, and never signal on a pid alone.
    pub(crate) async fn reconcile_stranded(&self) -> Recovery {
        let mut recovery = Recovery::default();

        let records = match services::records(&self.store).await {
            Ok(records) => records,

            Err(error) => {
                tracing::error!(
                    %error,
                    "cannot read the services table; nothing stranded can be reconciled"
                );

                return recovery;
            }
        };

        let declared = match self.graph().await {
            Ok(graph) => Some(graph),

            Err(error) => {
                let reason: &dyn std::fmt::Display = match &error {
                    Undeclarable::Unavailable(why) => why,
                    Undeclarable::Invalid(why) => why,
                };

                tracing::error!(
                    %reason,
                    "cannot tell which services are declared; anything stranded will be stopped \
                     rather than adopted"
                );

                None
            }
        };

        let held = self.supervised();

        for (stored, record) in records {
            if !record.state.is_supervised() && record.pid.is_none() {
                continue;
            }

            let service = match ServiceId::parse(&stored) {
                Ok(service) => service,

                Err(error) => {
                    tracing::error!(
                        service = stored,
                        %error,
                        "the services table holds an id this build cannot read; leaving the row \
                         alone"
                    );

                    continue;
                }
            };

            // The whole of what makes this safe on a running daemon.
            if held.contains(&service) {
                continue;
            }

            self.reconcile(&service, &record, declared.as_ref(), &mut recovery)
                .await;
        }

        recovery
    }

    /// What to do about one row the last daemon left behind. See [`Registry::recover`].
    async fn reconcile(
        &self,
        service: &ServiceId,
        record: &services::ServiceRecord,
        declared: Option<&ServiceGraph>,
        recovery: &mut Recovery,
    ) {
        let survivor = match survivor(service, record) {
            Ok(survivor) => survivor,

            // The OS has the answer and would not give it, which is neither "it is there" nor "it is
            // gone". Treated as gone, because the alternative is a row left claiming a supervisor
            // for ever — and because the only thing this branch forgoes is *adopting*: nothing is
            // signalled on a pid whose identity was never confirmed.
            Err(error) => {
                tracing::warn!(
                    service = service.as_str(),
                    error = %error,
                    "cannot tell whether this service's process outlived the daemon that started \
                     it; treating it as gone"
                );

                None
            }
        };

        let Some(adopted) = survivor else {
            if self
                .discard(service, record, None, StateReason::Vanished)
                .await
            {
                recovery.cleared.push(service.clone());
            }

            return;
        };

        let spec = declared.and_then(|graph| graph.spec(service));

        // Only a service that was *up* can be taken over as it is. The mid-flight states have a
        // process and no way to resume what was being done to it: a `starting` service was never
        // proved ready and cannot be re-checked without the pipes that went with the old daemon, and
        // a `stopping` one is halfway through a stop somebody asked for.
        let resumable = matches!(record.state, ServiceState::Running | ServiceState::Degraded);

        let stopped = match (spec, resumable) {
            (Some(spec), true) => {
                self.supervise(
                    spec,
                    &mut lock(&self.running),
                    Some(adopted),
                    StateReason::Requested,
                );
                recovery.adopted.push(service.clone());

                return;
            }

            // **Two different sentences, because they are two different people's problem.** A
            // service nothing declares is one somebody removed and the answer is that its process
            // goes with it; a daemon that could not be told what is declared has stopped a service
            // that may be perfectly well declared, and a row saying "nothing declares this" would
            // send its owner looking for a declaration that is there. The log line `recover` writes
            // says the same thing once for the whole boot; this is what `mix service list` shows
            // for each service afterwards.
            (None, _) => {
                let reason = if declared.is_some() {
                    "nothing declares this service any more, so nothing could supervise the \
                     process it left behind"
                } else {
                    "this daemon could not read which services are declared, so it had nothing to \
                     supervise the process it left behind against"
                };

                self.discard(
                    service,
                    record,
                    Some(adopted),
                    StateReason::Unadopted {
                        reason: reason.to_owned(),
                    },
                )
                .await
            }

            (Some(_), false) => {
                self.discard(
                    service,
                    record,
                    Some(adopted),
                    StateReason::Unadopted {
                        reason: format!(
                            "the daemon supervising it went away while it was {}, which is not a \
                             state another daemon can take over",
                            record.state
                        ),
                    },
                )
                .await
            }
        };

        if stopped {
            recovery.stopped.push(service.clone());
        } else {
            recovery.refused.push(service.clone());
        }
    }

    /// Let go of a service the last daemon left behind: stop whatever survived, clear the pid it
    /// named, and record where that leaves it. `false` if the survivor would not go.
    ///
    /// **In that order, and the order is the whole of the guarantee**: a row is only cleared once the
    /// process it named is no longer running, so a daemon killed in the middle of this leaves the next
    /// one exactly the case it already knows how to handle. The corollary is that a survivor which
    /// will not go leaves its row untouched — still claiming the pid, still in the state it was found
    /// in — rather than being written down as stopped while it holds the port. That is the same rule
    /// [`Runner::stop_adopted`](runner) follows, and it is why this can report a failure at all.
    ///
    /// [`runner`]: runner::Runner
    async fn discard(
        &self,
        service: &ServiceId,
        row: &services::ServiceRecord,
        survivor: Option<Adopted>,
        reason: StateReason,
    ) -> bool {
        if let Some(adopted) = survivor {
            tracing::info!(
                service = service.as_str(),
                pid = adopted.pid(),
                %reason,
                "stopping a process that outlived the daemon supervising it"
            );

            if let Err(error) = adopted.stop() {
                tracing::error!(
                    service = service.as_str(),
                    pid = adopted.pid(),
                    error = %error,
                    "cannot stop it; its row is left naming it, for the next daemon to try again"
                );

                return false;
            }

            if !gone(service, &adopted).await {
                tracing::error!(
                    service = service.as_str(),
                    pid = adopted.pid(),
                    "this process did not go when it was stopped; its row is left naming it, for \
                     the next daemon to try again"
                );

                return false;
            }
        }

        if row.pid.is_some()
            && let Err(error) = services::ended(&self.store, service, None).await
        {
            tracing::error!(
                service = service.as_str(),
                error = %error,
                "cannot clear the process this service's row names; the next daemon will meet the \
                 same pid again"
            );
        }

        if !row.state.is_supervised() {
            // A row that already says `stopped` or `failed` and merely held a stale pid. There is
            // no move to make and nothing to announce: it was where it says it is.
            return true;
        }

        // Through `Stopping`, which is the only edge into `Stopped` — and which the machine already
        // means: this is the last thing anybody did to the process. A row that was *already*
        // stopping is skipped rather than asked to move to where it is, which is not an event.
        if row.state != ServiceState::Stopping {
            record(
                &self.store,
                &self.events,
                service,
                ServiceState::Stopping,
                reason.clone(),
            )
            .await;
        }

        record(
            &self.store,
            &self.events,
            service,
            ServiceState::Stopped,
            reason,
        )
        .await;

        true
    }

    /// The store this registry writes through, for the activator's own read of a row — T70.
    pub(crate) fn store(&self) -> &Store {
        &self.store
    }

    /// What is being held on stopped services' behalf — roadmap task **T70a**.
    pub(crate) fn holder(&self) -> &Arc<hold::Holder> {
        &self.holder
    }

    /// The machine this registry supervises on.
    ///
    /// Handed out for the one caller that has to ask the OS a question of its own before a row
    /// exists: `service.create` allocates a port, and what makes a port free is the machine rather
    /// than the table — see [`mixengine_core::services::ports`]. Reached through here rather than
    /// through `mixengine_platform::host()` so that a test driving the API against a mock host is
    /// answered by that mock and not by whatever is listening on the runner.
    pub(crate) fn host(&self) -> &dyn Host {
        self.host.as_ref()
    }

    /// Whether this home answers a site with nothing behind it — roadmap task **T124**.
    ///
    /// Read by every caller that builds a generator of its own, so that the drift check and the
    /// install render the same thing.
    pub(crate) fn welcome(&self) -> bool {
        self.welcome
    }

    /// Which services have a task supervising them right now.
    ///
    /// **Not a second opinion about what a service is doing** — that is the row's, and this registry
    /// never writes one behind `core`'s back. It answers the other question, and since T18 the two
    /// only come apart within one run of the daemon: a row that says `running` with nothing in here
    /// used to be what a killed daemon left behind, and [`Registry::recover`] now reconciles those
    /// before the first client is served. What is left for this to show is a service whose runner
    /// ended without the row following it — which is a fault, and is what `service.list` makes
    /// visible instead of implying.
    pub(crate) fn supervised(&self) -> BTreeSet<ServiceId> {
        lock(&self.running)
            .iter()
            .filter(|(_, entry)| !entry.task.is_finished())
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// What a client reads on `GET /logs/{id}` — roadmap task **T16b**.
    ///
    /// A read-only handle for the endpoint, which is what the layering asks for: everything that
    /// *writes* a line into one of these is a runner this registry started, and a request handler
    /// only ever asks a service's log what it has and what comes next.
    pub(crate) fn logs(&self) -> &Logs {
        &self.logs
    }

    /// Whether a task is supervising this service right now.
    fn is_running(&self, id: &ServiceId) -> bool {
        lock(&self.running)
            .get(id)
            .is_some_and(|entry| !entry.task.is_finished())
    }
}

/// The process this row names, if it is still the one the row was written about.
///
/// **Both halves or nothing.** A row with a pid and no start time is one this build wrote when the
/// OS would not say when the process began, and there is no way to tell now whether the number still
/// means what it meant — so it is treated as gone, which costs an adoption and never a wrong signal.
///
/// # Errors
///
/// Whatever [`Adopted::identify`] could not ask the OS. Not "there is no such process", which is
/// [`None`].
fn survivor(
    service: &ServiceId,
    row: &services::ServiceRecord,
) -> mixengine_platform::Result<Option<Adopted>> {
    let (Some(pid), Some(started)) = (row.pid, row.pid_start_time) else {
        if row.pid.is_some() {
            tracing::warn!(
                service = service.as_str(),
                "this service's row names a process and not when it began, so it cannot be \
                 identified; treating it as gone"
            );
        }

        return Ok(None);
    };

    Adopted::identify(pid, StartTime::from_stored(started))
}

/// Which services a restart brings back up — roadmap task **T18**, shared since **T71a**.
///
/// **Everything that was supervised, and the service that was named whether it was or not.** A
/// stop plan reaches every dependent, including ones that were already down, and feeding the whole
/// plan back into a start would take that as a request to start them: restarting a database would
/// silently bring up every site that names it. What was down before is left down.
///
/// The service the caller *named* is the exception, and is restarted whether or not it was running:
/// `restart` on something stopped is a request for it to be running, the same reading `start` gives.
/// With nothing named every declared service is the named one, so `service.restart` with no target
/// stays what it says — restart everything.
///
/// Supervision rather than the row is the test for "was up", because it is the registry's own
/// answer to a question about the registry's own tasks: a service in its fourth restart backoff is
/// not running and is very much still one this daemon is bringing up.
pub(crate) fn restarted(
    named: Option<&ServiceId>,
    down: &Plan,
    supervised: &BTreeSet<ServiceId>,
) -> Vec<ServiceId> {
    down.flat()
        .filter(|id| named.is_none_or(|named| named == *id) || supervised.contains(*id))
        .cloned()
        .collect()
}

/// Take a set down and bring it back up — the whole of what a restart *does*.
///
/// **One implementation, called by `service.restart` and by the memory watchdog.** A second one
/// would be a second set of edge cases about dependents, drifting apart the day a recipe declares a
/// `depends_on` — and the two callers would then disagree about what a person's restart and an
/// automatic one each mean.
///
/// **When the stop fails, that is what comes back and the start never happens.** The one way it can
/// (T18) is a survivor this daemon adopted and could not kill — a process still holding the port and
/// the data directory — and starting the service again on top of it would put a second one there to
/// collide with the first. A restart that could not take the service down has not restarted it, and
/// says so.
pub(crate) async fn stop_then_start(
    registry: &Registry,
    graph: &ServiceGraph,
    down: &Plan,
    up: &Plan,
) -> Walk {
    // Reported against the *start* plan, which is what the caller announced — and the service that
    // would not stop is always in it, because it was supervised when `restarted` read the set.
    if let Some((refused, _)) = registry.stop(down).await.failed {
        return Walk {
            failed: Some((refused, Fault::unexplained())),
            ..Walk::default()
        };
    }

    registry.start(graph, up).await
}

/// Restart one service, on nobody's request but this daemon's — roadmap task **T71a**.
///
/// The plans a person's `mix service restart` would build for the same service, walked by the same
/// [`stop_then_start`]. [`None`] where a plan could not be built at all, which cannot happen through
/// a graph this id came out of and is reported rather than unwrapped: a panic in a background loop
/// would take the daemon with it.
pub(crate) async fn restart(
    registry: &Registry,
    graph: &ServiceGraph,
    id: &ServiceId,
) -> Option<Walk> {
    let down = graph.stop_plan([id]).ok()?;

    // Read before anything is stopped, because afterwards nothing is supervised and every service
    // would look like one that had been down all along.
    let roots = restarted(Some(id), &down, &registry.supervised());
    let up = graph.start_plan(roots.iter()).ok()?;

    let walk = stop_then_start(registry, graph, &down, &up).await;

    walk.failed.is_none().then_some(walk)
}

// Persist a state change and publish the value that was persisted. `false` if it did not land.
///
/// The one place a `services.state` write happens in this crate, which is what keeps the row and the
/// event from ever describing different events: what is published is the [`ServiceTransition`] the
/// transaction handed back, not a second description built beside it.
///
/// [`ServiceTransition`]: mixengine_proto::ServiceTransition
async fn record(
    store: &Store,
    events: &Events,
    service: &ServiceId,
    to: ServiceState,
    reason: StateReason,
) -> bool {
    record_noting(store, events, service, to, reason, None).await
}

/// [`record`], with the sentence that explains a move into `failed` — roadmap task **T200b**, D5.
async fn record_noting(
    store: &Store,
    events: &Events,
    service: &ServiceId,
    to: ServiceState,
    reason: StateReason,
    detail: Option<&str>,
) -> bool {
    match services::transition_noting(store, service, to, reason, detail, now()).await {
        Ok(change) => {
            events.publish(DaemonEvent::ServiceStateChanged(change));

            true
        }

        Err(error) => {
            tracing::error!(
                service = service.as_str(),
                to = %to,
                %error,
                "cannot record a state change for this service"
            );

            false
        }
    }
}

/// The daemon's clock, in the one shape everything below it takes.
fn now() -> Timestamp {
    Timestamp::from_system_time(SystemTime::now())
}

/// The running map, whether or not a task holding it panicked.
///
/// A poisoned lock here means a runner task died mid-tidy-up; the map itself is a `HashMap` of
/// handles and is no less valid for it, and refusing to supervise anything else would turn one
/// failed service into a daemon that cannot start another.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use mixengine_proto::{Backoff, Millis, RestartPolicy, StopBehaviour};
    use mixengine_testkit::FakeService;

    use super::fixture::{
        Declared, EVENTUALLY, SILENCE, Unavailable, arguments, gone, home, registry, registry_on,
        row, service, spec,
    };
    use super::*;

    /// A `fakeservice` running, and a row that says the *last* daemon started it — T18's subject.
    ///
    /// The row is written the way the runner writes one, through `core`, so what recovery meets is
    /// what a killed daemon really leaves: a supervised state, a pid, and the moment that process
    /// began. Nothing here is supervising it, which is the point.
    async fn left_running(
        store: &Store,
        id: &ServiceId,
        state: ServiceState,
    ) -> mixengine_testkit::service::Running {
        left_running_with(store, id, state, FakeService::new()).await
    }

    /// [`left_running`], for a test that needs the survivor to behave in a particular way.
    async fn left_running_with(
        store: &Store,
        id: &ServiceId,
        state: ServiceState,
        fake: FakeService,
    ) -> mixengine_testkit::service::Running {
        let service = fake.spawn();

        // Waited for, because a spawn returns before the process has parsed its arguments — and a
        // start time read in that window would be right for the wrong reason.
        assert!(
            service.wait_for_stdout(mixengine_testkit::service::READY_LINE, EVENTUALLY),
            "the survivor did not start"
        );

        let pid = service.id();
        let started = mixengine_platform::process::started_at(pid)
            .expect("this system can be asked when a process began")
            .expect("the survivor is running");

        services::transition(
            store,
            id,
            ServiceState::Starting,
            StateReason::Requested,
            now(),
        )
        .await
        .expect("a stopped service can start");

        if state != ServiceState::Starting {
            services::transition(store, id, state, StateReason::Ready, now())
                .await
                .expect("a starting service can reach the state this fixture asks for");
        }

        services::started(store, id, pid, Some(started.stored()), now())
            .await
            .expect("the row takes the process the last daemon started");

        service
    }

    /// A ceiling this machine will not hold, watched to the end of the count — task **T71a**.
    ///
    /// **The whole path against the real registry and a real process.** The only invented thing is
    /// what the machine reports about that process, which is the one thing a test cannot arrange for
    /// real: a service cannot be asked to grow to a size on cue. Everything else — the graph, the
    /// spec's ceiling, the transitions, the stop and the start — is what a daemon does.
    ///
    /// It asserts the three rules that could each fail silently: a warning arrives before any
    /// restart, the restart happens on the third minute and not the second, and a service that comes
    /// back still over the line is **not** restarted again.
    #[tokio::test]
    async fn a_service_over_a_ceiling_this_machine_cannot_hold_is_warned_about_and_restarted() {
        let (_home, paths, store) = home(&["caddy"]).await;

        let fake = FakeService::new();
        let declared = Declared(vec![
            spec("caddy")
                .args(arguments(&fake))
                .limits(mixengine_proto::ResourceLimits {
                    memory_mb: Some(64),
                    ..mixengine_proto::ResourceLimits::default()
                })
                .restart_over_memory(true)
                .build()
                .expect("a usable spec"),
        ]);

        // **A machine that watches rather than caps**, which is what arms the watchdog — and is
        // programmed rather than depended on, because the runner this lands on may be any of three.
        let mut host = mixengine_platform::mock::Host::with_home(paths.root());
        host.set_limit_support(mixengine_platform::LimitSupport {
            mechanism: mixengine_platform::LimitMechanism::None,
            cpu: mixengine_platform::Enforcement::Unsupported,
            memory: mixengine_platform::Enforcement::Advisory { why: None },
            memory_measure: mixengine_platform::MemoryMeasure::Resident,
            priority: true,
            cores: 4,
        });

        let registry = Arc::new(registry_on(
            &paths,
            &store,
            Arc::new(declared),
            Arc::new(host),
        ));

        let graph = registry.graph().await.expect("one declared service");
        let plan = graph.start_plan([&service("caddy")]).expect("a plan");
        assert!(registry.start(&graph, &plan).await.failed.is_none());

        let (state, first_pid) = row(&store, &service("caddy")).await;
        assert_eq!(state, ServiceState::Running);

        let mut watchdog = watchdog::Watchdog::new(Arc::clone(&registry), 3);

        // Two minutes over the line: a warning, and nothing else. The service is still the one that
        // was started.
        for _ in 0..2 {
            watchdog.minute(&[over_the_line()]).await;
        }

        assert!(
            settles_to(&store, ServiceState::Degraded).await,
            "two minutes over the line is a warning"
        );

        let (_, still) = row(&store, &service("caddy")).await;
        assert_eq!(still, first_pid, "a warning is not a restart");

        // The third finishes the count.
        watchdog.minute(&[over_the_line()]).await;

        assert!(
            settles_to(&store, ServiceState::Running).await,
            "the third minute restarts it, and it comes back up"
        );

        let (_, second_pid) = row(&store, &service("caddy")).await;
        assert_ne!(second_pid, first_pid, "a restart is a different process");

        // And it comes back still over the line, because the reading says so. One restart per
        // episode: this is a misconfigured ceiling, not a leak, and a daemon restarting it for ever
        // would be worse than a number nobody enforced.
        for _ in 0..6 {
            watchdog.minute(&[over_the_line()]).await;
        }

        let (_, third_pid) = row(&store, &service("caddy")).await;
        assert_eq!(
            third_pid, second_pid,
            "a service that comes back still over the line is left alone"
        );

        registry.stop(&graph.stop_order()).await;
    }

    /// One finished minute for `caddy`, comfortably over the 64 MB ceiling above.
    fn over_the_line() -> mixengine_proto::MetricsMinute {
        mixengine_proto::MetricsMinute {
            subject: mixengine_proto::MetricsSubject::Service(service("caddy")),
            minute: mixengine_proto::Timestamp(0),
            cpu_avg: None,
            cpu_peak: None,
            rss_avg: 128 * 1024 * 1024,
            rss_peak: 128 * 1024 * 1024,
            samples: 1,
        }
    }

    /// Wait for the row to reach `wanted`, or say it did not within [`EVENTUALLY`].
    ///
    /// **A poll rather than an assertion**, because a restart is a walk: the stop, the runner's
    /// tidy-up and the start each land in their own turn, and reading the row the instant the
    /// watchdog returns would read whichever of them got there first.
    async fn settles_to(store: &Store, wanted: ServiceState) -> bool {
        let deadline = tokio::time::Instant::now() + EVENTUALLY;

        while tokio::time::Instant::now() < deadline {
            if row(store, &service("caddy")).await.0 == wanted {
                return true;
            }

            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        false
    }

    /// **T18, and the case M1 is about.** A process that outlived the daemon which started it is
    /// supervised again by the one that finds it — not restarted, not left running with nothing
    /// watching it, and above all not reported as something it is not.
    ///
    /// The survivor here is this test's own child rather than one a daemon left behind, which is the
    /// only way to produce one on Windows at all: a supervised child there dies with its daemon by
    /// kernel guarantee (ADR 0007), so the process that reaches a real `recover` is the one from the
    /// window that ADR accepts. Nothing in the code under test can tell the difference — it is
    /// handed a row, and it asks the OS about the pid in it.
    #[tokio::test]
    async fn a_service_that_outlived_the_last_daemon_is_supervised_again() {
        let (_home, paths, store) = home(&["caddy"]).await;
        let declared = Declared(vec![spec("caddy").build().expect("a usable spec")]);
        let registry = registry(&paths, &store, Arc::new(declared));

        let mut survivor = left_running(&store, &service("caddy"), ServiceState::Running).await;

        // Subscribed before the reconciliation, because half of what is asserted here is a silence:
        // nothing happened to this service, so nothing may be announced about it.
        let mut watching = registry.events.subscribe();

        let recovered = registry.recover().await;

        assert_eq!(recovered.adopted, vec![service("caddy")], "{recovered:?}");
        assert!(recovered.stopped.is_empty(), "{recovered:?}");
        assert!(recovered.cleared.is_empty(), "{recovered:?}");

        assert!(
            registry.supervised().contains(&service("caddy")),
            "the adopted service has nothing supervising it"
        );
        assert_eq!(
            row(&store, &service("caddy")).await.0,
            ServiceState::Running,
            "an adopted service is where it was; adoption is not a start"
        );
        assert!(
            survivor.still_running(),
            "the process was stopped by the daemon that was supposed to take it over"
        );

        let announced = tokio::time::timeout(SILENCE, watching.next()).await;
        assert!(
            announced.is_err(),
            "adopting a service announced a state change, so a client was told a service somebody \
             has been using all along had just moved: {announced:?}"
        );

        // And the other half of being supervised again: a stop reaches it. This is the assertion
        // that separates adoption from a registry entry that merely looks right.
        let graph = registry.graph().await.expect("one declared service");
        let stopping = graph.stop_plan([&service("caddy")]).expect("a plan");
        tokio::time::timeout(EVENTUALLY, registry.stop(&stopping))
            .await
            .expect("the adopted service was stopped");

        let (state, pid) = row(&store, &service("caddy")).await;
        assert_eq!(state, ServiceState::Stopped);
        assert_eq!(pid, None);
        assert!(
            !survivor.still_running(),
            "the adopted process outlived the stop of the service it belongs to"
        );
    }

    /// **One credential the keyring cannot answer for does not take the rest of the environment
    /// with it.**
    ///
    /// The adopted path is the only one that resolves an environment at stop time — see
    /// `Runner::where_commands_run` — and it is the one where a keyring is most likely to refuse: a
    /// machine that has been rebooted since the daemon that started this process. Dropping the whole
    /// environment for that would run `mariadb-admin shutdown` without the `HOME` the spec declares
    /// outright, so the stop that was meant to survive a locked keyring would fail a second time for
    /// a reason nobody chose.
    ///
    /// The mock store holds no credential, which is the same answer a locked one eventually gives.
    #[tokio::test]
    async fn an_adopted_service_stops_with_the_environment_entries_that_did_resolve() {
        let (home, paths, store) = home(&["mariadb"]).await;

        let asked = home.path().join("shutdown-was-asked-for");
        let dumped = home.path().join("stop-command-environment");
        let shutdown = FakeService::new().dump_env(&dumped).touch(&asked);

        let declared = Declared(vec![
            spec("mariadb")
                .env("MIXENGINE_DECLARED", "outright")
                .env_from_keyring("MIXENGINE_SECRET", "mariadb", "root")
                .stop(StopBehaviour::Command {
                    program: FakeService::program(),
                    args: arguments(&shutdown),
                    grace: Millis::from_secs(10),
                })
                .build()
                .expect("a usable spec"),
        ]);
        let registry = registry(&paths, &store, Arc::new(declared));

        // Deaf to every signal, so the clean exit below is evidence the command ran and nothing
        // else.
        let mut survivor = left_running_with(
            &store,
            &service("mariadb"),
            ServiceState::Running,
            FakeService::new().ignoring_stop().exit_when(&asked),
        )
        .await;

        let recovered = registry.recover().await;
        assert_eq!(recovered.adopted, vec![service("mariadb")], "{recovered:?}");

        let graph = registry.graph().await.expect("one declared service");
        let stopping = graph.stop_plan([&service("mariadb")]).expect("a plan");
        let stopped = tokio::time::timeout(EVENTUALLY, registry.stop(&stopping))
            .await
            .expect("the adopted service was stopped");

        assert_eq!(stopped.reached, vec![service("mariadb")], "{stopped:?}");
        assert!(
            gone(&mut survivor).await,
            "the adopted process outlived the stop of the service it belongs to"
        );

        let environment =
            std::fs::read_to_string(&dumped).expect("the stop command recorded its environment");

        assert!(
            environment
                .lines()
                .any(|line| line == "MIXENGINE_DECLARED=outright"),
            "the entry the spec states outright was thrown away with the one the keyring refused, \
             so the stop command ran without the environment it needs:\n{environment}"
        );
        assert!(
            !environment.contains("MIXENGINE_SECRET"),
            "an entry that did not resolve was passed anyway, which for a credential means an \
             empty password:\n{environment}"
        );
    }

    /// The other half of M1: what did *not* survive is cleaned, and cleaning it signals nothing.
    ///
    /// **The pid in the row is this test process's own**, which is the strongest way to assert the
    /// second half: if the identity check were skipped — if a pid alone were taken for a service —
    /// this test would not fail, it would be killed. What makes the pair not match is a start time
    /// one tick from the real one, which is exactly what a recycled pid looks like from in here.
    #[tokio::test]
    async fn a_row_whose_process_did_not_survive_is_cleared_and_nothing_is_signalled() {
        let (_home, paths, store) = home(&["caddy"]).await;
        let declared = Declared(vec![spec("caddy").build().expect("a usable spec")]);
        let registry = registry(&paths, &store, Arc::new(declared));

        let ours = std::process::id();
        let mistaken = mixengine_platform::process::started_at(ours)
            .expect("this system can be asked when a process began")
            .expect("this process is running")
            .stored()
            + 1;

        services::transition(
            &store,
            &service("caddy"),
            ServiceState::Starting,
            StateReason::Requested,
            now(),
        )
        .await
        .expect("a stopped service can start");
        services::transition(
            &store,
            &service("caddy"),
            ServiceState::Running,
            StateReason::Ready,
            now(),
        )
        .await
        .expect("a starting service can be running");
        services::started(&store, &service("caddy"), ours, Some(mistaken), now())
            .await
            .expect("the row takes a process");

        let mut watching = registry.events.subscribe();

        let recovered = registry.recover().await;

        assert_eq!(recovered.cleared, vec![service("caddy")], "{recovered:?}");
        assert!(recovered.adopted.is_empty(), "{recovered:?}");

        let (state, pid) = row(&store, &service("caddy")).await;
        assert_eq!(state, ServiceState::Stopped);
        assert_eq!(
            pid, None,
            "a row that kept a pid would be adopted by the next daemon, and by then it is somebody \
             else's"
        );

        // Through `Stopping`, which is the only edge into `Stopped`, and with the reason a person
        // reading the service list needs to understand why a service they left running is not.
        let mut seen = Vec::new();
        while seen.len() < 2 {
            let frame = tokio::time::timeout(EVENTUALLY, watching.next())
                .await
                .expect("the stream is not silent")
                .expect("the stream is still open");

            if let crate::api::events::Frame::Event(DaemonEvent::ServiceStateChanged(change)) =
                frame
            {
                seen.push(change);
            }
        }

        assert_eq!(seen[0].to, ServiceState::Stopping);
        assert_eq!(seen[1].to, ServiceState::Stopped);
        assert_eq!(seen[1].reason, StateReason::Vanished);
    }

    /// A survivor nothing declares cannot be supervised, and is not left holding the port either.
    ///
    /// The state a service is left in is what a user is told, so it says which of the two things
    /// went wrong — the process was there, and this daemon had no declaration to run it against.
    #[tokio::test]
    async fn a_survivor_nothing_declares_any_more_is_stopped() {
        let (_home, paths, store) = home(&["caddy"]).await;
        let registry = registry(&paths, &store, Arc::new(Declared(Vec::new())));

        let mut survivor = left_running(&store, &service("caddy"), ServiceState::Running).await;

        let recovered = registry.recover().await;

        assert_eq!(recovered.stopped, vec![service("caddy")], "{recovered:?}");
        assert!(recovered.adopted.is_empty(), "{recovered:?}");

        assert!(
            gone(&mut survivor).await,
            "a service nothing declares was left running with nothing supervising it, which is the \
             process the next start collides with"
        );

        let (state, pid) = row(&store, &service("caddy")).await;
        assert_eq!(state, ServiceState::Stopped);
        assert_eq!(pid, None);
        assert!(
            registry.supervised().is_empty(),
            "nothing can be supervising a service that has no declaration"
        );
    }

    /// A daemon that cannot be *told* what is declared stops the same survivors — and must not tell
    /// their owner they were undeclared.
    ///
    /// The two look identical from inside the reconciliation and are opposite problems outside it:
    /// one service was removed and its process goes with it, the other is declared perfectly well by
    /// a source this daemon could not read. `mix service list` is where a person meets the
    /// difference, and a row saying "nothing declares this" would send them looking for a
    /// declaration that is sitting right there.
    #[tokio::test]
    async fn a_survivor_stopped_because_the_declarations_could_not_be_read_says_so() {
        let (_home, paths, store) = home(&["caddy"]).await;
        let registry = registry(&paths, &store, Arc::new(Unavailable));

        let mut survivor = left_running(&store, &service("caddy"), ServiceState::Running).await;

        let mut watching = registry.events.subscribe();

        let recovered = registry.recover().await;

        assert_eq!(recovered.stopped, vec![service("caddy")], "{recovered:?}");
        assert!(
            gone(&mut survivor).await,
            "a survivor was left running by a daemon that could not tell whether it was declared"
        );

        let stopped = loop {
            let frame = tokio::time::timeout(EVENTUALLY, watching.next())
                .await
                .expect("the stream is not silent")
                .expect("the stream is still open");

            if let crate::api::events::Frame::Event(DaemonEvent::ServiceStateChanged(change)) =
                frame
                && change.to == ServiceState::Stopped
            {
                break change;
            }
        };

        let StateReason::Unadopted { reason } = &stopped.reason else {
            panic!("a survivor this daemon stopped is not {}", stopped.reason);
        };

        assert!(
            reason.contains("could not read which services are declared"),
            "the reason does not say why this daemon had nothing to supervise the process \
             against: {reason}"
        );
        assert!(
            !reason.contains("nothing declares"),
            "a service whose declarations could not be read was reported as undeclared: {reason}"
        );
    }

    /// A reconciliation that could not finish is not a quiet boot.
    ///
    /// The one outcome that leaves the machine as it found it — a survivor that would not go, whose
    /// row still names it — has to reach the summary `mixengined` writes, or the line a person opens
    /// `daemon.log` for says nothing happened in exactly the boot where something did. Asserted on
    /// the value rather than through a process, because a process that survives `SIGKILL` and
    /// `TerminateProcess` is not something a test can arrange on three operating systems.
    #[test]
    fn a_survivor_that_would_not_stop_is_not_reported_as_nothing_to_do() {
        let refused = Recovery {
            refused: vec![service("caddy")],
            ..Recovery::default()
        };

        assert!(!refused.is_empty(), "{refused:?}");
        assert!(Recovery::default().is_empty());
    }

    /// A process is not enough: a service left mid-start cannot be resumed, so it is stopped.
    ///
    /// Its readiness was never decided and cannot be decided now — the ready check most specs use
    /// matches a log pattern, and the pipes it would read went with the daemon that died. Adopting
    /// it as though it were up would route traffic to a service nothing ever proved was listening.
    #[tokio::test]
    async fn a_service_left_mid_start_is_stopped_rather_than_taken_for_ready() {
        let (_home, paths, store) = home(&["caddy"]).await;
        let declared = Declared(vec![spec("caddy").build().expect("a usable spec")]);
        let registry = registry(&paths, &store, Arc::new(declared));

        let mut survivor = left_running(&store, &service("caddy"), ServiceState::Starting).await;

        let recovered = registry.recover().await;

        assert_eq!(recovered.stopped, vec![service("caddy")], "{recovered:?}");
        assert!(
            gone(&mut survivor).await,
            "a service that was still starting was adopted rather than stopped"
        );
        assert_eq!(
            row(&store, &service("caddy")).await.0,
            ServiceState::Stopped
        );
    }

    /// Adoption ends where an ordinary life begins: the moment the survivor exits, its policy has
    /// it, and what the policy starts is a child of *this* daemon — pipes, group and log capture
    /// restored.
    #[tokio::test]
    async fn an_adopted_service_that_ends_is_put_back_as_this_daemon_s_own() {
        let (_home, paths, store) = home(&["caddy"]).await;
        let declared = Declared(vec![
            spec("caddy")
                .restart(RestartPolicy::Always {
                    backoff: Backoff {
                        initial: Millis(50),
                        max: Millis(50),
                        ..Backoff::default()
                    },
                })
                .build()
                .expect("a usable spec"),
        ]);
        let registry = registry(&paths, &store, Arc::new(declared));

        // A survivor that is going to end by itself shortly after being adopted.
        let mut survivor = left_running_with(
            &store,
            &service("caddy"),
            ServiceState::Running,
            FakeService::new().exit_after(750),
        )
        .await;
        let first = survivor.id();

        registry.recover().await;

        // The row naming a *different* process is the whole assertion: the exit was noticed, the
        // policy was asked, and what it started was spawned here rather than adopted.
        let deadline = tokio::time::Instant::now() + EVENTUALLY;
        loop {
            let (state, pid) = row(&store, &service("caddy")).await;

            if state == ServiceState::Running && pid.is_some_and(|pid| pid != i64::from(first)) {
                break;
            }

            assert!(
                tokio::time::Instant::now() < deadline,
                "the adopted service was not put back by its policy: it is {state} with pid {pid:?}"
            );

            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        assert!(
            !survivor.still_running(),
            "the process that was adopted is somehow still running"
        );

        registry.shutdown.cancel();
        tokio::time::timeout(EVENTUALLY, registry.shut_down())
            .await
            .expect("every runner finished");
    }

    #[tokio::test]
    async fn every_state_change_is_published_as_it_is_persisted() {
        let (_home, paths, store) = home(&["caddy"]).await;
        let declared = Declared(vec![spec("caddy").build().expect("a usable spec")]);
        let registry = registry(&paths, &store, Arc::new(declared));

        let mut watching = registry.events.subscribe();

        let graph = registry.graph().await.expect("one declared service");
        let plan = graph.start_plan([&service("caddy")]).expect("a plan");
        registry.start(&graph, &plan).await;

        let mut seen = Vec::new();
        while seen.len() < 2 {
            let frame = tokio::time::timeout(EVENTUALLY, watching.next())
                .await
                .expect("the stream is not silent")
                .expect("the stream is still open");

            if let crate::api::events::Frame::Event(DaemonEvent::ServiceStateChanged(change)) =
                frame
            {
                seen.push(change);
            }
        }

        assert_eq!(seen[0].to, ServiceState::Starting);
        assert_eq!(seen[0].reason, StateReason::Requested);
        assert_eq!(seen[1].to, ServiceState::Running);
        assert_eq!(seen[1].reason, StateReason::Ready);
    }
}
