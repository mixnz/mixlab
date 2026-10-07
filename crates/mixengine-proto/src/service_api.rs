//! What `service.*` answers, where [`crate::service`] is the vocabulary a spec is *written* in.
//!
//! The split is the same one [`crate::daemon`] draws for `daemon.*`: a [`ServiceSpec`] describes a
//! program to run and is the input to supervision, while these types describe what the daemon then
//! made of it and are the output. A client renders these and never sees a spec.
//!
//! **What is deliberately not here is a reason for the state a service is in.** `services` keeps
//! only the columns a restart has to recover, and no column holds the *why* of a transition — that
//! travels with [`crate::DaemonEvent::ServiceStateChanged`], which carries the
//! [`StateReason`] of every move as it happens. A summary read a minute later
//! can say `failed` and cannot say what failed; the one place a walk *can* say so is
//! [`ServiceFailure`], because there the daemon has just watched it happen.
//!
//! [`ServiceSpec`]: crate::ServiceSpec

use crate::{
    FrontEndServer, IdleExemption, IdlePolicy, IdleSource, LimitSupport, PackageVersion,
    ProjectRef, ResourceLimits, ServiceId, ServiceState, StateReason, Timestamp,
};

/// Which database's superuser credential to re-set — roadmap task **T127**.
///
/// **A type of its own rather than [`ServiceTarget`], and T125 is why.** That type's `service` is
/// optional and [`None`] means *every service this home declares* — which is how both clients came
/// to ask for the whole home when they meant one project. A repair asked for with no subject is that
/// mistake with worse consequences, so the subject is a required field and there is nothing left to
/// refuse at run time.
///
/// There is no `project` either, for the reason `service.stop` refuses one: a project's services
/// include the front end every *other* site is reached through, and none of them but a database
/// keeps a credential to re-set.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ResetCredential {
    /// The database to repair: `mariadb@main`, `postgres@shop`.
    pub service: ServiceId,

    /// Whether to answer when the repair has finished rather than when it has been accepted.
    ///
    /// [`ServiceTarget::wait`]'s meaning and its default: a GUI that is already subscribed to the
    /// events would rather draw the walk arriving than block a window on it.
    #[serde(default = "waits", skip_serializing_if = "is_waiting")]
    pub wait: bool,
}

/// Which services a call is about, and whether the caller waits for the answer to be true.
///
/// One params type for `service.start`, `service.stop` and `service.restart`, because the question
/// each asks is the same one: *which* service, and *how long may this take*.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ServiceTarget {
    /// The service to act on, or [`None`] for every declared service.
    ///
    /// Naming one does not mean acting on one: a plan is the transitive set, so starting `php-fpm`
    /// pulls in what it depends on and stopping `mariadb` pulls in what depends on *it*. What the
    /// walk covered comes back in [`ServiceWalk::planned`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<ServiceId>,

    /// Every service one project needs, instead of one service or all of them — roadmap task
    /// **T125**.
    ///
    /// **What a project needs is the daemon's answer and never a client's.** It is the services its
    /// sites declare — `site_service_links`, which is where `blueprint.apply` writes the database
    /// and the cache it ensured — plus the php-fpm pool a site names, plus the front end every one
    /// of those sites is reached through. A client working that set out for itself would be
    /// business logic in a client, and it is why both clients said *every service this home
    /// declares* until this task: the question had no answer to ask for.
    ///
    /// **`service.start` only.** `service.stop` and `service.restart` refuse it, because the set
    /// includes the front end: a project-scoped stop that takes down the web server every *other*
    /// site is reached through is a machine-wide outage wearing one project's name.
    ///
    /// Never sent together with [`ServiceTarget::service`] — two subjects in one call is a request
    /// nothing can carry out as asked, so it is refused rather than resolved by precedence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<ProjectRef>,

    /// Whether to answer when the walk has finished rather than when it has been accepted.
    ///
    /// **Defaults to true, and the exit code is why.** `mix service start db && mix …` is a sentence
    /// about the database being up; an answer sent before the walk would make it exit `0` for a
    /// service that never came up, and the only way for a client to know better would be to re-derive
    /// the verdict from the event stream — the business-logic-in-a-client bug `CLAUDE.md` forbids.
    ///
    /// A GUI sends `false`: it is already subscribed to
    /// [`ServiceStateChanged`](crate::DaemonEvent::ServiceStateChanged) and would rather draw each
    /// service arriving than block a window on the slowest one. What comes back then is the plan and
    /// [`ServiceWalk::complete`] is `false` — the walk itself carries on inside the daemon.
    #[serde(default = "waits", skip_serializing_if = "is_waiting")]
    pub wait: bool,
}

impl Default for ServiceTarget {
    fn default() -> Self {
        Self {
            service: None,
            project: None,
            wait: waits(),
        }
    }
}

/// The `wait` a client that says nothing gets. See [`ServiceTarget::wait`].
fn waits() -> bool {
    true
}

/// Serialising the default is noise on the wire, and this is the one field with a non-`None` one.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "the signature serde's `skip_serializing_if` requires"
)]
fn is_waiting(wait: &bool) -> bool {
    *wait
}

/// Which service `service.status` is about.
///
/// Its own type rather than a [`ServiceTarget`] with the option filled in, because the id is
/// **required** here: a status with no subject is a `service.list` that was typed wrongly, and
/// answering it as one would hide the mistake instead of reporting `invalid_params`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ServiceQuery {
    /// The service to describe.
    pub service: ServiceId,
}

/// Which service `service.delete` is about, and whether a site declaring it may be crossed.
///
/// Flattened over [`ServiceQuery`] on [`crate::RuntimeUninstall`]'s precedent: the same wire shape
/// every client has sent since T31a, plus one optional key an older client's request parses without.
///
/// **`force` crosses a declaration and never a process** (spec D4). A site naming this service is a
/// statement about the next `site.start`, which somebody shown the sites is entitled to overrule; a
/// running service is a fact about now, and no flag buys it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ServiceDelete {
    /// The service to delete.
    #[serde(flatten)]
    pub target: ServiceQuery,

    /// Delete it even though a site declares it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub force: bool,
}

