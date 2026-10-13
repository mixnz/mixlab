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
mod recovery;
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
use mixengine_proto::{DaemonEvent, ServiceId, ServiceState, StateReason, Timestamp};
use tokio::sync::{Notify, watch};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::api::Events;
use logs::Logs;
use runner::Readiness;
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

    use super::fixture::{Declared, EVENTUALLY, home, registry, service, spec};
    use super::*;

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
