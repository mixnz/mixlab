//! The queue of privileged operations this daemon is holding, and the only thing that raises a
//! prompt. Roadmap task **T40b**.
//!
//! **The division is the one [`crate::jobs`] documents, one table across.**
//! [`mixengine_core::elevation`] owns the row, the document and the report and has no loop, no clock
//! and no task; what owns the timing, the cancellation the grant hangs off and the `Events` the
//! batch is announced on is here.
//!
//! **This daemon never raises a prompt on its own initiative.**
//! `docs/architecture/daemon-and-ipc.md` already carries the rule: *a method that writes outside
//! `MIXENGINE_HOME` is never called on the daemon's own initiative* (T26). Everything the helper
//! will ever do — the hosts file, the trust store, the resolver, a firewall rule — is outside the
//! home by definition; that is why it needs root. So enqueuing and flushing have two different
//! triggers: producers enqueue, and only a client calls `elevation.grant`. A fresh install where
//! nobody ever does is a machine in degraded mode forever, and that is the correct behaviour.
//!
//! **T41's `HostsApply` is the first producer.** [`Elevation::enqueue`] ships with none, which is the
//! deliberate position rather than an oversight — the same one T22 took with the job registry and
//! T19 with the service runner: the alternative is writing the queue twice, once inside the first
//! producer and once properly afterwards.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use mixengine_core::{Paths, Store};
use mixengine_platform::{ElevationSupport, Host};
use mixengine_proto::privileged::{OpOutcome, PrivilegedOp};
use mixengine_proto::{
    ByHand, DaemonEvent, ElevationDrop, ElevationStatus, ElevationSummary, Error, GrantOutcome,
    JobId, Timestamp,
};

use crate::api::Events;
use crate::error::ToWire as _;

mod by_hand;
mod flush;
mod require;

/// Where this home records the firewall plan it last had applied — roadmap task **T180**.
///
/// **A memory and not a reading.** The daemon never reads a machine's rule set back (T74), so what
/// it can honestly compare a new plan against is the last one a grant reported done. Absent means
/// no rules, which is what a fresh home has.
pub(crate) const FIREWALL_APPLIED: &str = "firewall.applied";

/// The single grant slot — the runtime half of "no code path elevates in a loop".
///
/// Two concurrent grants are two prompts for one queue, which is the defect ADR 0005 names, and
/// refusing the second is the only answer that cannot itself become a loop.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Slot {
    /// Nothing is being granted.
    #[default]
    Free,

    /// A grant has been accepted and its job row is not written yet.
    ///
    /// Its own state rather than a placeholder id: the row is written by `Jobs::begin`, and a
    /// `JobId(0)` held in the meantime would be a number a client could be told.
    Reserved,

    /// A grant is running, and this is the job to wait on.
    Running(JobId),
}

/// What this daemon knows that outlives a single grant but not a restart.
#[derive(Debug, Default)]
struct State {
    /// The one grant at a time.
    slot: Slot,

    /// What the most recent one did — in memory, deliberately. See
    /// [`ElevationStatus::last`](mixengine_proto::ElevationStatus).
    last: Option<GrantOutcome>,

    /// The grant in the slot, while it waits for a terminal to run the helper (ADR 0065).
    waiting: Option<ByHand>,
}

/// How long a grant waits for somebody to run the helper from a terminal before it counts as
/// declined, and gives the slot back.
const BY_HAND_WAIT: Duration = Duration::from_secs(600);

/// How the one prompt of a grant is raised (ADR 0065).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Route {
    /// This daemon raises the operating system's dialog: UAC, `osascript`, `pkexec`.
    Dialog,

    /// This daemon writes the request and waits for a terminal to run the helper on it. Nothing
    /// is spawned here, so a machine where no dialog can be raised is not refused.
    ByHand,
}

impl From<bool> for Route {
    /// The `by_hand` flag `elevation.grant` and `daemon.uninstall` carry.
    fn from(by_hand: bool) -> Self {
        match by_hand {
            true => Self::ByHand,
            false => Self::Dialog,
        }
    }
}