/// What `service.list` answers.
///
/// An object around the list rather than a bare array, on [`crate::DaemonStatus`]'s precedent: a
/// field can be added beside it — a count, a note about where the declarations came from — without
/// changing the shape of every existing client's parser.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ServiceList {
    /// Every declared service, in [`ServiceId`] order.
    pub services: Vec<ServiceSummary>,
}

/// One service, as the daemon currently sees it. Also the whole of what `service.status` answers.
///
/// Why a service last failed — roadmap task **T200b**, its design's D5.
///
/// `detail` is always a sentence: the error the supervisor had when it gave up, or the reason's
/// own words. That is what lets a client show it without a second table of what each
/// [`StateReason`] means.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ServiceFailureNote {
    /// When it failed.
    pub at: Timestamp,

    /// The reason the transition recorded.
    pub reason: StateReason,

    /// What a person reads.
    pub detail: String,
}

/// One type for the list and for the single lookup on purpose: they are the same sentence about a
/// service, so a client renders them with one function and a field added here reaches both.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ServiceSummary {
    /// Which service this is.
    pub id: ServiceId,

    /// What the `services` row says it is doing.
    ///
    /// **[`None`] means declared but never created**: something describes how to run this service
    /// and the daemon has no row for it, so it has no state to be in and cannot be started until one
    /// exists. Not a case a finished MixEngine reaches — from T30 onwards a declaration is *rendered
    /// from* a row — which is exactly why it is reported rather than smoothed over into `stopped`: a
    /// service that quietly claims to be stopped and then refuses to start explains nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<ServiceState>,

    /// Whether a task in this daemon is supervising it right now.
    ///
    /// Not a second opinion about [`ServiceSummary::state`] — the row is the only thing that says
    /// what a service is doing — but the answer to a different question: a row saying `running` with
    /// nothing supervising it is what a daemon that was killed leaves behind, and adopting or
    /// clearing those is T18. Until then this is where that gap is visible instead of implied.
    pub supervised: bool,

    /// The process it is running as, where there is one.
    pub pid: Option<u32>,

    /// The port its row was given, and [`None`] for a service that listens on none.
    ///
    /// **The number in the row, not one derived from the running process** — roadmap task
    /// **T34c**. It is allocated once, when the service is created, and never recomputed: it is in
    /// somebody's `.env` and in a colleague's shell history by the end of the afternoon, so a
    /// service that quietly moved between restarts would break both. A stopped service has it just
    /// as a running one does, which is the point of reporting it here rather than only at creation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,

    /// When it was last started, whether or not it is still running.
    pub last_started_at: Option<Timestamp>,

    /// What its process exited with the last time one ended. [`None`] where the OS reported none —
    /// a Unix process killed by a signal — for the same reason [`StateReason::Exited`] carries an
    /// option there.
    pub last_exit_code: Option<i32>,

    /// What it declares it needs, as the graph holds it: each dependency once, in [`ServiceId`]
    /// order. Empty for a service that depends on nothing.
    pub depends_on: Vec<ServiceId>,

    /// What this service is *for*, where two packages can be for the same thing — roadmap task
    /// **T97**.
    ///
    /// **The one thing a client could previously do nothing about.** Which program a home's sites
    /// are reached through is a fact the daemon holds and never published, so a client wanting to
    /// draw it had to hardcode that the package names `caddy` and `nginx` mean "front end" — a copy,
    /// in the client, of a table the daemon compiles in. This is that table answering.
    ///
    /// **A wire fact and not a domain one**, in
    /// [ADR 0019](https://github.com/mixnz/mixlab/blob/master/docs/decisions/0019-an-added-response-member-is-optional.md)'s
    /// sense: [`None`] means *this daemon was built before the member existed* and never *the role
    /// could not be determined*. A row whose package this build has no recipe for is
    /// [`ServiceRole::Other`], decided rather than absent — which is the same answer the refusal
    /// behind this already gives such a row when it passes over it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<ServiceRole>,

    /// Whether this service starts when the daemon does — roadmap task **T112**.
    ///
    /// **The setting and not a prediction**: it says what the column holds, not whether the service
    /// will in fact be running after the next login, which depends on a port being free and a
    /// program still being there.
    ///
    /// `false` for a service that is declared and has no row, on the same rule `state` and `port`
    /// follow: a service with no row has no setting to report, exactly as it has no state.
    ///
    /// **Defaulted**, on [ADR 0020]'s rule that the published contract is the shape the daemon
    /// writes: a client older than T112 keeps parsing what a newer daemon says, and one newer than
    /// a daemon that predates this member reads `false` — which is what every home had before
    /// anybody could set it.
    ///
    /// [ADR 0020]: https://github.com/mixnz/mixlab/blob/master/docs/decisions/0020-the-published-contract-is-the-shape-the-daemon-writes.md
    #[serde(default)]
    pub autostart: bool,

    /// Who left it stopped, while it is — roadmap task **T167d**.
    ///
    /// **Absent while the service is not `stopped`**, and from a daemon older than this member
    /// (ADR 0019). It is what lets a client tell a service MixEngine put to rest, which the next
    /// request wakes, from one a person stopped — which stays stopped — and draw the first as
    /// resting rather than as a failure ([ADR 0041]).
    ///
    /// [ADR 0041]: https://github.com/mixnz/mixlab/blob/master/docs/decisions/0041-mixengine-stops-nothing-a-person-did-not-ask-it-to.md
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped_by: Option<StoppedBy>,

    /// Which version of its program this service runs — roadmap task **T183**.
    ///
    /// **What a person reads next to the id**, because the id is a name they chose and says
    /// nothing about the program: `mysql@main` is whichever MySQL was installed when it was created.
    /// The full version as installed; a client that wants the `5.7` line drops the rest itself.
    ///
    /// [`None`] is drawn as nothing, whichever of its causes it has: a daemon from before this
    /// member (ADR 0019), a declared service with no row, an extension's service, or a stored text
    /// this build cannot parse.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<PackageVersion>,

    /// Why it failed, while it is `failed` — roadmap task **T200b**, D5.
    ///
    /// On `stopped_by`'s rule: the row keeps the note after a person stops a failed service, and no
    /// listing shows a failure the state no longer claims. Optional on the wire (ADR 0019).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_failure: Option<ServiceFailureNote>,
}

/// Who left a service stopped — the wire half of `mixengine-core`'s `StoppedBy`, and of the
/// `services.stopped_by` column it reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[non_exhaustive]
pub enum StoppedBy {
    /// Nobody: the service has not run since it was created or since the machine started.
    Never,

    /// A person, through `service.stop` or `service.restart`. It stays stopped until asked.
    Person,

    /// MixEngine itself — an idle sweep, its own shutdown, a process that vanished. The next request
    /// that needs it starts it again.
    Daemon,
}

/// Whether this home stops services nobody is using — `service.save_resources` answers it, and
/// `service.set_save_resources` answers it after changing it. Roadmap task **T167b**.
///
/// **Off unless a person turned it on** ([ADR 0041]). While it is off, a service whose own idle
/// setting is unset is never stopped for being idle; a service somebody gave a number keeps it.
///
/// [ADR 0041]: https://github.com/mixnz/mixlab/blob/master/docs/decisions/0041-mixengine-stops-nothing-a-person-did-not-ask-it-to.md
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SaveResources {
    /// Whether the switch is on.
    pub on: bool,
}

/// What `service.set_save_resources` takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SaveResourcesSet {
    /// What it should be.
    pub on: bool,
}

/// What `service.set_autostart` takes: one service, and whether it starts with the daemon.
///
/// **A `bool` and not an `Option<bool>`.** [`ServiceIdleSet`] has three states because an absent
/// value restores the recipe's own default; no recipe declares an autostart, so there is no third
/// state for one to restore and an option would be a case every client had to decide about for
/// nothing.
///
/// Roadmap task **T112**.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ServiceAutostartSet {
    /// The service to change.
    pub service: ServiceId,

    /// What it should be.
    pub autostart: bool,
}

/// What a service is *for*, where two packages can be for the same thing — roadmap task **T97**.
///
/// The wire half of `mixengine-core`'s `Role`, which is what a recipe answers and what
/// `service.create` refuses a second front end by (**T37**).
///
/// **[`FrontEnd`](Self::FrontEnd) carries which of the two it is, and that is the whole point.**
/// Without the payload a client that wanted to draw a two-way switch would be back where it started:
/// it would know that `caddy` is a front end and have no way to name the other one but by writing
/// the string. With it, the value read off the active row is the value
/// [`FrontEndSwitch::server`] takes, and
/// [ADR 0026](https://github.com/mixnz/mixlab/blob/master/docs/decisions/0026-the-active-front-end-is-a-row-and-switching-it-is-a-job.md)'s
/// *no client may map a package name to a role* is something a client can obey.
///
/// Only the one distinction, because only one exists: every other recipe is a server a home may run
/// beside any of the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum ServiceRole {
    /// The program every site on this machine is reached through. Exactly one at a time.
    FrontEnd {
        /// Which of them, as the value a switch is asked for by.
        server: FrontEndServer,
    },

    /// Everything else: a database, a cache, a pool.
    ///
    /// `Other {}` and not `Other`, for the reason `PrivilegedOp::Probe` is written that way: serde
    /// reads a *unit* variant of an internally tagged enum through `deserialize_any`, which takes
    /// the map and drops every key but the tag. An empty struct variant is read as a struct.
    Other {},
}

/// What a `service.start`, `service.stop` or `service.restart` did.
///
/// **Not a success or a failure**, which is why it is a struct rather than an error: a plan of six
/// services where the fourth fails leaves three running, one failed and two never tried, and a
/// client that has to render that needs all three lists. The failure is still a failure — `mix`
/// exits non-zero on one — but it is a described failure and not a lost walk.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ServiceWalk {
    /// Everything the plan covered, in the order it was walked.
    ///
    /// This is the transitive set and not what the caller named: it is what a client watches the
    /// event stream for, and the only way to know that starting one service is about to touch four.
    pub planned: Vec<ServiceId>,

    /// Whether the rest of this answer describes a finished walk.
    ///
    /// `false` for a call that asked not to wait ([`ServiceTarget::wait`]), where the plan has been
    /// accepted and is being walked behind this answer — the three lists below are then empty
    /// because nothing has happened yet, not because nothing happened.
    pub complete: bool,

    /// Services that reached what the walk was aiming for, in the order they got there.
    pub reached: Vec<ServiceId>,

    /// The service that stopped the walk, where one did.
    ///
    /// Always [`None`] for a stop: a service that is not running is already where the caller wants
    /// it, so there is no state a stop fails to reach.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed: Option<ServiceFailure>,

    /// Services never tried, because something they depend on failed.
    ///
    /// Each was recorded as `failed` with
    /// [`DependencyFailed`](StateReason::DependencyFailed) naming the direct edge it declared, so a
    /// chain of four reads as four sentences leading to the one service to fix.
    pub blocked: Vec<ServiceId>,
}