/// The two files this daemon could hand an elevation prompt — roadmap task **T85**.
///
/// One value rather than two fields because they are one question: *which file does this machine
/// run as root?* [`mixengine_core::elevation::helper`] answers it, and both halves have to reach it
/// together.
#[derive(Debug, Clone)]
pub(crate) struct Candidates {
    /// The program that is running, which is what a shipped helper is found beside (T40b's D9).
    pub(crate) program: PathBuf,

    /// Where **this operating system** keeps an installed privileged helper.
    ///
    /// Resolved once, by whoever constructs this, from
    /// [`mixengine_platform::install::helper_path`], and carried rather than asked for again: it
    /// cannot change under a running process, which is `elevated`'s reasoning below.
    ///
    /// `None` is a machine that will not name one — and, in this module's own tests, a machine the
    /// test is *describing* as having none, which is the only way the "nothing installed" row of
    /// T85's D5 can be exercised on a developer machine that does have one.
    pub(crate) installed: Option<PathBuf>,
}

/// The queue, the machine that can be asked about it, and the only door into a prompt.
/// What a grant does once the prompt has been answered — roadmap task **T182b**, D6.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Afterwards {
    /// Ask again for the hosts block this home's sites want, which the grant may have made
    /// redundant. Every ordinary grant.
    Reconcile,

    /// Nothing: an uninstall's grant, which must not queue again what it is removing.
    Nothing,
}

/// The helper a grant runs, and the protocol its request is written at — the T182b design, D3.
#[derive(Debug, Clone)]
struct Chosen {
    /// The file handed to the elevation prompt.
    path: PathBuf,

    /// What the request is marked at: the lower of this daemon's and the installed helper's, or
    /// this daemon's own when the shipped copy runs the batch.
    speaks: mixengine_proto::ProtocolVersion,
}

#[derive(Debug)]
pub(crate) struct Elevation {
    /// Where the rows live.
    store: Store,

    /// How a batch is announced.
    events: Events,

    /// The registry a grant becomes a job in.
    jobs: Arc<crate::jobs::Jobs>,

    /// The OS: `probe()` for the degraded mode, `run()` for the grant.
    host: Arc<dyn Host>,

    /// `MIXENGINE_HOME`, which every request names and the helper checks the ownership of.
    home: PathBuf,

    /// `<root>/run/elevate` — the parent of every single-use request directory.
    elevate: PathBuf,

    /// The two files a prompt could be handed — see [`Candidates`].
    candidates: Candidates,

    /// Whether **this daemon** holds an administrative token, read once at construction.
    ///
    /// Reported and not refused — the T40b design, D10. Read once because it cannot change under a
    /// running process, and reading it per request would be a syscall per `mix status`.
    elevated: bool,

    /// Which of the two name mechanisms this home is on — roadmap task **T44**.
    ///
    /// Read by [`require_hosts`](Elevation::require_hosts) and by nothing else here: whether a
    /// managed name resolves through DNS decides whether the hosts file needs to hold it at all.
    dns: Arc<crate::dns::Dns>,

    /// The grant slot and the last outcome.
    state: Mutex<State>,

    /// What the installed helper answered a handshake with — roadmap task **T88a**.
    ///
    /// Taken once at start, when there *is* an installed helper, by running it as an ordinary
    /// process: nothing here costs a prompt. [`None`] is a machine with nothing installed, or one
    /// that would not answer, and every reader treats those the same — this daemon does not know
    /// and will not guess.
    facts: Mutex<Option<crate::helper::HelperFacts>>,

    /// How long a [`Route::ByHand`] grant waits — [`BY_HAND_WAIT`], and shorter only in tests.
    by_hand_wait: Duration,
}

impl Elevation {
    /// A registry with nothing waiting and nothing running.
    pub(crate) fn new(
        paths: &Paths,
        store: &Store,
        events: Events,
        jobs: Arc<crate::jobs::Jobs>,
        host: Arc<dyn Host>,
        candidates: Candidates,
        dns: Arc<crate::dns::Dns>,
    ) -> Arc<Self> {
        Arc::new(Self {
            store: store.clone(),
            events,
            jobs,
            elevated: mixengine_platform::elevated::is_elevated(),
            host,
            home: paths.root().to_path_buf(),
            elevate: paths.run().join("elevate"),
            candidates,
            dns,
            state: Mutex::new(State::default()),
            facts: Mutex::new(None),
            by_hand_wait: BY_HAND_WAIT,
        })
    }