/// What `service.create` takes: which service, from which version of its package.
///
/// **The package is not a field**, because [`ServiceId::name`] already is one: it documents itself
/// as "the part before `@` — the package this is an instance of", so a second parameter would either
/// repeat the id or be a pair somebody has to police for agreement. The invariant is better held by
/// construction than by a check.
///
/// **The version is required**, on [`RuntimeTarget`](crate::RuntimeTarget)'s reasoning: choosing a
/// version for somebody is a decision, and there is no `service.resolve` to make it.
///
/// Everything else is optional because the column behind it is nullable and null already means
/// something: a service with no port is one whose recipe renders no port line, and a service with no
/// data directory is one the generator places under `data/<package>` itself.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ServiceCreate {
    /// Which service to create, and — through its name — which package it is an instance of.
    pub id: ServiceId,

    /// Which installed version of that package to run.
    pub version: PackageVersion,

    /// The port it listens on, or [`None`] for the recipe's own default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,

    /// The address it binds, or [`None`] for `127.0.0.1`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bind_addr: Option<String>,

    /// Where its data lives, or [`None`] for the home's own layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_dir: Option<String>,

    /// Whether it starts with the daemon. [`None`] is the column's default, which is no.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autostart: Option<bool>,

    /// The settings this instance overrides, checked against the recipe before the row is kept.
    ///
    /// A map rather than a [`String`] of JSON: the column holds a document, and a client that had to
    /// serialise one itself would be a client that could send something that is not an object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overrides: Option<serde_json::Map<String, serde_json::Value>>,
}

/// Why a service is not on the port its recipe would have preferred — roadmap task **T34c**.
///
/// **A port a person did not pick is one they have to be told about.** A recipe names the number
/// its product is documented under — 3306 for both databases, 6379 for Redis — and the first row to
/// ask for it is given it; every later asker is given the first free port above. So the sentence
/// this carries is either "the number you know is in use by `mysqld.exe`" or "another MixEngine
/// service already has it", and both of them are things a developer whose `.env` says 3306 needs to
/// read at the moment of creation rather than to discover from a connection that is refused.
///
/// The two identifying fields are separate rather than one rendered string for
/// [`StateReason::PortInUse`]'s reason, and are empty in the case
/// that variant cannot have: a port lost to another *MixEngine* service is held by a process the
/// daemon knows about and may not be running at all.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PortMoved {
    /// The port the recipe asked for, and did not get.
    pub preferred: u16,

    /// The process holding it, where this account may learn it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,

    /// The file name of that process's program, where this account may read it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
}

/// What `service.create` answers: the service, and the port story where there is one.
///
/// **Its own type rather than a [`ServiceSummary`]** — roadmap task **T34c** — because a create
/// answers two different questions and only one of them is true forever. What the service *is*
/// outlives the call and is a [`ServiceSummary`]; why it is not on the port its recipe prefers is
/// true only of this moment, and a field for it on every listing would be a sentence about a
/// decision nobody is making any more.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ServiceCreation {
    /// The service that now exists, as `service.status` would answer it.
    pub service: ServiceSummary,

    /// Why its port is not the one its recipe asked for, when it is not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moved_from: Option<PortMoved>,
}

/// What `service.delete` answers: what went, and what deliberately did not.
///
/// **A delete removes a row and a configuration directory, and never a data directory.** Generated
/// config is disposable and can be rendered again from the row; a data directory is somebody's
/// databases, and there is no undo behind a local development tool. So the path is *named* rather
/// than removed, because a directory nobody was told about is a directory nobody ever cleans up.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ServiceRemoval {
    /// The service as it stood before its row went, which is the only moment anything could describe
    /// it.
    pub removed: ServiceSummary,

    /// The data directory left in place, when there was one on disk to leave.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_kept: Option<String>,
}

/// The service a walk stopped at, and what was persisted about why.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ServiceFailure {
    /// Which service.
    pub service: ServiceId,

    /// What was written about it, which is the same value the matching
    /// [`ServiceStateChanged`](crate::DaemonEvent::ServiceStateChanged) carried.
    ///
    /// [`None`] when the failure was the daemon's own — a database that would not take the write, a
    /// supervising task that panicked. That is in `daemon.log` and is not a state a client could
    /// render; a client meeting one says so rather than inventing a reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<StateReason>,
}

/// One service data directory an earlier home left under `data/`, with no service row — roadmap
/// task **T182g**. The answer to `service.found` is a list of these.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ServiceFound {
    /// The service it would become: what `service.adopt` takes back.
    pub service: ServiceId,

    /// The directory.
    pub path: String,

    /// The server version that bootstrapped it, where its first run finished.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub made_by: Option<PackageVersion>,

    /// The installed version `service.adopt` would run it with. Absent when it cannot be adopted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opens_with: Option<PackageVersion>,

    /// Why it cannot be adopted yet, as a sentence with what to do. Absent when it can.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why_not: Option<String>,
}

/// What `service.found` answers — roadmap task **T182g**.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ServiceFoundList {
    /// Every data directory with no service row, in the order the directory lists them.
    pub found: Vec<ServiceFound>,
}

/// What `service.adopt` takes — roadmap task **T182g**.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ServiceAdopt {
    /// A service `service.found` listed.
    pub service: ServiceId,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service(id: &str) -> ServiceId {
        ServiceId::parse(id).expect("a valid service id")
    }

    /// **T182g.** A found instance that can be adopted says with what, and leaves out the two
    /// fields that would only be there when it cannot.
    #[test]
    fn a_found_service_that_opens_carries_only_what_opens_it() {
        let found = ServiceFound {
            service: service("mariadb@main"),
            path: "/data/mariadb/main".to_owned(),
            made_by: None,
            opens_with: Some(PackageVersion::parse("11.4.5").expect("a version")),
            why_not: None,
        };

        let json = serde_json::to_value(&found).expect("encodes");
        assert_eq!(json["service"], "mariadb@main");
        assert_eq!(json["opens_with"], "11.4.5");
        assert!(
            json.get("made_by").is_none() && json.get("why_not").is_none(),
            "{json}"
        );

        let back: ServiceFound = serde_json::from_value(json).expect("decodes");
        assert_eq!(back, found);
    }

    /// Both answers travel tagged, and the front-end one carries which program it is — which is
    /// what a client sends back to switch.
    #[test]
    fn a_role_says_what_a_service_is_for_and_names_the_program_when_it_matters() {
        let front_end = ServiceRole::FrontEnd {
            server: FrontEndServer::Nginx,
        };

        let encoded = serde_json::to_value(front_end).expect("a role encodes");
        assert_eq!(encoded["role"], "front_end");
        assert_eq!(encoded["server"], "nginx");
        assert_eq!(
            serde_json::from_value::<ServiceRole>(encoded).expect("and decodes"),
            front_end
        );

        let other = serde_json::to_value(ServiceRole::Other {}).expect("a role encodes");
        assert_eq!(other["role"], "other");
        assert_eq!(
            serde_json::from_value::<ServiceRole>(other).expect("and decodes"),
            ServiceRole::Other {}
        );
    }

    /// **The floor of protocol 1**, as the JSON a daemon from before **T97** actually sent — and
    /// the guard on [ADR 0019] that keeps the next added member optional too.
    ///
    /// Every member below is one this type was frozen with. [`ServiceSummary::role`] is absent,
    /// which is what such a daemon puts on the wire, and this build reads it as [`None`] rather
    /// than refusing the whole answer — which is the state a self-updating product spends every
    /// upgrade in, its binaries replaced and its daemon not yet restarted.
    ///
    /// [ADR 0019]: https://github.com/mixnz/mixlab/blob/master/docs/decisions/0019-an-added-response-member-is-optional.md
    #[test]
    fn a_summary_from_before_a_role_existed_still_reads() {
        let floor = r#"{
            "id": "mariadb@main",
            "state": "stopped",
            "supervised": false,
            "pid": null,
            "last_started_at": null,
            "last_exit_code": null,
            "depends_on": []
        }"#;

        let decoded: ServiceSummary = serde_json::from_str(floor).expect("a summary");

        assert_eq!(decoded.id.as_str(), "mariadb@main");
        assert_eq!(
            decoded.role, None,
            "an absent role is a daemon that predates the member, never a role nobody established"
        );
        assert_eq!(
            decoded.version, None,
            "an absent version is a daemon that predates T183"
        );
    }

    /// **T183.** A version goes out as the plain string it was installed as, and comes back equal.
    #[test]
    fn a_summary_carries_the_version_it_runs_and_omits_one_it_does_not_know() {
        let known = ServiceSummary {
            version: Some(PackageVersion::parse("5.7.44").expect("a version")),
            ..summary("mysql@main")
        };
        let encoded = serde_json::to_value(&known).unwrap();
        assert_eq!(encoded["version"], "5.7.44");
        assert_eq!(
            serde_json::from_value::<ServiceSummary>(encoded).unwrap(),
            known
        );

        let unknown = serde_json::to_value(summary("mysql@main")).unwrap();
        assert!(unknown.get("version").is_none(), "{unknown}");
    }

    /// **D4.** The shape every client has sent since T31a still parses, and gains one optional key.
    #[test]
    fn a_service_delete_from_before_force_existed_still_parses() {
        let asked: ServiceDelete =
            serde_json::from_value(serde_json::json!({"service": "mariadb@main"}))
                .expect("the old shape");

        assert!(!asked.force);
        assert_eq!(asked.target.service.as_str(), "mariadb@main");
    }

    /// One service, with nothing interesting about it.
    fn summary(id: &str) -> ServiceSummary {
        ServiceSummary {
            id: service(id),
            state: Some(ServiceState::Stopped),
            supervised: false,
            pid: None,
            port: None,
            last_started_at: None,
            last_exit_code: None,
            depends_on: Vec::new(),
            role: Some(ServiceRole::Other {}),
            autostart: false,
            stopped_by: None,
            version: None,
            last_failure: None,
        }
    }

    /// A create names the service and the version, and derives the package from the id — which is
    /// what [`ServiceId::name`] has always said it is.
    #[test]
    fn a_create_takes_an_id_and_a_version_and_nothing_redundant() {
        let create: ServiceCreate =
            serde_json::from_str(r#"{"id":"mariadb@main","version":"11.4.2"}"#)
                .expect("the two required fields");

        assert_eq!(create.id.name(), "mariadb");
        assert_eq!(create.id.instance(), Some("main"));
        assert_eq!(create.version.as_str(), "11.4.2");
        assert_eq!(
            create.port, None,
            "a port nobody named is the recipe's own default"
        );
    }

    /// A delete says what it kept, because what it kept is somebody's databases.
    #[test]
    fn a_removal_names_the_data_it_did_not_touch() {
        let removal = ServiceRemoval {
            removed: summary("mariadb@main"),
            data_kept: Some("/home/me/.local/share/mixengine/data/mariadb/main".to_owned()),
        };

        let encoded = serde_json::to_value(&removal).expect("a removal encodes");

        assert_eq!(
            encoded["data_kept"],
            "/home/me/.local/share/mixengine/data/mariadb/main"
        );
        assert_eq!(
            serde_json::from_value::<ServiceRemoval>(encoded).expect("and decodes"),
            removal
        );
    }

    /// The shape a client that types nothing gets: everything, and wait for it.
    #[test]
    fn a_target_with_no_parameters_means_every_service_and_waiting() {
        let target: ServiceTarget = serde_json::from_str("{}").expect("every field has a default");

        assert_eq!(target, ServiceTarget::default());
        assert_eq!(target.service, None);
        assert!(target.wait, "a client that said nothing is waited for");
    }

    #[test]
    fn not_waiting_is_the_one_thing_a_target_has_to_spell_out() {
        let target: ServiceTarget =
            serde_json::from_str(r#"{"service":"mariadb@main","wait":false}"#)
                .expect("both fields");

        assert_eq!(target.service, Some(service("mariadb@main")));
        assert!(!target.wait);

        // The default is not written back out: `{"service":"…"}` and `{"service":"…","wait":true}`
        // are the same request, and only one of them is worth putting on the wire.
        assert_eq!(
            serde_json::to_string(&ServiceTarget {
                service: Some(service("mariadb@main")),
                project: None,
                wait: true,
            })
            .unwrap(),
            r#"{"service":"mariadb@main"}"#
        );
    }

    /// **T97.** The only thing a Settings screen has to send is which program it wants.
    #[test]
    fn a_switch_needs_only_the_server_and_raises_nothing_by_itself() {
        let switch: FrontEndSwitch =
            serde_json::from_str(r#"{"server":"nginx"}"#).expect("the one required field");

        assert_eq!(switch.server, FrontEndServer::Nginx);
        assert_eq!(
            switch.version, None,
            "a version nobody named is the newest installed"
        );
        assert!(
            !switch.grant,
            "what is about to be allowed is read before it is allowed"
        );
    }

    /// A misspelled member must not be read as *do not raise a prompt*, which is what a switch with
    /// an unknown key and a permissive parser would silently be.
    #[test]
    fn a_switch_refuses_a_member_it_does_not_know() {
        serde_json::from_str::<FrontEndSwitch>(r#"{"server":"caddy","grants":true}"#)
            .expect_err("deny_unknown_fields");
    }

    /// Each ending travels tagged, and the two that carry nothing are still objects — the shape a
    /// client switches on.
    #[test]
    fn every_ending_a_switch_can_have_round_trips_tagged() {
        for (outcome, tag) in [
            (FrontEndOutcome::Unchanged {}, "unchanged"),
            (FrontEndOutcome::Switched { started: None }, "switched"),
            (
                FrontEndOutcome::NotGranted {
                    because: "nobody answered the prompt".to_owned(),
                },
                "not_granted",
            ),
            (
                FrontEndOutcome::RolledBack {
                    because: "nginx -t refused the rendering".to_owned(),
                },
                "rolled_back",
            ),
            (
                FrontEndOutcome::Failed {
                    because: "the old row could not be written back".to_owned(),
                },
                "failed",
            ),
        ] {
            let encoded = serde_json::to_value(&outcome).expect("an outcome encodes");
            assert_eq!(encoded["outcome"], tag);
            assert_eq!(
                serde_json::from_value::<FrontEndOutcome>(encoded).expect("and decodes"),
                outcome
            );
        }
    }

    /// The exit code, and the one ending that is deliberately not a failure: asking for the server a
    /// home is already on is a question with an answer.
    #[test]
    fn only_a_switch_that_did_not_happen_is_something_the_caller_wanted_more_of() {
        let report = |outcome| FrontEndReport {
            was: Some(service("caddy")),
            now: Some(service("caddy")),
            outcome,
            answering: true,
            kept_data: None,
            not_carried: Vec::new(),
        };

        assert!(!report(FrontEndOutcome::Unchanged {}).wanted_more());
        assert!(!report(FrontEndOutcome::Switched { started: None }).wanted_more());

        for refused in [
            FrontEndOutcome::NotGranted {
                because: "no helper".to_owned(),
            },
            FrontEndOutcome::RolledBack {
                because: "it would not render".to_owned(),
            },
            FrontEndOutcome::Failed {
                because: "and would not go back".to_owned(),
            },
        ] {
            assert!(report(refused).wanted_more());
        }

        assert!(
            report(FrontEndOutcome::Switched { started: None }).moved(),
            "and only a switch moved the home"
        );
        assert!(!report(FrontEndOutcome::Unchanged {}).moved());
    }

    /// What a switch left behind is on the wire even when there is nothing to say, because an empty
    /// list and an absent one are different answers — and a client renders the first as *nothing
    /// was lost*.
    #[test]
    fn a_report_that_left_nothing_behind_still_says_so() {
        let report = FrontEndReport {
            was: None,
            now: Some(service("nginx")),
            outcome: FrontEndOutcome::Switched { started: None },
            answering: false,
            kept_data: None,
            not_carried: Vec::new(),
        };

        let encoded = serde_json::to_value(&report).expect("a report encodes");
        assert!(encoded.get("was").is_none(), "{encoded}");
        assert!(encoded.get("kept_data").is_none(), "{encoded}");
        assert_eq!(encoded["not_carried"], serde_json::json!([]));
        assert_eq!(
            serde_json::from_value::<FrontEndReport>(encoded).expect("and decodes"),
            report
        );
    }

    #[test]
    fn a_status_without_a_service_does_not_decode() {
        serde_json::from_str::<ServiceQuery>("{}")
            .expect_err("a status with no subject is a mistyped list, not a list");
    }

    /// T167: who stopped a service travels as one word, and a daemon older than the member — or a
    /// service that is not stopped — sends nothing.
    #[test]
    fn stopped_by_is_a_word_and_is_absent_when_not_said() {
        assert_eq!(serde_json::to_value(StoppedBy::Daemon).unwrap(), "daemon");
        assert_eq!(serde_json::to_value(StoppedBy::Person).unwrap(), "person");
        assert_eq!(serde_json::to_value(StoppedBy::Never).unwrap(), "never");

        let older = serde_json::json!({
            "id": "caddy", "state": "stopped", "supervised": false, "depends_on": []
        });
        let summary: ServiceSummary = serde_json::from_value(older).expect("an older daemon's row");
        assert_eq!(summary.stopped_by, None);
        assert!(
            serde_json::to_value(&summary)
                .unwrap()
                .get("stopped_by")
                .is_none(),
            "an absent reason is not written as null"
        );
    }

    #[test]
    fn save_resources_is_one_boolean_each_way() {
        let asked: SaveResourcesSet = serde_json::from_str(r#"{"on":true}"#).unwrap();
        assert!(asked.on);
        assert_eq!(
            serde_json::to_value(SaveResources { on: false }).unwrap(),
            serde_json::json!({ "on": false })
        );
    }

    #[test]
    fn a_summary_of_a_service_with_no_row_omits_the_state_rather_than_inventing_one() {
        let summary = ServiceSummary {
            id: service("mailpit"),
            state: None,
            supervised: false,
            pid: None,
            port: None,
            last_started_at: None,
            last_exit_code: None,
            depends_on: Vec::new(),
            role: None,
            autostart: false,
            stopped_by: None,
            version: None,
            last_failure: None,
        };

        let encoded = serde_json::to_value(&summary).unwrap();
        assert!(encoded.get("state").is_none(), "{encoded}");
        assert!(encoded.get("role").is_none(), "{encoded}");
        assert!(encoded.get("version").is_none(), "{encoded}");
        assert!(encoded.get("last_failure").is_none(), "{encoded}");
        assert_eq!(encoded["supervised"], false);
        assert_eq!(
            serde_json::from_value::<ServiceSummary>(encoded).unwrap(),
            summary
        );
    }

    #[test]
    fn a_walk_that_stopped_carries_the_reason_the_transition_did() {
        let walk = ServiceWalk {
            planned: vec![service("mariadb@main"), service("php-fpm@8.3")],
            complete: true,
            reached: Vec::new(),
            failed: Some(ServiceFailure {
                service: service("mariadb@main"),
                reason: Some(StateReason::ReadyTimeout {
                    after: crate::Millis::from_secs(10),
                }),
            }),
            blocked: vec![service("php-fpm@8.3")],
        };

        let encoded = serde_json::to_value(&walk).unwrap();
        assert_eq!(encoded["failed"]["service"], "mariadb@main");
        assert_eq!(encoded["failed"]["reason"]["kind"], "ready_timeout");
        assert_eq!(
            serde_json::from_value::<ServiceWalk>(encoded).unwrap(),
            walk
        );
    }

    #[test]
    fn a_walk_that_was_not_waited_for_says_so_rather_than_looking_like_one_that_did_nothing() {
        let accepted = ServiceWalk {
            planned: vec![service("mariadb@main")],
            complete: false,
            reached: Vec::new(),
            failed: None,
            blocked: Vec::new(),
        };

        let encoded = serde_json::to_value(&accepted).unwrap();
        assert_eq!(encoded["complete"], false);
        assert!(
            encoded.get("failed").is_none(),
            "nothing failed — nothing has happened yet: {encoded}"
        );
    }
}

/// What `service.limits` and `service.set_limits` both answer.
///
/// **One report for a read and a write**, because the answer to "what are this service's limits" is
/// the same shape as the answer to "what are they now" — and because a client that has just written
/// one needs to be told what the machine will do with it just as much as one that has only read it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ServiceLimitsReport {
    /// Which service this is about.
    pub service: ServiceId,

    /// What has been asked for. Stored whether or not this machine can enforce it — see
    /// [`support`](Self::support), and the T68 design, D10.
    pub limits: ResourceLimits,

    /// What this machine will actually do with each field of it.
    pub support: LimitSupport,

    /// What is watching this service's ceiling, where nothing enforces it — task **T71a**.
    ///
    /// [`None`] means nothing is watching, which is two different situations with one answer: this
    /// machine caps `memory_mb` itself, or this service has not declared one. A client that drew a
    /// watchdog for either would be describing a loop that never runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watchdog: Option<MemoryWatchdog>,
}

/// What watches a service's `memory_mb` where this machine cannot enforce it — task **T71a**.
///
/// **Per service, which is why it is not a field of [`LimitSupport`]**: that type answers for the
/// machine and is handed no service, so it cannot say whether *this* one would be restarted. The
/// recipe answers that, and the answer therefore travels with the service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct MemoryWatchdog {
    /// Consecutive finished minutes over the line before the service is restarted.
    ///
    /// Minutes because the unit is the metrics row: the sampler finishes one per subject per
    /// minute, and the count is spent in those exactly as an idle policy is spent in sweeps.
    pub after_minutes: u32,

    /// Whether a restart follows at all, which is the recipe's answer.
    ///
    /// `false` is a service that is warned about and then left alone — a database, a cache. The
    /// warning still happens; a client that renders only the restart would show nothing at all for
    /// the services most worth saying something about.
    pub restarts: bool,
}

/// What `service.idle` answers — roadmap task **T69**.
///
/// **The exemptions are carried rather than folded into `policy`**, on the rule T46 wrote for
/// `DnsStatus`: "why is this still running?" has four answers that look identical from outside — no
/// policy, policy switched off, a running dependent, a keep-warm project — and collapsing them into
/// one `Option` sends a person to change a setting that was never the cause.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct IdleReport {
    /// Which service this is about.
    pub service: ServiceId,

    /// The policy in force, joined from the row and the recipe. [`None`] is never idle-stop.
    pub policy: Option<IdlePolicy>,

    /// Where that policy came from, or why there is none.
    pub source: IdleSource,

    /// What would stop it being stopped right now. Empty when nothing does.
    pub exempt: Vec<IdleExemption>,
}

/// What `service.set_idle` takes.
///
/// **Three states, spelled on the wire the way the column stores them.** [`None`] restores whatever
/// the recipe wants, `Some(0)` switches idle-stopping off for this service whatever the recipe
/// wants, and `Some(n)` is minutes. They are deliberately not two states: a default arriving in a
/// later release has to reach the home that never touched this setting and must never reach the one
/// whose owner turned it off — see `IdleSource`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ServiceIdleSet {
    /// The service whose policy this is.
    pub service: ServiceId,

    /// How many minutes it may look idle before it is stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minutes: Option<u32>,
}