    /// Put an operation in the queue, and announce the batch when that changed something.
    ///
    /// [`require_hosts`](Self::require_hosts) is the one caller, and T41 made it the first: the
    /// queue and its event landed with T40b so that T41 would be one operation rather than an
    /// operation plus a mechanism.
    ///
    /// # Errors
    ///
    /// The wire error of a row that could not be written.
    pub(crate) async fn enqueue(&self, op: &PrivilegedOp) -> Result<(), Error> {
        let at = Timestamp::from_system_time(SystemTime::now());

        let announced = mixengine_core::elevation::enqueue(&self.store, op, at)
            .await
            .map_err(|error| error.to_wire())?;

        // `None` is the operation that was already waiting. The machine's needs did not change, so
        // there is nothing to announce — an event per attempt would put a producer's retry loop on a
        // client's screen. See the T40b design, D8.
        if let Some(pending) = announced {
            self.events
                .publish(DaemonEvent::ElevationRequired { pending });
        }

        Ok(())
    }

    /// Take an operation out of the queue, because this machine no longer needs it — roadmap task
    /// **T179**.
    ///
    /// **The producers' other answer.** Each of them reads what this home declares and what the
    /// machine holds; when those agree there is nothing to ask for, and a row queued by an earlier
    /// reading describes a home that no longer exists. Leaving it there put a sentence in front of a
    /// prompt that did not match what the grant would do.
    ///
    /// **A withdrawal that removed nothing publishes nothing**, on [`enqueue`](Self::enqueue)'s
    /// rule: a producer runs at every daemon start and on every change, and an event per attempt
    /// would put its loop on a client's screen. One that did publishes the queue that is left,
    /// because [`DaemonEvent::ElevationRequired`] carries the whole queue and a client redraws from
    /// it.
    ///
    /// # Errors
    ///
    /// The wire error of a row that could not be removed, or a queue that could not be read back.
    pub(crate) async fn no_longer_needed(&self, key: &str) -> Result<(), Error> {
        let removed = mixengine_core::elevation::withdraw(&self.store, key)
            .await
            .map_err(|error| error.to_wire())?;

        if removed == 0 {
            return Ok(());
        }

        let waiting = self.announce_what_is_left().await?;

        tracing::info!(key, waiting, "a queued operation is no longer needed");

        Ok(())
    }

    /// Publish the queue as it now stands, after something took rows out of it, and say how many
    /// are left.
    ///
    /// [`DaemonEvent::ElevationRequired`] carries the whole queue, so a shrunken one is announced the
    /// same way a grown one is, and an empty batch is how a client learns nothing waits any more.
    /// Without it a window that had drawn the list kept drawing it after the rows were gone, and an
    /// "Allow" on that list was answered "nothing is waiting for permission".
    ///
    /// # Errors
    ///
    /// The wire error of a queue that could not be read back.
    async fn announce_what_is_left(&self) -> Result<usize, Error> {
        let pending = mixengine_core::elevation::pending(&self.store)
            .await
            .map_err(|error| error.to_wire())?;
        let waiting = pending.len();

        self.events
            .publish(DaemonEvent::ElevationRequired { pending });

        Ok(waiting)
    }