/// What `service.set_front_end` takes — roadmap task **T97**.
///
/// **Which program, and not which row.** A caller does not name a `ServiceId`, because the id of the
/// service that is about to exist is the recipe's to decide and the one that is about to go is the
/// daemon's to find — see
/// [ADR 0026](https://github.com/mixnz/mixlab/blob/master/docs/decisions/0026-the-active-front-end-is-a-row-and-switching-it-is-a-job.md).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FrontEndSwitch {
    /// Which program every site on this machine should be reached through from now on.
    ///
    /// The same value [`ServiceRole::FrontEnd`] reports, which is what makes a switch drawable from
    /// a listing and nothing else.
    pub server: FrontEndServer,

    /// Which installed version of it, and the newest installed when absent.
    ///
    /// **Optional because nobody choosing a web server is choosing a patch release.** A version
    /// that is named and not installed is refused; no version installed at all is refused with the
    /// install command, because a switch installs nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<PackageVersion>,

    /// Flush the elevation queue in this same call, raising the one prompt.
    ///
    /// **Defaults to `false`**, on [`UninstallQuery::grant`](crate::UninstallQuery)'s rule and for
    /// its reason: what is about to be allowed is read before it is allowed. A switch that needs a
    /// grant it was not allowed to raise answers [`FrontEndOutcome::NotGranted`] with the operation
    /// left waiting, so allowing it and asking again works.
    #[serde(default)]
    pub grant: bool,
}

/// What a switch did — roadmap task **T97**.
///
/// **A measurement of the home afterwards, not a claim about what was attempted**, which is
/// [`UninstallReport`](crate::UninstallReport)'s shape and its reasoning: the interesting outcomes
/// here are the ones where the home ends up where it started, and a client has to be able to render
/// each of them as the outcome it is rather than as a failure with no detail.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FrontEndReport {
    /// What the home was reached through when the call began, and [`None`] for a home that had no
    /// front end at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub was: Option<ServiceId>,

    /// What it is reached through now. Equal to [`was`](Self::was) whenever nothing moved, and
    /// [`None`] only where a switch failed and could not be undone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now: Option<ServiceId>,

    /// What happened, and why.
    pub outcome: FrontEndOutcome,

    /// Whether the front end named by [`now`](Self::now) may bind 80 and 443 on this machine.
    ///
    /// **Read from the machine after the walk, whatever the outcome.** `false` is the degraded mode
    /// of [ADR 0005] and not a failure of this call: a Linux home where nobody has granted
    /// `cap_net_bind_service` has a front end that will not start, and it had one before this was
    /// called too.
    ///
    /// [ADR 0005]: https://github.com/mixnz/mixlab/blob/master/docs/decisions/0005-on-demand-elevation.md
    pub answering: bool,

    /// The old front end's data directory, kept where it was.
    ///
    /// `service.delete`'s rule — nothing about replacing a program says anything about wanting its
    /// files gone — and named for its reason: a directory nobody was told about is a directory
    /// nobody ever cleans up. [`None`] when there was none on disk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kept_data: Option<String>,

    /// What did not travel to the new row, one sentence each.
    ///
    /// **Settings measured against the program that is going**: an override written in one server's
    /// configuration language, a ceiling, an idle policy. A silently dropped override is the failure
    /// this member exists to prevent; a listed one is something a person can put back. Empty when
    /// the old row carried none of them.
    pub not_carried: Vec<String>,
}