    /// Record the firewall plan this machine now holds — roadmap task **T180**.
    ///
    /// **Only what landed.** `Applied`, `AlreadyDone` and `Unmanaged` all mean the queue stops
    /// asking — that is `settle`'s own rule, and T74's for the third — so all three are what this
    /// home now has. A refusal, an unsupported operation and a failure change nothing and record
    /// nothing, so the next sharing change asks again.
    ///
    /// **Never fails a grant.** The batch has already been applied by the time this runs; a record
    /// that could not be written costs a prompt somebody will see again, which is worth a line in
    /// the log and nothing more.
    async fn remember_the_firewall(
        &self,
        waiting: &[mixengine_proto::PendingOp],
        results: &[(mixengine_proto::PendingOpId, OpOutcome)],
    ) {
        let landed = results.iter().find_map(|(id, outcome)| {
            if !matches!(
                outcome,
                OpOutcome::Applied { .. } | OpOutcome::AlreadyDone | OpOutcome::Unmanaged { .. }
            ) {
                return None;
            }

            waiting
                .iter()
                .find(|pending| pending.id == *id)
                .and_then(|pending| match &pending.op {
                    PrivilegedOp::FirewallApply { plan } => Some(plan.clone()),
                    _ => None,
                })
        });

        let Some(plan) = landed else {
            return;
        };

        if let Err(error) =
            mixengine_core::updates::records::set(&self.store, FIREWALL_APPLIED, &plan).await
        {
            tracing::warn!(
                %error,
                "the firewall plan was applied and could not be recorded; the next share will ask again"
            );
        }
    }

    /// Ask for `op`, and for a privileged helper in front of it when this machine has none —
    /// roadmap task **T88d**.
    ///
    /// **The four `require_*` producers use this and nothing else does.** They are where *this
    /// machine needs something done as root* is decided, which is the question a missing helper is
    /// an answer to; [`enqueue`](Self::enqueue) itself is also how the uninstall's rows and the
    /// helper's own two get in, and a batch that installed a helper in order to remove it would be
    /// doing and undoing work in front of a person reading the list.
    ///
    /// **At the enqueue and not at the top of the producer**, so a machine that already agrees is
    /// still asked for nothing: a helper row raised by a site creation that needed no hosts entry
    /// would be a prompt with no work behind it.
    ///
    /// # Errors
    ///
    /// The wire error of a row that could not be written.
    async fn enqueue_needing_a_helper(&self, op: &PrivilegedOp) -> Result<(), Error> {
        self.bootstrap_the_helper().await?;

        self.enqueue(op).await
    }

    /// Ask for a helper when this machine has none installed and ships one to install from —
    /// roadmap task **T88d**.
    ///
    /// **Because [`require_helper`](Self::require_helper) runs at start and nowhere else**, and a
    /// `mix uninstall` that kept the home does not restart the daemon: until this, a person who
    /// removed MixEngine's footprint and went on using it met `dependency_missing` at every prompt
    /// for as long as that daemon lived, and on the three formats an installer writes as root there
    /// was no way back at all.
    ///
    /// **Silent when there is nothing to install from.** A row whose only possible outcome is a
    /// refusal is worse than the refusal `elevation.grant` already gives, and it would sit on
    /// `mix status` for ever.
    ///
    /// **And it says so when the source is not an administrator's**, which is the whole of what
    /// `docs/architecture/security-model.md`'s residual is observable as. Read here, once per
    /// row asked for, rather than in `mixengine_core::elevation::helper` — which every `mix status`
    /// and every poll a window makes goes through.
    ///
    /// # Errors
    ///
    /// The wire error of a row that could not be written.
    async fn bootstrap_the_helper(&self) -> Result<(), Error> {
        if self
            .candidates
            .installed
            .as_deref()
            .is_some_and(Path::is_file)
        {
            return Ok(());
        }

        let Ok(source) = mixengine_core::elevation::helper(
            &self.candidates.program,
            self.candidates.installed.as_deref(),
        ) else {
            return Ok(());
        };

        if let Some(why) = mixengine_core::elevation::source_trust(&source) {
            tracing::warn!(
                source = %source.display(),
                %why,
                "the privileged helper would be installed from a file an ordinary account can rewrite"
            );
        }

        self.enqueue(&PrivilegedOp::HelperInstall {}).await
    }

    /// This machine, for the one other thing in this daemon that reads it — roadmap task **T46**.
    ///
    /// **Reached through here rather than built again.** A `Host` is a handful of trait objects and
    /// a second one is cheap, but two of them are two answers to "what does this machine's hosts
    /// file hold" — and the diagnostic exists to report the answer *this queue acted on*.
    pub(crate) fn host(&self) -> Arc<dyn Host> {
        Arc::clone(&self.host)
    }

    /// Is there a helper where this system keeps one? A read of the file system, no probe.
    pub(crate) fn helper_is_installed(&self) -> bool {
        self.candidates
            .installed
            .as_deref()
            .is_some_and(Path::is_file)
    }

    /// Whether this is the first time this home is asked to prompt for `version`'s helper — the
    /// T182b design, D2. Answers `true` once per version, so a person who declines is not asked at
    /// every start; the replacement then waits for the next prompt the product needs anyway.
    pub(crate) async fn first_prompt_for(&self, version: &str) -> bool {
        const KEY: &str = "helper.prompted";

        let seen: Option<String> = mixengine_core::updates::records::get(&self.store, KEY)
            .await
            .ok()
            .flatten();

        if seen.as_deref() == Some(version) {
            return false;
        }

        if let Err(error) =
            mixengine_core::updates::records::set(&self.store, KEY, &version.to_owned()).await
        {
            tracing::warn!(%error, "could not remember that the helper's prompt was raised");
        }

        true
    }

    /// Which helper runs this batch, and at which protocol — the T182b design, D3.
    ///
    /// **The installed one, unless it cannot read the batch** and the copy this release ships can.
    /// The shipped copy is only probed when the installed one's `supported_ops` are missing an
    /// operation, so an ordinary grant costs nothing extra. When it runs the batch, the request is
    /// written at this daemon's own protocol, since that copy is this release.
    async fn choose_for(&self, waiting: &[mixengine_proto::PendingOp]) -> Result<Chosen, Error> {
        let installed_ops: Option<Vec<String>> = self
            .facts
            .lock()
            .ok()
            .and_then(|facts| facts.as_ref().map(|facts| facts.supported_ops.clone()));

        let batch: Vec<&str> = waiting.iter().map(|pending| pending.op.name()).collect();

        // Both conditions `Bypass::applies` looks at, read first so that the shipped copy is only
        // probed when one of them could be true.
        let worth_asking = installed_ops.as_ref().is_some_and(|known| {
            let knows = |op: &str| known.iter().any(|name| name == op);
            batch.iter().any(|op| !knows(op))
                || (batch.contains(&"helper-install") && !knows("helper-replace"))
        });

        let shipped_version = match worth_asking {
            true => match mixengine_core::elevation::shipped(&self.candidates.program) {
                Some(shipped) => crate::helper::handshake(&shipped, &self.home, &self.elevate)
                    .await
                    .map(|facts| facts.version),
                None => None,
            },
            false => None,
        };

        let bypass =
            installed_ops
                .as_deref()
                .map(|installed_ops| mixengine_core::elevation::Bypass {
                    installed_ops,
                    shipped_version: shipped_version.as_deref(),
                    batch_ops: &batch,
                });

        let path = mixengine_core::elevation::helper_for(
            &self.candidates.program,
            self.candidates.installed.as_deref(),
            bypass.as_ref(),
        )
        .map_err(|error| error.to_wire())?;

        let bypassed = bypass.is_some_and(|bypass| bypass.applies());
        if bypassed {
            tracing::info!(
                helper = %path.display(),
                "the installed helper cannot read this batch, so the one this release ships runs it"
            );
        }

        Ok(Chosen {
            path,
            speaks: match bypassed {
                true => mixengine_proto::PROTOCOL_VERSION,
                false => self.speaks(),
            },
        })
    }

    /// Raise the prompt **inside the caller's job**, rather than starting one of its own.
    ///
    /// [`grant`](Self::grant) answers with a job and returns; a caller with work to do *after*
    /// permission is given cannot use it, because there is no hook between one job ending and
    /// another beginning. `cert.ca_rotate` is that caller — roadmap task **T54**: whether it
    /// replaces this home's authority depends on what the trust store says once the prompt has been
    /// answered, and reading it any earlier would answer a different question.
    ///
    /// The slot is released however this ends, because [`flush`](Self::flush) holds a `Drop` guard
    /// rather than a last statement. That is what makes the split safe: a caller that panics between
    /// the grant and its own next step does not wedge every later grant for the life of this daemon.
    ///
    /// # Errors
    ///
    /// As [`preflight`](Self::preflight), and whatever the helper answered.
    pub(crate) async fn grant_within(
        &self,
        handle: &crate::jobs::JobHandle,
    ) -> Result<serde_json::Value, Error> {
        let (waiting, helper) = self.preflight(None, Route::Dialog).await?;

        self.flush(
            handle,
            helper,
            waiting,
            Afterwards::Reconcile,
            Route::Dialog,
        )
        .await
    }