impl FrontEndReport {
    /// Did the home actually move?
    #[must_use]
    pub fn moved(&self) -> bool {
        matches!(self.outcome, FrontEndOutcome::Switched { .. })
    }

    /// Did the caller ask for something they did not get?
    ///
    /// **What `mix service set-front-end` exits with**, on `UninstallReport::left_behind`'s rule.
    /// [`FrontEndOutcome::Unchanged`] is deliberately not one: asking for the server a home is
    /// already on is a question with an answer, not a request that failed.
    #[must_use]
    pub fn wanted_more(&self) -> bool {
        matches!(
            self.outcome,
            FrontEndOutcome::NotGranted { .. }
                | FrontEndOutcome::RolledBack { .. }
                | FrontEndOutcome::Failed { .. }
        )
    }
}

/// The five ends a switch can have — roadmap task **T97**.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum FrontEndOutcome {
    /// It was already the front end. Nothing was stopped, deleted, created or started.
    ///
    /// Not an error: a switch clicked on the option that is already selected is an ordinary thing to
    /// happen, and refusing it would make a client suppress the call — which is the client deciding.
    Unchanged {},

    /// Done.
    Switched {
        /// The walk that brought the new front end up, and [`None`] when the old one was not
        /// running and nothing was started in its place.
        ///
        /// **A switch preserves what it found.** Starting a server somebody had deliberately
        /// stopped is the tool overruling its user.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        started: Option<ServiceWalk>,
    },

    /// This machine will not let the new front end answer on 80 and 443 and the one it is on can,
    /// so the home was left where it was.
    ///
    /// ADR 0026's *"a machine where nobody grants stays on the front end it had"*. The grant this
    /// asked for is still waiting, so allowing it and asking again works.
    NotGranted {
        /// What the machine said, phrased for a person.
        because: String,
    },

    /// Something failed after the old row had gone, and the old front end was put back.
    ///
    /// The usual cause is the one worth catching: a front end renders through its own checker —
    /// `nginx -t`, `caddy validate` — so a configuration the new program refuses is caught at the
    /// one step that is still undoable.
    RolledBack {
        /// What refused, unedited.
        because: String,
    },

    /// Something failed and the home could not be put back either.
    ///
    /// [`FrontEndReport::now`] says what is there, which may be nothing at all. Nothing can prevent
    /// this ending; the report exists so that it is a sentence on somebody's screen instead of a
    /// home that quietly stopped serving.
    Failed {
        /// What went wrong, in the order it went wrong.
        because: String,
    },
}

/// What `service.set_limits` takes.
///
/// **A complete [`ResourceLimits`], never a patch.** A field left at its default is a field being
/// set to its default — so setting `cpu_percent` alone removes a memory ceiling that was there. That
/// is the specified behaviour rather than an accident: the alternative is a three-way value per
/// field in every reader of limits, or a read-modify-write in a client, which is business logic a
/// client may not hold. `mix service limits set` prints all three fields of the result, which is
/// where the cost of this choice is paid.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ServiceLimitsSet {
    /// The service to cap.
    pub service: ServiceId,

    /// Everything it may take, in full.
    pub limits: ResourceLimits,
}