    /// [`grant_within`](Self::grant_within) over the rows `ids` names, and nothing else waiting —
    /// roadmap task **T182b**, D6.
    ///
    /// **An uninstall grants only what it asked for.** Everything else in the queue is somebody
    /// else's want: a daemon started on a kept home queues the wiring its sites still need, and a
    /// prompt raised to take MixEngine off the machine must not put that wiring back.
    ///
    /// # Errors
    ///
    /// As [`grant_within`](Self::grant_within); none of `ids` still waiting is "nothing is waiting".
    pub(crate) async fn grant_only(
        &self,
        handle: &crate::jobs::JobHandle,
        ids: &[mixengine_proto::PendingOpId],
        route: Route,
    ) -> Result<serde_json::Value, Error> {
        let (waiting, helper) = self.preflight(Some(ids), route).await?;

        self.flush(handle, helper, waiting, Afterwards::Nothing, route)
            .await
    }

    /// The three facts `daemon.status` carries.
    ///
    /// # Errors
    ///
    /// The wire error of a queue that could not be read.
    pub(crate) async fn summary(&self) -> Result<ElevationSummary, Error> {
        let waiting = mixengine_core::elevation::pending(&self.store)
            .await
            .map_err(|error| error.to_wire())?;

        Ok(ElevationSummary {
            elevated: self.elevated,
            can_prompt: self.reason().is_none(),
            pending: waiting.len(),
        })
    }

    /// What the installed helper is, as the handshake at start found it — roadmap task **T88a**.
    ///
    /// The sentence is composed here rather than by a client, on `PendingOp::description`'s rule:
    /// what to do about an old helper differs by *which* old helper it is, and a client deciding
    /// that would be a client deciding what runs as root.
    pub(crate) fn installed_helper(&self) -> Option<mixengine_proto::InstalledHelper> {
        let facts = self.facts.lock().ok()?.clone()?;

        Some(mixengine_proto::InstalledHelper {
            upgrade: crate::helper::upgrade_sentence(
                &facts,
                mixengine_proto::privileged::HELPER_VERSION,
            ),
            version: facts.version,
            protocol: facts.speaks.0,
            supported_ops: facts.supported_ops,
        })
    }

    /// What the installed helper answered a handshake with, for the one caller that needs the
    /// operation list rather than the sentence — `crate::helper::upgrade`.
    pub(crate) fn facts(&self) -> Option<crate::helper::HelperFacts> {
        self.facts.lock().ok()?.clone()
    }

    /// The protocol to mark a request to the helper with — roadmap task **T88a**.
    ///
    /// The lower of what this daemon speaks and what the helper answered, because a fixed old binary
    /// can never be taught a newer protocol and the newer peer is therefore the one that speaks
    /// down. With no facts it is this build's own: that is the machine where nothing is installed
    /// and the file being elevated is the copy shipped beside this very daemon.
    fn speaks(&self) -> mixengine_proto::ProtocolVersion {
        self.facts
            .lock()
            .ok()
            .and_then(|facts| facts.as_ref().map(|facts| facts.speaks))
            .map_or(mixengine_proto::PROTOCOL_VERSION, |theirs| {
                theirs.min(mixengine_proto::PROTOCOL_VERSION)
            })
    }

    /// `elevation.status` — the screen.
    ///
    /// # Errors
    ///
    /// The wire error of a queue that could not be read.
    pub(crate) async fn status(&self) -> Result<ElevationStatus, Error> {
        let pending = mixengine_core::elevation::pending(&self.store)
            .await
            .map_err(|error| error.to_wire())?;

        let reason = self.reason();
        let (last, waiting) = {
            let state = self
                .state
                .lock()
                .expect("the elevation slot is not held across an await");

            (state.last.clone(), state.waiting.clone())
        };

        Ok(ElevationStatus {
            elevated: self.elevated,
            can_prompt: reason.is_none(),
            reason,
            helper: mixengine_core::elevation::helper(
                &self.candidates.program,
                self.candidates.installed.as_deref(),
            )
            .ok()
            .map(|path| path.display().to_string()),
            installed_helper: self.installed_helper(),
            pending,
            last,
            waiting,
        })
    }

    /// `elevation.drop` — forget one operation, or all of them, and answer with what is left.
    ///
    /// Answering with the whole [`ElevationStatus`] rather than a count: what a person does next is
    /// look at the list, and a client that had to call again to see it would render a stale one in
    /// between.
    ///
    /// # Errors
    ///
    /// The wire error of a queue that could not be written or read back.
    pub(crate) async fn drop_pending(
        &self,
        asked: &ElevationDrop,
    ) -> Result<ElevationStatus, Error> {
        let removed = mixengine_core::elevation::discard(&self.store, asked.op)
            .await
            .map_err(|error| error.to_wire())?;

        tracing::info!(removed, "pending privileged operations were dropped");

        self.status().await
    }

    /// Why a prompt cannot be raised here, or [`None`] when one can.
    ///
    /// **Two halves, and the sentence differs.** A machine with no authentication agent cannot show
    /// a prompt; a daemon with no `mixengine-elevate` beside it has nothing to show one *for*. Both
    /// leave `can_prompt` false, and only one of them is fixed by installing a polkit agent.
    fn reason(&self) -> Option<String> {
        if let Some(reason) = self.helper_reason() {
            return Some(reason);
        }

        match self.host.elevation().probe() {
            ElevationSupport::Available => None,
            ElevationSupport::Unavailable { reason } => Some(reason),
        }
    }

    /// Why there is no helper to run, or [`None`] when there is one — the half of
    /// [`reason`](Self::reason) a terminal cannot fix either.
    fn helper_reason(&self) -> Option<String> {
        mixengine_core::elevation::helper(
            &self.candidates.program,
            self.candidates.installed.as_deref(),
        )
        .err()
        .map(|error| error.to_string())
    }
}

/// The grant slot, released however the work ends.
///
/// A guard and not a last statement, on `Going`'s reasoning in [`crate::api`]: the future serving a
/// job is dropped where it stands if the daemon shuts down under it, and a panic anywhere inside the
/// flush does the same. A slot left `Running` after either would refuse every later grant for as
/// long as this daemon lives — with no way out short of restarting it.
struct Released<'a>(&'a Elevation);

impl Drop for Released<'_> {
    fn drop(&mut self) {
        self.0.release();
    }
}

/// Whether a batch held an operation that could have put a different helper on this machine.
///
/// **The two that write the helper's own file, and nothing else.** `HelperRemove` is not one: on
/// Windows the file it removed is still there until the next restart, and on the other two the
/// probe finds nothing to run — neither answer is a new helper to learn. And a probe after a batch
/// that removed the audit log recreates it on a machine whose token is already elevated, which is
/// how CI's Windows runner read an uninstall as unfinished; see `judge`.
fn may_have_changed_the_helper(waiting: &[mixengine_proto::PendingOp]) -> bool {
    waiting
        .iter()
        .any(|pending| is_about_the_helper(&pending.op))
}

/// The helper's own operation first, everything else in the order it was queued — the T182b
/// design, D2.
fn put_the_helper_first(waiting: &mut [mixengine_proto::PendingOp]) {
    waiting.sort_by_key(|pending| !is_about_the_helper(&pending.op));
}

/// Does this operation put a helper where the system keeps one?
fn is_about_the_helper(op: &PrivilegedOp) -> bool {
    matches!(
        op,
        PrivilegedOp::HelperInstall {} | PrivilegedOp::HelperReplace {}
    )
}

#[cfg(test)]
mod tests;
