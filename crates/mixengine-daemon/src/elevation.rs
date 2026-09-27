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
use std::time::SystemTime;

use mixengine_core::{Paths, Store};
use mixengine_platform::{ElevationSupport, Host};
use mixengine_proto::privileged::{ElevationOutcome, OpOutcome, PrivilegedOp};
use mixengine_proto::{
    DaemonEvent, ElevationDrop, ElevationStatus, ElevationSummary, Error, ErrorCode, GrantOutcome,
    JobId, JobKind, JobSummary, Timestamp, rpc,
};

use crate::api::Events;
use crate::error::ToWire as _;

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

        let pending = mixengine_core::elevation::pending(&self.store)
            .await
            .map_err(|error| error.to_wire())?;

        tracing::info!(
            key,
            waiting = pending.len(),
            "a queued operation is no longer needed"
        );

        self.events
            .publish(DaemonEvent::ElevationRequired { pending });

        Ok(())
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

    /// Ask for this helper to be installed where only an administrator can rewrite it — roadmap
    /// task **T85**.
    ///
    /// **Called at every daemon start, before the other three and for their reason**: what it asks
    /// for lands in the single grant first-run setup already costs rather than behind a prompt of
    /// its own. Before them rather than after, because it is about the file those three are applied
    /// *by*. Reading the answer costs two `stat`s in the ordinary case, which is what makes asking
    /// every time affordable.
    ///
    /// Five answers, and only two of them queue anything (the T85 design, D6):
    ///
    /// - **no helper shipped beside this program** — there is nothing to install, so nothing is
    ///   asked for. That is a daemon somebody moved on its own, and `elevation.grant` already says
    ///   so where a person can act on it;
    /// - **nothing installed** — asked for;
    /// - **installed, and the same bytes** — nothing to do, and this is the ordinary case;
    /// - **installed, and different bytes** — asked for, because a MixEngine upgraded yesterday is
    ///   otherwise driving the helper from the version before it for ever. **That is a replacement
    ///   behind an explicit prompt and not an auto-update**: nothing is copied until somebody
    ///   allows a batch, which is exactly what
    ///   `docs/architecture/security-model.md`'s auto-update boundary asks for. The signature
    ///   check that decides whether the new binary deserved that prompt at all is **T88a**;
    /// - **installed and not an administrator's** — nothing is asked for, and a warning is logged.
    ///   The helper would refuse the operation anyway, and `elevation.status` is already saying so
    ///   through [`mixengine_core::elevation::helper`]'s own refusal.
    ///
    /// A read that fails asks for nothing, as [`require_resolver`](Self::require_resolver) does:
    /// a comparison that could not be made has said nothing about what to ask for.
    ///
    /// # Errors
    ///
    /// The wire error of a row that could not be written.
    pub(crate) async fn require_helper(&self) -> Result<(), Error> {
        let Some(installed) = self.candidates.installed.as_deref() else {
            tracing::warn!("this machine will not name a directory for a privileged helper");
            return Ok(());
        };

        // **The installed helper is asked first, and whether one is shipped beside this daemon has
        // nothing to do with it.** macOS's package puts the helper in `/Library/PrivilegedHelperTools`
        // and nothing in `/usr/local/bin` beside `mixengined`, so a gate on the beside copy — which
        // is what this used to open with — meant no packaged macOS install ever ran the handshake:
        // the helper upgrade reported *no privileged helper installed* on a machine whose
        // helper had just served two grants, and a daemon restart did not change its mind. The
        // beside copy is what an *install* is made from, and that is the only question it answers.
        if installed.is_file() {
            self.learn_installed_helper().await;
            return Ok(());
        }

        let beside = self
            .candidates
            .program
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(format!("mixengine-elevate{}", std::env::consts::EXE_SUFFIX));

        if !beside.is_file() {
            tracing::debug!(
                "no helper is shipped beside this daemon, so there is none to install anywhere"
            );
            return Ok(());
        }

        self.enqueue(&PrivilegedOp::HelperInstall {}).await
    }

    /// Ask the installed helper what it is, and remember the answer for `helper::keep_in_step`.
    ///
    /// **Version and not bytes, and nothing is enqueued** — roadmap task T88a. The bytes beside this
    /// daemon are *not* the newer helper after a `mix self-update`, which keeps the helper by name;
    /// and in a development tree they differ on every rebuild, which put a row on `mix status` whose
    /// only meaning was "you rebuilt". What decides is what the installed helper says it is — and a
    /// replacement needs a signed candidate, which has to be fetched, which is `mix elevation
    /// upgrade`'s job. A daemon start that reached the network would be a start an offline machine
    /// pays for, which `docs/features/updates.md` forbids in as many words.
    ///
    /// Called at start, and again after a prompt that installed or replaced the helper: a daemon
    /// that learned the helper's version only at start went on answering *none installed* until it
    /// was restarted. Not after every prompt — [`may_have_changed_the_helper`] says why.
    async fn learn_installed_helper(&self) {
        let Some(installed) = self.candidates.installed.as_deref() else {
            return;
        };

        if !installed.is_file() {
            return;
        }

        if let Err(error) = mixengine_core::elevation::helper(
            &self.candidates.program,
            self.candidates.installed.as_deref(),
        ) {
            tracing::warn!(
                %error,
                "the installed helper is not one this daemon will run as an administrator"
            );
            return;
        }

        let facts = crate::helper::handshake(installed, &self.home, &self.elevate).await;

        if let Some(facts) = &facts {
            tracing::debug!(
                helper = %facts.version,
                protocol = facts.speaks.0,
                "the installed privileged helper answered a probe"
            );
        }

        if let Ok(mut held) = self.facts.lock() {
            *held = facts;
        }
    }

    /// Ask for the hosts file to say what this home's sites say it should — roadmap task **T41**.
    ///
    /// **The disk is read before a prompt is spent** (T41 design, D11). A machine that already
    /// agrees needs nothing, and enqueueing anyway would put a row on `mix status` whose only
    /// possible outcome is `AlreadyDone`.
    ///
    /// **Here rather than on `Sites`** because this object already holds the `Host` and already owns
    /// the "is this worth a prompt" question. `Sites` gains one dependency and three call sites —
    /// after a successful `create`, `update` and `delete`, and never before, so a failed create asks
    /// for nothing.
    ///
    /// A read that fails does not stop the operation being queued: the helper is the authority on
    /// what is in that file, and it will refuse with the reason on the screen T64 built.
    ///
    /// **What the block should hold depends on which TLDs this machine routes here** — roadmap task
    /// **T45**, design D6. A TLD the resolver sends to our server is answered by pattern, so a hosts
    /// entry under it adds nothing; a TLD nothing routes has no other mechanism. Asking for a block
    /// without the routed names is also what clears one an earlier, unwired daemon left behind —
    /// skipping the queue instead would leave those stale names resolving to loopback for ever.
    ///
    /// **Per TLD rather than per mode**, which is T45's correction to T44: T44 computed the whole
    /// block from one home-wide `DnsMode`, which was right only while nothing could be wired at all.
    /// Every mechanism there is scopes to one TLD, and `.local` is never routed — so a home with
    /// both `blog.test` and `shop.local` needs a block with exactly one line in it.
    ///
    /// # Errors
    ///
    /// The wire error of a home whose sites cannot be read, or whose row cannot be written.
    pub(crate) async fn require_hosts(&self) -> Result<(), Error> {
        let desired = mixengine_core::hosts::desired(&self.store, &self.dns.wired())
            .await
            .map_err(|error| error.to_wire())?;

        let wanted = PrivilegedOp::hosts_apply(desired);

        // Compared as operations rather than as lists, so the ordering and deduplication are
        // `hosts_apply`'s in both directions and there is one definition of "the same block".
        match self.host.hosts_file().managed() {
            // Not a pattern guard: `present` is a `Vec` and a guard cannot move out of one.
            Ok(present) if PrivilegedOp::hosts_apply(present.clone()) == wanted => {
                // Nothing to ask for — and nothing to keep asking for either (T179).
                self.no_longer_needed(&wanted.dedupe_key()).await
            }
            Ok(_) => self.enqueue_needing_a_helper(&wanted).await,
            Err(error) => {
                tracing::warn!(
                    %error,
                    "the hosts file cannot be read; asking for permission to write it anyway"
                );

                self.enqueue_needing_a_helper(&wanted).await
            }
        }
    }

    /// Ask for this machine to send its managed TLDs to this daemon's DNS server — roadmap task
    /// **T45**.
    ///
    /// **Called at every daemon start, beside [`require_port_access`](Self::require_port_access),
    /// and that ordering is what makes M4's promise true.** On a fresh home this queues the
    /// operation *before any site exists*, so the single grant of first-run setup wires the machine;
    /// from then on `site.create` computes a hosts block that already matches the disk, enqueues
    /// nothing and prompts for nothing.
    ///
    /// Asking after the first site is created gets that wrong in a way that is invisible until it is
    /// counted: the block would already hold that site's line, emptying it is a second operation,
    /// and a second operation is a second prompt — which is the acceptance criterion this phase is
    /// measured against.
    ///
    /// **A machine with no DNS server of its own asks for nothing**, because there would be nothing
    /// to route names to; and a machine with no scoped mechanism asks for nothing either, which is
    /// a Linux without systemd and is a mode rather than a failure (the T45 design, D2).
    ///
    /// A probe that fails asks for nothing, as `require_port_access` does and unlike
    /// [`require_hosts`](Self::require_hosts): there the helper is the authority on the file and
    /// will refuse with a reason on the screen T64 built, and here a probe that could not read the
    /// machine has said nothing about what to ask for.
    ///
    /// # Errors
    ///
    /// The wire error of a row that could not be written.
    pub(crate) async fn require_resolver(&self) -> Result<(), Error> {
        let Some(port) = self.dns.wirable_port() else {
            tracing::debug!(
                "no DNS server is answering on a port a resolver could be pointed at, so nothing \
                 is asked for"
            );
            return Ok(());
        };

        let want: Vec<&str> = mixengine_proto::domains::WIRED_TLDS.to_vec();

        let state = match self.host.resolver().probe(&want, port) {
            Ok(state) => state,
            Err(error) => {
                tracing::warn!(
                    %error,
                    "this machine's resolver cannot be read; asking for nothing"
                );
                return Ok(());
            }
        };

        match state.plan(&want, port) {
            Some(plan) => {
                self.enqueue_needing_a_helper(
                    &mixengine_proto::privileged::PrivilegedOp::ResolverApply { plan },
                )
                .await
            }
            // The machine already routes what it should, so a row queued earlier is stale (T179).
            None => self.no_longer_needed("resolver").await,
        }
    }

    /// Ask this machine to trust MixEngine's own certificate authority — roadmap task **T49a**.
    ///
    /// **Called at every daemon start, immediately after the authority is made.** Reading a trust
    /// store costs no privilege on any of the three systems — measured by
    /// `mixengine-platform/tests/trust.rs` in CI's ordinary job, not assumed here — which is what
    /// makes asking on every start affordable, and what notices a store an operating-system update
    /// or another account cleared.
    ///
    /// **And here rather than when the first HTTPS site is created**, which is the ordering T48
    /// generated the authority for: `docs/architecture/security-model.md` promises one elevation
    /// prompt at first run covering the CA, the resolver and the port grant together, and an install
    /// that first appeared with the first site would be a second batch behind a second prompt.
    ///
    /// `None` is a home whose authority could not be made or read: nothing is asked for, because
    /// there is nothing to ask about.
    ///
    /// A probe that fails asks for nothing, as [`require_resolver`](Self::require_resolver) does and
    /// unlike [`require_hosts`](Self::require_hosts): there the helper is the authority on the file
    /// and refuses with a reason on the screen T64 built, and here a store that could not be read has
    /// said nothing about what to ask for.
    ///
    /// # Errors
    ///
    /// The wire error of a row that could not be written.
    pub(crate) async fn require_trust_store(&self, der: Option<&[u8]>) -> Result<(), Error> {
        let Some(der) = der else {
            tracing::debug!("this home has no certificate authority, so nothing is asked to trust");

            // Nothing to trust means nothing to ask about, including what was asked before (T179).
            return self.no_longer_needed("trust-store").await;
        };

        let state = match self.host.trust_store().probe(der) {
            Ok(state) => state,
            Err(error) => {
                tracing::warn!(
                    %error,
                    "this machine's trust store cannot be read; asking for nothing"
                );
                return Ok(());
            }
        };

        match state.plan(der) {
            Some(plan) => {
                self.enqueue_needing_a_helper(
                    &mixengine_proto::privileged::PrivilegedOp::TrustCaInstall { plan },
                )
                .await
            }
            // This machine already trusts it (T179).
            None => self.no_longer_needed("trust-store").await,
        }
    }

    /// The ports a site is reached on. Fixed, per the T42 design, D2: a front end renumbered to 81
    /// is not a front end anybody asked for, and the recipes say so too.
    pub(crate) const ANSWERING: [u16; 2] = [80, 443];

    /// Ask for this machine to let `binary` answer on 80 and 443 — roadmap task **T42**.
    ///
    /// **Called at every daemon start, and that is also the re-probe the roadmap asks for.** A
    /// capability is cleared by any write to the binary, so an update loses it; asking here catches
    /// that, and catches a loss that was not an update, and needs no hook in the updater. What makes
    /// it affordable is that reading the grant back costs one `getxattr` and no privilege at all —
    /// measured, not assumed.
    ///
    /// `None` is a home with no front end: nothing is asked for. **And nothing is ever revoked
    /// here** — the T42 design, D12: on Linux the question needs the binary, which is precisely what
    /// a home with no front end cannot supply, so "no row, therefore withdraw" is a question this
    /// system cannot be asked. Uninstall (T87) is the producer that can.
    ///
    /// A probe that fails asks for nothing, unlike [`require_hosts`](Self::require_hosts): there the
    /// helper is the authority on the file and will refuse with a reason on the screen T64 built,
    /// and here a probe that could not read one attribute has told us nothing about what to ask for.
    ///
    /// # Errors
    ///
    /// The wire error of a row that could not be written.
    pub(crate) async fn require_port_access(&self, binary: Option<&Path>) -> Result<(), Error> {
        let Some(binary) = binary else {
            tracing::debug!("this home has no front end, so nothing needs to answer on 80 or 443");
            return Ok(());
        };

        let state = match self.host.port_access().probe(binary, &Self::ANSWERING) {
            Ok(state) => state,
            Err(error) => {
                tracing::warn!(
                    %error,
                    binary = %binary.display(),
                    "cannot tell whether this machine will let the front end answer on 80 and 443"
                );

                return Ok(());
            }
        };

        if state.granted {
            // The binary already answers, so the grant nobody answered is stale. Withdrawing the
            // request is not revoking the capability: the machine keeps what it holds (T179).
            return self.no_longer_needed("port-access").await;
        }

        // Derived from the method rather than from a `#[cfg]`, which is the whole reason
        // `PortAccessState` carries one — the T42 design, D1.
        let Some(plan) = state.plan(binary) else {
            return self.no_longer_needed("port-access").await;
        };

        if let Some(missing) = &state.missing {
            tracing::info!(%missing, "asking for permission to answer on 80 and 443");
        }

        self.enqueue_needing_a_helper(&PrivilegedOp::PortAccessGrant { plan })
            .await
    }

    /// Everything that has to be true before a prompt can be raised, and the one slot, taken.
    ///
    /// Extracted from [`grant`](Self::grant) so that [`grant_within`](Self::grant_within) makes the
    /// same checks in the same order. **The order is the point**: the queue and the helper are read
    /// *before* the slot is taken, so a machine that cannot prompt is told so without a job row
    /// being written and immediately failed.
    ///
    /// # Errors
    ///
    /// As [`grant`](Self::grant), which is the whole of what this decides.
    async fn preflight(
        &self,
        only: Option<&[mixengine_proto::PendingOpId]>,
    ) -> Result<(Vec<mixengine_proto::PendingOp>, Chosen), Error> {
        let mut waiting = mixengine_core::elevation::pending(&self.store)
            .await
            .map_err(|error| error.to_wire())?;

        // T182b, D6: a caller that queued its own operations raises the prompt over those alone.
        if let Some(only) = only {
            waiting.retain(|pending| only.contains(&pending.id));
        }

        if waiting.is_empty() {
            return Err(Error::new(
                ErrorCode::PreconditionFailed,
                "nothing is waiting for permission",
            )
            .with_hint("`mix elevation status` lists what would be asked for"));
        }

        let helper = self.choose_for(&waiting).await?;

        if let Some(reason) = self.reason() {
            return Err(Error::new(
                ErrorCode::PrivilegedRequired,
                format!("this machine cannot raise an elevation prompt: {reason}"),
            ));
        }

        self.reserve()?;

        Ok((waiting, helper))
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
        let (waiting, helper) = self.preflight(None).await?;

        self.flush(handle, helper, waiting, Afterwards::Reconcile)
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
    ) -> Result<serde_json::Value, Error> {
        let (waiting, helper) = self.preflight(Some(ids)).await?;

        self.flush(handle, helper, waiting, Afterwards::Nothing)
            .await
    }

    /// `elevation.grant` — spend one prompt on everything that is waiting.
    ///
    /// **A job, and the exception `service.start` earns does not transfer.** What this waits on is a
    /// person reading a dialog: `Elevation::run` blocks with no deadline, and there is no declared
    /// ready timeout to bound it with. So the row exists the moment a client asks, and the work runs
    /// on `spawn_blocking` exactly as the trait's own documentation anticipates.
    ///
    /// **Cancellation is checked before the prompt and after it, and never during.** A cancellation
    /// token cannot close a UAC dialog, and pretending otherwise would report a job as cancelled
    /// while the person at the machine was still looking at a prompt with MixEngine's name on it.
    ///
    /// # Errors
    ///
    /// `precondition_failed` when nothing is waiting — the helper refuses an empty batch outright,
    /// so raising a prompt to discover that would be a dialog for nothing. `dependency_missing` when
    /// there is no helper beside this daemon. `privileged_required`, carrying `probe()`'s reason,
    /// when this machine cannot raise a prompt at all. `conflict`, naming the job already running,
    /// when a grant is in flight.
    pub(crate) async fn grant(self: &Arc<Self>) -> Result<JobSummary, Error> {
        let (waiting, helper) = self.preflight(None).await?;

        let elevation = Arc::clone(self);
        let started = self
            .jobs
            .begin(
                &JobKind::parse(rpc::method::ELEVATION_GRANT).expect("a valid kind"),
                move |handle| async move {
                    elevation
                        .flush(&handle, helper, waiting, Afterwards::Reconcile)
                        .await
                },
            )
            .await;

        match started {
            Ok(summary) => {
                // Only while the slot is still `Reserved`: the work may already have finished and
                // released it, and writing the id over a free slot would wedge every later grant.
                let mut state = self
                    .state
                    .lock()
                    .expect("the elevation slot is not held across an await");
                if state.slot == Slot::Reserved {
                    state.slot = Slot::Running(summary.id);
                }

                Ok(summary)
            }
            Err(error) => {
                self.release();
                Err(error)
            }
        }
    }

    /// Take the one grant slot, or say who has it.
    fn reserve(&self) -> Result<(), Error> {
        let mut state = self
            .state
            .lock()
            .expect("the elevation slot is not held across an await");

        match state.slot {
            Slot::Free => {
                state.slot = Slot::Reserved;
                Ok(())
            }
            Slot::Reserved => Err(Error::new(
                ErrorCode::Conflict,
                "a grant is already starting",
            )),
            Slot::Running(job) => Err(Error::new(
                ErrorCode::Conflict,
                format!("job {job} is already asking for permission"),
            )
            .with_hint(format!(
                "`mix job wait {job}` follows the one that is running"
            ))),
        }
    }

    /// Give the slot back.
    fn release(&self) {
        self.state
            .lock()
            .expect("the elevation slot is not held across an await")
            .slot = Slot::Free;
    }

    /// The work: write the batch, raise the one prompt, apply what came back.
    async fn flush(
        &self,
        handle: &crate::jobs::JobHandle,
        helper: Chosen,
        mut waiting: Vec<mixengine_proto::PendingOp>,
        afterwards: Afterwards,
    ) -> Result<serde_json::Value, Error> {
        // Released however this ends — including through a panic the RPC layer contains, which is
        // the whole reason it is not a line at the bottom.
        let _slot = Released(self);

        // **The helper's own operation first** — the T182b design, D2. The batch is run by the
        // helper installed when it starts, so anything after a replacement would still be answered
        // by the old one. A stable sort keeps every other operation in the order it was queued.
        put_the_helper_first(&mut waiting);

        if handle.is_cancelled() {
            return Err(Error::new(
                ErrorCode::Conflict,
                "the grant was cancelled before any prompt was raised",
            ));
        }

        // A fresh single-use directory per grant: `response.json`'s existence is the anti-replay
        // check, so a directory that has been answered is finished (T40/D10).
        let directory = self.elevate.join(
            mixengine_platform::generate_secret(16)
                .map_err(|error| mixengine_core::Error::Platform(error).to_wire())?,
        );

        // The lower of what this daemon speaks and what the installed helper answered its handshake
        // with — roadmap task T88a, and `speaks` is where that is decided.
        let request = mixengine_core::elevation::write_request(
            &directory,
            &self.home,
            &waiting,
            helper.speaks,
        )
        .map_err(|error| error.to_wire())?;

        handle.progress(20, "asking for permission").await;

        let path = request.path().to_path_buf();
        let machine = Arc::clone(&self.host);
        let raised =
            tokio::task::spawn_blocking(move || machine.elevation().run(&helper.path, &path))
                .await
                .map_err(|join| {
                    Error::new(
                        ErrorCode::Internal,
                        format!("the elevation prompt could not be waited on: {join}"),
                    )
                })?;

        let answer = self.judge(handle, &request, raised, &waiting).await;

        // **D8.** The machine may have just been wired, and a daemon that only learned that at its
        // next start would go on writing hosts entries while the user watched their grant do
        // nothing. Unconditional rather than "only if a resolver operation was in the batch": the
        // helper is the authority on what it applied, and a re-read costs one file or one registry
        // key.
        self.dns.reprobe(self.host.as_ref());

        // And the block a hosts-only home accumulated is now redundant, so it is cleared by the
        // grant that made it so rather than by a second prompt a week later. A failure here is
        // logged and not returned: the grant itself succeeded, and reporting it as failed because
        // the follow-up could not be queued would be a worse answer than the truth.
        //
        // **Not after an uninstall's grant** (T182b, D6): that prompt takes MixEngine off the
        // machine, and asking again for the block a site still wants would put back what it removed.
        if afterwards == Afterwards::Reconcile
            && let Err(error) = self.require_hosts().await
        {
            tracing::warn!(%error, "the hosts block could not be reconciled after the grant");
        }

        // On every branch, including the failing ones. The directory is single-use by construction,
        // and leaving one behind would make the next grant's fresh directory the only thing keeping
        // that true — a property worth having in two places rather than one.
        //
        // **`tokio::fs` and not `std::fs`**, which is the rule the rest of this crate keeps without
        // exception: a synchronous removal here runs on the worker thread that is serving every
        // other request. It is `tokio::fs` rather than the explicit `spawn_blocking` the certificate
        // and log paths use because there is one call to move, and that is exactly what `tokio::fs`
        // is — the same hand-off, without a closure to read past.
        if let Err(error) = tokio::fs::remove_dir_all(request.directory()).await {
            tracing::warn!(
                directory = %request.directory().display(),
                %error,
                "a single-use elevation request directory could not be removed"
            );
        }

        answer
    }

    /// Turn what the prompt answered into a job result, and apply it to the queue.
    async fn judge(
        &self,
        handle: &crate::jobs::JobHandle,
        request: &mixengine_core::elevation::Request,
        raised: mixengine_platform::Result<mixengine_platform::Raised>,
        waiting: &[mixengine_proto::PendingOp],
    ) -> Result<serde_json::Value, Error> {
        let mixengine_platform::Raised { outcome, said } =
            raised.map_err(|error| mixengine_core::Error::Platform(error).to_wire())?;
        let asked = waiting.len();
        let mut problems: Vec<String> = Vec::new();

        let (applied, still_pending) = match &outcome {
            // Every row kept, and the job **succeeds**: ADR 0005 says a declined prompt is a normal
            // outcome, never an error, and a failed job would put a red line in `mix job list` for
            // somebody exercising a choice the design offers them.
            ElevationOutcome::Declined => {
                tracing::info!("the elevation prompt was declined; nothing was applied");
                (0, asked)
            }

            // Kept too, but this one is a failure: nothing was asked and nothing can be until the
            // machine changes. The reason is the answer — on Linux it is a command to type.
            ElevationOutcome::Unavailable { reason } => {
                let error = Error::new(
                    ErrorCode::PrivilegedRequired,
                    format!("this machine cannot raise an elevation prompt: {reason}"),
                );
                self.remember(handle, &outcome, 0, asked, Vec::new());
                return Err(error);
            }

            // The helper **ran**. Whether it left a report is the next question, and "no report" is
            // a real state: T40a is explicit that `Completed` does not promise a file.
            ElevationOutcome::Completed => {
                handle.progress(70, "reading what the helper did").await;

                let report = match mixengine_core::elevation::read_report(request) {
                    Ok(report) => report,
                    // What the helper said is the only account of why it left nothing — a refusal
                    // writes no file beside a request it does not trust (T166, D6). `warn`, because
                    // until now it reached nothing above `debug`.
                    Err(mixengine_core::Error::ElevateReportMissing { path, .. }) => {
                        tracing::warn!(
                            said = said.as_deref().unwrap_or(""),
                            "the elevation helper ran and left no report"
                        );
                        self.remember(handle, &outcome, 0, asked, Vec::new());
                        return Err(
                            mixengine_core::Error::ElevateReportMissing { path, said }.to_wire()
                        );
                    }
                    Err(error) => {
                        self.remember(handle, &outcome, 0, asked, Vec::new());
                        return Err(error.to_wire());
                    }
                };

                let results: Vec<_> = request
                    .ids()
                    .iter()
                    .copied()
                    .zip(report.results.iter().cloned())
                    .collect();

                let settled = mixengine_core::elevation::settle(&self.store, &results)
                    .await
                    .map_err(|error| error.to_wire())?;

                // What this home now holds, so the producer can tell a plan already applied from a
                // new one — roadmap task T180.
                self.remember_the_firewall(waiting, &results).await;

                for (id, reason) in &settled.refused {
                    tracing::warn!(
                        %id,
                        reason,
                        "a privileged operation will not succeed as written and was dropped"
                    );
                }

                tracing::info!(
                    applied = settled.applied,
                    kept = settled.failed.len(),
                    refused = settled.refused.len(),
                    elevated = report.elevated,
                    helper = report.elevate_version,
                    "an elevated batch was applied"
                );

                // A batch that installed or replaced the helper is one whose answer the daemon no
                // longer holds: the one it knows about is the one it asked at start. **Only those two
                // operations, and not "any batch that changed the machine"**, which was the first
                // draft and undid an uninstall on CI's Windows runner (2026-09-07): the batch removed
                // the audit log last, the probe that followed ran under the runner's already-elevated
                // token, and an elevated helper writes a line for what it did — so the log the batch
                // had just removed was back before the uninstall read the machine a second time, and
                // the home was kept for it. A probe is only ever spent on what could have changed the
                // thing it asks about.
                if settled.applied > 0 && may_have_changed_the_helper(waiting) {
                    self.learn_installed_helper().await;
                }

                // **The sentences, named by the operation they are about** — roadmap task
                // **T147**. Composed here rather than by a client: what an operation is called and
                // what it said are both the daemon's to know, and a client joining the two would be
                // a second place the wording lives.
                problems = settled
                    .refused
                    .iter()
                    .chain(settled.failed.iter())
                    .map(|(id, reason)| {
                        let what = waiting
                            .iter()
                            .find(|pending| pending.id == *id)
                            .map_or_else(
                                || format!("operation {id}"),
                                |pending| pending.op.name().to_owned(),
                            );

                        format!("{what} — {reason}")
                    })
                    .collect();

                (settled.applied, settled.failed.len())
            }
        };

        let grant = self.remember(handle, &outcome, applied, still_pending, problems);

        serde_json::to_value(&grant).map_err(|source| {
            Error::new(
                ErrorCode::Internal,
                format!("the grant's outcome could not be encoded: {source}"),
            )
        })
    }

    /// Record what this grant did, for `elevation.status`, and hand it back for the job's result.
    fn remember(
        &self,
        handle: &crate::jobs::JobHandle,
        outcome: &ElevationOutcome,
        applied: usize,
        still_pending: usize,
        problems: Vec<String>,
    ) -> GrantOutcome {
        let grant = GrantOutcome {
            job: handle.id(),
            at: Timestamp::from_system_time(SystemTime::now()),
            outcome: outcome.clone(),
            applied,
            still_pending,
            problems,
        };

        self.state
            .lock()
            .expect("the elevation slot is not held across an await")
            .last = Some(grant.clone());

        grant
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
        let last = self
            .state
            .lock()
            .expect("the elevation slot is not held across an await")
            .last
            .clone();

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
        if let Err(error) = mixengine_core::elevation::helper(
            &self.candidates.program,
            self.candidates.installed.as_deref(),
        ) {
            return Some(error.to_string());
        }

        match self.host.elevation().probe() {
            ElevationSupport::Available => None,
            ElevationSupport::Unavailable { reason } => Some(reason),
        }
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
mod tests {
    use super::*;

    /// T182b, D2. A batch holding a helper operation and others runs the helper's first, and keeps
    /// the rest in the order they were queued.
    #[test]
    fn the_helper_operation_goes_first_in_its_batch() {
        let pending = |id: i64, op: PrivilegedOp| mixengine_proto::PendingOp {
            id: mixengine_proto::PendingOpId(id),
            description: op.describe(),
            op,
            requested_at: Timestamp(0),
        };

        let mut waiting = vec![
            pending(1, PrivilegedOp::hosts_apply(Vec::new())),
            pending(2, PrivilegedOp::AuditLogRemove {}),
            pending(3, PrivilegedOp::HelperInstall {}),
        ];

        put_the_helper_first(&mut waiting);

        let order: Vec<i64> = waiting.iter().map(|pending| pending.id.0).collect();
        assert_eq!(order, vec![3, 1, 2]);
    }

    use mixengine_platform::mock;

    /// A registry over a temporary home, with a machine that accepts every prompt.
    ///
    /// The helper is a **file that exists** and is never run: everything in this module stops at
    /// `mock::Host`, which records the prompt and raises nothing.
    async fn registry(
        machine: mock::Host,
    ) -> (tempfile::TempDir, Arc<Elevation>, Events, Arc<mock::Host>) {
        registry_resolving(machine, crate::dns::Dns::hosts_only_for_tests()).await
    }

    /// The same, for the two tests that care which way this home resolves a name.
    async fn registry_resolving(
        machine: mock::Host,
        dns: crate::dns::Dns,
    ) -> (tempfile::TempDir, Arc<Elevation>, Events, Arc<mock::Host>) {
        let home = tempfile::tempdir().expect("a temporary home");
        let paths = Paths::new(
            home.path().to_path_buf(),
            &mixengine_core::config::PathOverrides::default(),
        );
        std::fs::create_dir_all(paths.run()).expect("the run directory");

        let program = home
            .path()
            .join(format!("mixengined{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&program, b"not run").expect("a program to be found beside");
        std::fs::write(
            home.path()
                .join(format!("mixengine-elevate{}", std::env::consts::EXE_SUFFIX)),
            b"not run",
        )
        .expect("a helper to be found beside it");

        let store = Store::open(&home.path().join("mixengine.db"))
            .await
            .expect("a fresh database migrates");
        let events = Events::new();
        let jobs = Arc::new(crate::jobs::Jobs::new(
            &store,
            events.clone(),
            tokio_util::sync::CancellationToken::new(),
        ));

        let machine = Arc::new(machine);
        let elevation = Elevation::new(
            &paths,
            &store,
            events.clone(),
            jobs,
            Arc::clone(&machine) as Arc<dyn Host>,
            Candidates {
                program,
                // **A directory this OS would install into, with nothing in it** — T85's D5, stated
                // rather than inherited. Anybody who has run the elevated suite on this machine has
                // a real one, and every assertion below is about the copy this fixture wrote beside
                // its own `mixengined`.
                //
                // `Some(a path that is not there)` and not `None`: those are two different machines.
                // `None` is one that will not name a directory at all, where nothing can be
                // installed and `require_helper` therefore asks for nothing.
                installed: Some(
                    home.path()
                        .join("system")
                        .join(format!("mixengine-elevate{}", std::env::consts::EXE_SUFFIX)),
                ),
            },
            Arc::new(dns),
        );

        (home, elevation, events, machine)
    }

    /// Put a helper at the path the fixture calls installed — roadmap task **T88d**.
    ///
    /// **The ordinary machine, and the one every test about a *producer* means.** The fixtures
    /// above describe a machine with nothing installed and a copy beside the daemon, because that
    /// is the state T85's D5 needs stated rather than inherited; on such a machine every producer
    /// now asks for the helper as well as for its own work, which would put a second row into
    /// assertions written about the first. Calling this makes the bootstrap return early, so the
    /// queue holds exactly what the producer asked for.
    ///
    /// Nothing runs the file and nothing reads who owns it: `helper` asks that only when somebody
    /// wants a path to hand a prompt, and no test that calls this does.
    fn with_an_installed_helper(home: &tempfile::TempDir) {
        let directory = home.path().join("system");
        std::fs::create_dir_all(&directory).expect("the directory this OS would install into");
        std::fs::write(
            directory.join(format!("mixengine-elevate{}", std::env::consts::EXE_SUFFIX)),
            b"not run",
        )
        .expect("an installed helper");
    }

    /// Wait for a job to end, so a test can assert on the row rather than on a race.
    async fn finished(jobs: &Arc<crate::jobs::Jobs>, job: JobId) -> mixengine_proto::JobSummary {
        jobs.wait(job, mixengine_proto::Millis(5_000))
            .await
            .expect("the job ends")
    }

    /// Three rows where this build has one operation: `Probe` through the shipped path, and two
    /// more written directly — the shape T41 will produce, and enough to prove a batch is a batch.
    async fn three_waiting(elevation: &Arc<Elevation>) {
        elevation.enqueue(&PrivilegedOp::Probe {}).await.unwrap();

        for (key, at) in [("second", 2), ("third", 3)] {
            sqlx::query(
                "INSERT INTO pending_privileged_ops (op, dedupe_key, requested_at) \
                 VALUES ('{\"op\":\"probe\"}', ?, ?)",
            )
            .bind(key)
            .bind(at)
            .execute(elevation.store.pool())
            .await
            .unwrap();
        }
    }

    #[tokio::test]
    async fn a_machine_with_nothing_waiting_is_not_degraded() {
        let (_home, elevation, _events, _machine) =
            registry(mock::Host::with_home("/tmp/mixengine")).await;

        let summary = elevation.summary().await.expect("the queue is readable");

        assert_eq!(summary.pending, 0);
        assert!(summary.can_prompt);

        let status = elevation.status().await.expect("a status");
        assert!(status.pending.is_empty());
        assert!(status.helper.is_some(), "it is beside the program");
        assert!(status.reason.is_none());
        assert!(status.last.is_none(), "this daemon has granted nothing yet");
    }

    /// D8, from the side that publishes it: the event carries the whole queue, and an enqueue that
    /// changed nothing publishes nothing at all.
    #[tokio::test]
    async fn enqueueing_announces_the_batch_and_a_repeat_announces_nothing() {
        let (_home, elevation, events, _machine) =
            registry(mock::Host::with_home("/tmp/mixengine")).await;
        let mut watching = events.subscribe();

        elevation
            .enqueue(&PrivilegedOp::Probe {})
            .await
            .expect("the row is written");

        let published = watching.next().await.expect("an event");
        let crate::api::events::Frame::Event(DaemonEvent::ElevationRequired { pending }) =
            published
        else {
            panic!("the wrong event: {published:?}")
        };
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].op, PrivilegedOp::Probe {});
        assert!(!pending[0].description.is_empty());

        elevation
            .enqueue(&PrivilegedOp::Probe {})
            .await
            .expect("a repeat is not an error");

        assert_eq!(
            elevation.summary().await.unwrap().pending,
            1,
            "one operation, however many times it was asked for"
        );
    }

    /// The other way out of a degraded mode, and the reason a decline is not a trap.
    #[tokio::test]
    async fn dropping_empties_the_queue_and_answers_with_what_is_left() {
        let (_home, elevation, _events, _machine) =
            registry(mock::Host::with_home("/tmp/mixengine")).await;

        elevation.enqueue(&PrivilegedOp::Probe {}).await.unwrap();
        let waiting = elevation.status().await.unwrap().pending;
        assert_eq!(waiting.len(), 1);

        let left = elevation
            .drop_pending(&ElevationDrop {
                op: Some(waiting[0].id),
            })
            .await
            .expect("the row goes");

        assert!(left.pending.is_empty());
        assert_eq!(elevation.summary().await.unwrap().pending, 0);
    }

    /// D6, and the reason `can_prompt` is not merely `probe()`: a machine with every mechanism in
    /// place and no helper beside the daemon cannot grant anything either, and the sentence a person
    /// needs is different in the two cases.
    #[tokio::test]
    async fn a_machine_that_cannot_prompt_says_which_half_is_missing() {
        let (_home, elevation, _events, _machine) = registry(mock::Host::unable_to_elevate(
            "/tmp/mixengine",
            "no polkit agent",
        ))
        .await;

        let status = elevation.status().await.expect("a status");

        assert!(!status.can_prompt);
        assert_eq!(status.reason.as_deref(), Some("no polkit agent"));
        assert!(
            status.helper.is_some(),
            "the helper is there; it is the prompt that is not"
        );
    }

    /// **The task line's own test**: three operations, one grant, one prompt.
    ///
    /// `docs/decisions/0005-on-demand-elevation.md` calls elevating inside a loop a defect, and
    /// this is that rule asserted rather than asserted-about. The pair the mock records is the whole
    /// claim: one prompt, on the request the daemon had just written, with the helper it resolved.
    #[tokio::test]
    async fn three_operations_are_one_prompt_on_the_request_that_was_just_written() {
        let (_home, elevation, _events, machine) =
            registry(mock::Host::with_home("/tmp/mixengine")).await;

        three_waiting(&elevation).await;
        assert_eq!(elevation.summary().await.unwrap().pending, 3);

        let started = elevation.grant().await.expect("a grant becomes a job");
        finished(&elevation.jobs, started.id).await;

        let raised = machine.prompts_raised();
        assert_eq!(raised.len(), 1, "three operations, one prompt: {raised:?}");
        assert_eq!(raised[0].request.file_name().unwrap(), "request.json");
        assert!(
            raised[0]
                .helper
                .ends_with(format!("mixengine-elevate{}", std::env::consts::EXE_SUFFIX)),
            "{:?}",
            raised[0].helper
        );

        // The single-use directory is removed however the grant ended — `response.json`'s existence
        // is the anti-replay check, and a directory left behind would make the *next* grant's fresh
        // one the only thing keeping that true.
        assert!(!raised[0].request.exists());
    }

    /// The mock raises nothing and writes nothing, which makes "the helper ran and left no report"
    /// the default here rather than a case somebody had to think to write. T40a is explicit that
    /// `Completed` does not promise a file: a crash is not a per-OS event.
    #[tokio::test]
    async fn a_helper_that_left_no_report_fails_the_job_and_keeps_every_row() {
        let (_home, elevation, _events, _machine) =
            registry(mock::Host::with_home("/tmp/mixengine")).await;
        elevation.enqueue(&PrivilegedOp::Probe {}).await.unwrap();

        let started = elevation.grant().await.unwrap();
        let ended = finished(&elevation.jobs, started.id).await;

        assert_eq!(ended.state, mixengine_proto::JobState::Failed);
        assert_eq!(
            elevation.summary().await.unwrap().pending,
            1,
            "nothing was reported, so nothing may be assumed applied"
        );
    }

    /// T166: a helper that refused its request writes nothing beside it, and what it said on stderr
    /// is the only account of why. It has to reach the job's error, where the person who has just
    /// typed a password reads it — not stop at a `debug` line.
    #[tokio::test]
    async fn a_helper_that_left_no_report_says_why_in_the_error() {
        let said = "mixengine-elevate: cannot read /Volumes/SSD/home/run/elevate/x/request.json: \
                    Operation not permitted (os error 1)";
        let (_home, elevation, _events, _machine) =
            registry(mock::Host::elevation_saying("/tmp/mixengine", said)).await;
        elevation.enqueue(&PrivilegedOp::Probe {}).await.unwrap();

        let started = elevation.grant().await.unwrap();
        let ended = finished(&elevation.jobs, started.id).await;

        let Some(mixengine_proto::JobOutcome::Failed { error }) = ended.outcome else {
            panic!("a grant with no report fails: {:?}", ended.outcome);
        };
        assert!(error.message.ends_with(said), "{}", error.message);
        assert!(
            error.message.contains("left no report beside"),
            "{}",
            error.message
        );
        assert_eq!(elevation.summary().await.unwrap().pending, 1);
    }

    /// ADR 0005: **a declined prompt is a normal outcome, never an error.** A failed job would put a
    /// red line in `mix job list` for a person exercising a choice the design offers them.
    #[tokio::test]
    async fn a_decline_leaves_the_queue_alone_and_the_job_succeeds() {
        let (_home, elevation, _events, _machine) =
            registry(mock::Host::declining_elevation("/tmp/mixengine")).await;
        elevation.enqueue(&PrivilegedOp::Probe {}).await.unwrap();

        let started = elevation.grant().await.unwrap();
        let ended = finished(&elevation.jobs, started.id).await;

        assert_eq!(ended.state, mixengine_proto::JobState::Succeeded);
        assert_eq!(elevation.summary().await.unwrap().pending, 1);

        let last = elevation
            .status()
            .await
            .unwrap()
            .last
            .expect("a last grant");
        assert_eq!(
            last.outcome,
            mixengine_proto::privileged::ElevationOutcome::Declined
        );
        assert_eq!(last.applied, 0);
        assert_eq!(last.still_pending, 1);

        // And the machine can still be asked: declined is not the same as impossible, which is the
        // distinction `probe()` exists to draw.
        assert!(elevation.summary().await.unwrap().can_prompt);
    }

    /// On Linux the reason is the whole `pkexec` command a person is meant to type. It is worthless
    /// if the daemon drops it, so it is asserted all the way through to `elevation.status`.
    #[tokio::test]
    async fn a_machine_that_cannot_prompt_keeps_the_reason_intact() {
        let (_home, elevation, _events, _machine) = registry(mock::Host::unable_to_elevate(
            "/tmp/mixengine",
            "no polkit agent; run: pkexec /opt/mixengine/mixengine-elevate /…",
        ))
        .await;
        elevation.enqueue(&PrivilegedOp::Probe {}).await.unwrap();

        let error = elevation
            .grant()
            .await
            .expect_err("there is no way to raise a prompt here");

        assert_eq!(error.code, mixengine_proto::ErrorCode::PrivilegedRequired);
        assert!(error.to_string().contains("pkexec"), "{error}");
        assert_eq!(elevation.summary().await.unwrap().pending, 1);

        let status = elevation.status().await.unwrap();
        assert_eq!(
            status.reason.as_deref(),
            Some("no polkit agent; run: pkexec /opt/mixengine/mixengine-elevate /…")
        );
    }

    /// D4's runtime half, asserted on the slot rather than on a race: two concurrent grants are two
    /// prompts for one queue, and refusing is the only answer that cannot become a loop.
    #[tokio::test]
    async fn a_second_grant_while_one_is_in_flight_is_refused_and_names_the_first() {
        let (_home, elevation, _events, _machine) =
            registry(mock::Host::with_home("/tmp/mixengine")).await;
        elevation.enqueue(&PrivilegedOp::Probe {}).await.unwrap();

        // Put the slot where a running grant puts it, without a grant that has to be slowed down to
        // stay there. The mock answers instantly, so racing one would be a test about scheduling.
        elevation.state.lock().unwrap().slot = Slot::Running(JobId(41));

        let error = elevation
            .grant()
            .await
            .expect_err("one prompt at a time, for one queue");

        assert_eq!(error.code, mixengine_proto::ErrorCode::Conflict);
        assert!(error.to_string().contains("41"), "{error}");

        // And a slot left free is a slot a grant can take, or one failed grant would wedge the
        // daemon for as long as it runs.
        elevation.state.lock().unwrap().slot = Slot::Free;
        let started = elevation.grant().await.expect("the slot was released");
        finished(&elevation.jobs, started.id).await;
        assert_eq!(elevation.state.lock().unwrap().slot, Slot::Free);
    }

    /// An empty queue asks for nothing. The helper refuses an empty batch outright, so spending a
    /// prompt to find that out would be a dialog raised for no reason at all.
    #[tokio::test]
    async fn granting_an_empty_queue_raises_nothing() {
        let (_home, elevation, _events, machine) =
            registry(mock::Host::with_home("/tmp/mixengine")).await;

        let error = elevation.grant().await.expect_err("nothing is waiting");

        assert_eq!(error.code, mixengine_proto::ErrorCode::PreconditionFailed);
        assert!(machine.prompts_raised().is_empty());
    }

    /// D9 and D11's second row: no helper beside the daemon is a different sentence from no way to
    /// prompt, and it is answered before anything is written.
    #[tokio::test]
    async fn granting_without_a_helper_beside_the_daemon_is_dependency_missing() {
        let (home, elevation, _events, machine) =
            registry(mock::Host::with_home("/tmp/mixengine")).await;
        elevation.enqueue(&PrivilegedOp::Probe {}).await.unwrap();

        std::fs::remove_file(
            home.path()
                .join(format!("mixengine-elevate{}", std::env::consts::EXE_SUFFIX)),
        )
        .expect("the helper goes");

        let error = elevation
            .grant()
            .await
            .expect_err("there is nothing to run");

        assert_eq!(error.code, mixengine_proto::ErrorCode::DependencyMissing);
        assert!(machine.prompts_raised().is_empty());
        assert_eq!(elevation.summary().await.unwrap().pending, 1);
    }

    /// D11: the machine already says what the database says it should, so nothing is queued. A row
    /// here would put an operation on `mix status` whose only possible outcome is `AlreadyDone`.
    #[tokio::test]
    async fn a_machine_that_already_agrees_is_not_asked_for_permission() {
        let (_home, elevation, _events, _machine) = registry(mock::Host::with_hosts(
            "/tmp/mixengine",
            ["127.0.0.1 blog.test"],
        ))
        .await;
        a_site_named(&elevation.store, "blog.test").await;

        elevation.require_hosts().await.unwrap();

        assert!(
            mixengine_core::elevation::pending(&elevation.store)
                .await
                .unwrap()
                .is_empty(),
            "nothing to do is not something to ask about"
        );
    }

    /// And when it disagrees, exactly one operation is waiting and one event was published.
    #[tokio::test]
    async fn a_machine_that_disagrees_is_asked_once() {
        let (home, elevation, events, _machine) =
            registry(mock::Host::with_hosts("/tmp/mixengine", [])).await;
        with_an_installed_helper(&home);
        let mut watching = events.subscribe();
        a_site_named(&elevation.store, "blog.test").await;

        elevation.require_hosts().await.unwrap();

        let waiting = mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap();
        assert_eq!(waiting.len(), 1);
        assert!(waiting[0].description.contains("blog.test"), "{waiting:?}");

        assert!(matches!(
            watching.next().await,
            Some(crate::api::events::Frame::Event(
                DaemonEvent::ElevationRequired { .. }
            ))
        ));
    }

    /// **The seam T44 built, in both directions** — the T44 design, D4.
    ///
    /// A home on the hosts file asks for the entry its sites declare. A home on DNS asks for an
    /// *empty* block, which is not the same as asking for nothing: the server answers the whole
    /// managed TLD by pattern, so an entry adds nothing, and the operation is what clears the names
    /// the other mode wrote. Skipping the queue would leave them resolving to loopback for ever.
    ///
    /// This is the test that stops D4 being tidied into "if DNS is on, do nothing".
    #[tokio::test]
    async fn a_home_on_dns_asks_for_an_empty_block_rather_than_for_nothing() {
        let (home, elevation, _events, _machine) = registry_resolving(
            mock::Host::with_hosts("/tmp/mixengine", ["127.0.0.1 blog.test"]),
            crate::dns::Dns::wired_for_tests(),
        )
        .await;
        with_an_installed_helper(&home);
        a_site_named(&elevation.store, "blog.test").await;

        elevation.require_hosts().await.unwrap();

        let waiting = mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap();

        assert_eq!(
            waiting.len(),
            1,
            "the block the machine holds is not the block a DNS home wants"
        );
        assert_eq!(
            waiting[0].op,
            PrivilegedOp::hosts_apply(Vec::new()),
            "a home on DNS wants no managed names in that file"
        );
    }

    /// The same home, resolving the way every machine does until T45: the entry is asked for.
    #[tokio::test]
    async fn a_home_on_the_hosts_file_asks_for_the_names_its_sites_declare() {
        let (home, elevation, _events, _machine) = registry_resolving(
            mock::Host::with_hosts("/tmp/mixengine", []),
            crate::dns::Dns::hosts_only_for_tests(),
        )
        .await;
        with_an_installed_helper(&home);
        a_site_named(&elevation.store, "blog.test").await;

        elevation.require_hosts().await.unwrap();

        let waiting = mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap();

        assert_eq!(waiting.len(), 1);
        assert!(waiting[0].description.contains("blog.test"), "{waiting:?}");
    }

    /// D2 asserted rather than described: two sites before anybody clicks Allow are one row holding
    /// the *second* state, and one event per change rather than one row per change.
    #[tokio::test]
    async fn two_sites_before_a_grant_are_one_row_holding_the_second_state() {
        let (home, elevation, events, _machine) =
            registry(mock::Host::with_hosts("/tmp/mixengine", [])).await;
        with_an_installed_helper(&home);
        let mut watching = events.subscribe();

        a_site_named(&elevation.store, "blog.test").await;
        elevation.require_hosts().await.unwrap();

        a_site_named(&elevation.store, "shop.test").await;
        elevation.require_hosts().await.unwrap();

        let waiting = mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap();
        assert_eq!(waiting.len(), 1, "{waiting:?}");
        assert!(waiting[0].description.contains("blog.test"), "{waiting:?}");
        assert!(waiting[0].description.contains("shop.test"), "{waiting:?}");

        for expected in ["blog.test", "shop.test"] {
            let published = watching.next().await.expect("an event");
            let crate::api::events::Frame::Event(DaemonEvent::ElevationRequired { pending }) =
                published
            else {
                panic!("the wrong event: {published:?}")
            };
            assert!(pending[0].description.contains(expected), "{pending:?}");
        }

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), watching.next())
                .await
                .is_err(),
            "and no third event"
        );
    }

    /// A read that fails is not a reason to refuse a site. The helper is the authority on what is in
    /// that file, and it will say so on the screen T64 built — a better place for "your hosts file
    /// has two BEGIN markers" than a site creation's error.
    #[tokio::test]
    async fn a_hosts_file_that_cannot_be_read_is_still_asked_about() {
        let (home, elevation, _events, _machine) = registry(
            mock::Host::unable_to_read_the_hosts_file("/tmp/mixengine", "two BEGIN markers"),
        )
        .await;
        with_an_installed_helper(&home);
        a_site_named(&elevation.store, "blog.test").await;

        elevation.require_hosts().await.unwrap();

        assert_eq!(
            mixengine_core::elevation::pending(&elevation.store)
                .await
                .unwrap()
                .len(),
            1
        );
    }

    /// A project and a site holding one domain, written straight into the store.
    ///
    /// What this suite is about is the producer's decision; `Sites` has its own tests, and going
    /// through the API here would put two things under one assertion.
    async fn a_site_named(store: &Store, domain: &str) {
        // One project per call, because a root is unique and two sites under one root would be a
        // second thing this fixture had to decide.
        let root = std::env::temp_dir().join(format!("mixengine-t41-{domain}"));

        let project = mixengine_core::projects::create(
            store,
            &mixengine_core::projects::Registration {
                name: domain.replace('.', "-"),
                root,
                pins: std::collections::BTreeMap::new(),
            },
            Timestamp::from_system_time(std::time::SystemTime::UNIX_EPOCH),
        )
        .await
        .expect("a project");

        mixengine_core::sites::create(
            store,
            &mixengine_core::sites::NewSite {
                owner: mixengine_core::sites::SiteOwner::Project(project.id),
                doc_root: String::new(),
                kind: mixengine_proto::SiteKind::Static,
                https_enabled: true,
                https_redirect: false,
                domains: vec![domain.to_owned()],
                services: Vec::new(),
                routes: Vec::new(),
            },
        )
        .await
        .expect("a site");
    }

    /// D7: a machine that needs a grant and has not got one leaves exactly one row in the queue and
    /// announces it once.
    #[tokio::test]
    async fn a_front_end_on_a_machine_with_no_grant_asks_for_one() {
        let (home, elevation, events, _machine) = registry(mock::Host::without_port_access(
            "/tmp/mixengine",
            mixengine_platform::PortAccessMethod::Capability,
            "the binary holds no capability",
        ))
        .await;
        with_an_installed_helper(&home);
        let mut watching = events.subscribe();

        elevation
            .require_port_access(Some(Path::new("/packages/caddy/caddy")))
            .await
            .unwrap();

        let waiting = mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap();
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].op.name(), "port-access-grant");
        assert!(matches!(
            watching.next().await,
            Some(crate::api::events::Frame::Event(
                DaemonEvent::ElevationRequired { .. }
            ))
        ));

        // A second start adds no second row: the dedupe key is the kind, and the state has not
        // changed — the T41 design, D2, which this operation reuses unchanged.
        elevation
            .require_port_access(Some(Path::new("/packages/caddy/caddy")))
            .await
            .unwrap();

        assert_eq!(
            mixengine_core::elevation::pending(&elevation.store)
                .await
                .unwrap()
                .len(),
            1
        );
    }

    /// A machine that already allows it spends no prompt, which is the whole reason the disk is read
    /// before the queue is written.
    #[tokio::test]
    async fn a_machine_that_already_allows_it_asks_for_nothing() {
        let (_home, elevation, _events, _machine) = registry(mock::Host::with_port_access(
            "/tmp/mixengine",
            mixengine_platform::PortAccessMethod::Capability,
        ))
        .await;

        elevation
            .require_port_access(Some(Path::new("/packages/caddy/caddy")))
            .await
            .unwrap();

        assert!(
            mixengine_core::elevation::pending(&elevation.store)
                .await
                .unwrap()
                .is_empty()
        );
    }

    /// A home with no front end has no binary to name, and on Linux the question cannot even be
    /// asked without one — D12's reason for the producer being one-directional.
    #[tokio::test]
    async fn a_home_with_no_front_end_asks_for_nothing() {
        let (_home, elevation, _events, _machine) = registry(mock::Host::without_port_access(
            "/tmp/mixengine",
            mixengine_platform::PortAccessMethod::Capability,
            "the binary holds no capability",
        ))
        .await;

        elevation.require_port_access(None).await.unwrap();

        assert!(
            mixengine_core::elevation::pending(&elevation.store)
                .await
                .unwrap()
                .is_empty()
        );
    }

    /// T85's D6, the row that queues: a helper shipped beside the daemon and none installed.
    ///
    /// The fixture's `Candidates::installed` is `None`, so this states the machine rather than
    /// inheriting whichever one the tests are running on.
    #[tokio::test]
    async fn a_machine_with_no_installed_helper_is_asked_to_have_one() {
        let (_home, elevation, _events, _machine) =
            registry(mock::Host::with_home("/tmp/mixengine")).await;

        elevation
            .require_helper()
            .await
            .expect("the row is written");

        let pending = mixengine_core::elevation::pending(&elevation.store)
            .await
            .expect("the queue reads back");

        assert_eq!(
            pending
                .iter()
                .filter(|op| op.op == PrivilegedOp::HelperInstall {})
                .count(),
            1,
            "{pending:?}"
        );
    }

    /// And asking twice is still one row.
    ///
    /// Worth its own test because this is the first operation whose dedupe key is a bare name with
    /// no data behind it: `Probe`'s key is its serialisation, and every other operation's is derived
    /// from a plan. A start that asked for this on every boot and got a row each time would put a
    /// growing list on the one screen whose job is to say what a prompt will change.
    #[tokio::test]
    async fn asking_twice_for_the_helper_queues_one_operation() {
        let (_home, elevation, _events, _machine) =
            registry(mock::Host::with_home("/tmp/mixengine")).await;

        elevation.require_helper().await.expect("once");
        elevation.require_helper().await.expect("twice");

        let pending = mixengine_core::elevation::pending(&elevation.store)
            .await
            .expect("the queue reads back");

        assert_eq!(
            pending
                .iter()
                .filter(|op| op.op == PrivilegedOp::HelperInstall {})
                .count(),
            1,
            "{pending:?}"
        );
    }

    /// T88d. A machine whose helper an uninstall removed still ships a source, so the next thing
    /// that needs root puts the installation in front of itself — without the daemon restart
    /// `require_helper` alone would have needed.
    #[tokio::test]
    async fn a_producer_on_a_machine_with_no_installed_helper_asks_for_one() {
        let (_home, elevation, _events, _machine) =
            registry(mock::Host::with_hosts("/tmp/mixengine", [])).await;
        a_site_named(&elevation.store, "blog.test").await;

        elevation.require_hosts().await.unwrap();

        let waiting = mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap();

        assert_eq!(
            waiting
                .iter()
                .filter(|row| row.op == PrivilegedOp::HelperInstall {})
                .count(),
            1,
            "{waiting:?}"
        );
    }

    /// **And a machine that already agrees is still asked for nothing.** The bootstrap sits at the
    /// enqueue rather than at the top of the producer, so a site creation that needed no hosts
    /// entry raises no prompt — which is the promise `docs/architecture/security-model.md` makes
    /// about creating a site, and the reason this row is here beside the one above.
    #[tokio::test]
    async fn a_producer_that_asks_for_nothing_asks_for_no_helper_either() {
        let (_home, elevation, _events, _machine) = registry(mock::Host::with_hosts(
            "/tmp/mixengine",
            ["127.0.0.1 blog.test"],
        ))
        .await;
        a_site_named(&elevation.store, "blog.test").await;

        elevation.require_hosts().await.unwrap();

        assert!(
            mixengine_core::elevation::pending(&elevation.store)
                .await
                .unwrap()
                .is_empty(),
            "nothing to do is not something to ask about"
        );
    }

    /// Nothing installed **and** nothing to install from: a row that could never be applied is
    /// worse than the refusal `elevation.grant` already gives, so none is written.
    #[tokio::test]
    async fn a_producer_with_no_helper_anywhere_asks_for_no_install() {
        let (home, elevation, _events, _machine) =
            registry(mock::Host::with_hosts("/tmp/mixengine", [])).await;
        a_site_named(&elevation.store, "blog.test").await;

        std::fs::remove_file(
            home.path()
                .join(format!("mixengine-elevate{}", std::env::consts::EXE_SUFFIX)),
        )
        .expect("the helper goes");

        elevation.require_hosts().await.unwrap();

        let waiting = mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap();

        assert_eq!(waiting.len(), 1, "{waiting:?}");
        assert!(
            !waiting
                .iter()
                .any(|row| row.op == PrivilegedOp::HelperInstall {}),
            "{waiting:?}"
        );
    }

    /// T88d, and the other half of the rule above: a helper installation already waiting is
    /// dropped before an uninstall asks for the removal, so the batch does not install the file it
    /// is about to remove.
    ///
    /// **Here rather than in `crate::uninstall`'s own test module**, which has no fixture holding a
    /// queue and a machine — this one does, and building a second would be two answers to what a
    /// pending operation is.
    #[tokio::test]
    async fn an_uninstall_drops_a_helper_installation_that_is_already_waiting() {
        let (_home, elevation, _events, _machine) =
            registry(mock::Host::with_home("/tmp/mixengine")).await;

        elevation
            .require_helper()
            .await
            .expect("the installation is asked for");

        crate::uninstall::drop_helper_installations(&elevation)
            .await
            .expect("the row goes");

        elevation
            .enqueue(&PrivilegedOp::HelperRemove {})
            .await
            .expect("the removal is asked for");

        let waiting = mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap();

        assert_eq!(waiting.len(), 1, "{waiting:?}");
        assert_eq!(waiting[0].op, PrivilegedOp::HelperRemove {});
    }

    /// **The uninstall's own door is untouched by T88d's rule.** `crate::uninstall` enqueues
    /// directly, so a batch that removes the helper never first installs one.
    #[tokio::test]
    async fn enqueueing_directly_asks_for_no_helper() {
        let (_home, elevation, _events, _machine) =
            registry(mock::Host::with_home("/tmp/mixengine")).await;

        elevation
            .enqueue(&PrivilegedOp::HelperRemove {})
            .await
            .expect("the row is written");

        let waiting = mixengine_core::elevation::pending(&elevation.store)
            .await
            .unwrap();

        assert_eq!(waiting.len(), 1, "{waiting:?}");
        assert_eq!(waiting[0].op, PrivilegedOp::HelperRemove {});
    }

    /// D6's first row: a daemon with no helper beside it has nothing to install, so it asks for
    /// nothing. Distinct from "it is already installed", and the reason it is a row of its own is
    /// that this is the state a moved binary is in — and `elevation.grant` already tells a person
    /// about *that* where they can act on it.
    #[tokio::test]
    async fn a_daemon_with_no_helper_beside_it_asks_for_no_install() {
        let (home, elevation, _events, _machine) =
            registry(mock::Host::with_home("/tmp/mixengine")).await;

        std::fs::remove_file(
            home.path()
                .join(format!("mixengine-elevate{}", std::env::consts::EXE_SUFFIX)),
        )
        .expect("the helper goes");

        elevation.require_helper().await.expect("nothing is asked");

        assert!(
            mixengine_core::elevation::pending(&elevation.store)
                .await
                .unwrap()
                .is_empty()
        );
    }

    /// A probe that fails is not a reason to ask for a prompt, and not a reason to fail a start
    /// either. It is logged, and the machine carries on — the opposite of `require_hosts`, and
    /// deliberately: there the helper is the authority on the file's contents and can refuse with a
    /// reason on the screen; here a probe that could not read one attribute tells us nothing about
    /// what to ask for.
    #[tokio::test]
    async fn a_probe_that_fails_asks_for_nothing_and_does_not_fail() {
        let (_home, elevation, _events, _machine) =
            registry(mock::Host::unable_to_probe_port_access(
                "/tmp/mixengine",
                "this filesystem carries no extended attributes",
            ))
            .await;

        elevation
            .require_port_access(Some(Path::new("/packages/caddy/caddy")))
            .await
            .expect("a probe that failed is not an error to the caller");

        assert!(
            mixengine_core::elevation::pending(&elevation.store)
                .await
                .unwrap()
                .is_empty()
        );
    }

    /// Windows: the method says nothing is needed, so nothing is asked for even though the front end
    /// answers on 80. No `#[cfg]` anywhere above this crate is what makes that true.
    #[tokio::test]
    async fn a_machine_that_reserves_no_ports_asks_for_nothing() {
        let (_home, elevation, _events, _machine) = registry(mock::Host::with_port_access(
            "/tmp/mixengine",
            mixengine_platform::PortAccessMethod::Direct,
        ))
        .await;

        elevation
            .require_port_access(Some(Path::new("/packages/caddy/caddy.exe")))
            .await
            .unwrap();

        assert!(
            mixengine_core::elevation::pending(&elevation.store)
                .await
                .unwrap()
                .is_empty()
        );
    }

    /// The probe after a grant is spent on the two operations that write the helper's file, and on
    /// nothing else — least of all on an uninstall, whose last operation removes the audit log the
    /// probe would put back on a machine whose token is already elevated.
    #[test]
    fn only_a_batch_that_installed_or_replaced_the_helper_is_worth_a_probe() {
        let batch = |ops: &[PrivilegedOp]| -> Vec<mixengine_proto::PendingOp> {
            ops.iter()
                .enumerate()
                .map(|(index, op)| mixengine_proto::PendingOp {
                    id: mixengine_proto::PendingOpId(index as i64),
                    description: op.describe(),
                    op: op.clone(),
                    requested_at: mixengine_proto::Timestamp(0),
                })
                .collect()
        };

        assert!(may_have_changed_the_helper(&batch(&[
            PrivilegedOp::hosts_apply(vec![]),
            PrivilegedOp::HelperInstall {},
        ])));
        assert!(may_have_changed_the_helper(&batch(&[
            PrivilegedOp::HelperReplace {}
        ])));

        assert!(!may_have_changed_the_helper(&batch(&[])));
        assert!(!may_have_changed_the_helper(&batch(&[
            PrivilegedOp::hosts_apply(vec![]),
        ])));
        assert!(!may_have_changed_the_helper(&batch(&[
            PrivilegedOp::HelperRemove {},
            PrivilegedOp::AuditLogRemove {},
        ])));
    }

    /// **T179.** A site is created, deleted before anybody clicks Allow, and the row it queued goes
    /// with it: the queue never describes a home that no longer declares the name, and a grant
    /// cannot write one into the machine's hosts file.
    #[tokio::test]
    async fn a_hosts_entry_nobody_declares_any_more_leaves_the_queue() {
        let (home, elevation, events, _machine) =
            registry(mock::Host::with_hosts("/tmp/mixengine", [])).await;
        with_an_installed_helper(&home);
        let mut watching = events.subscribe();

        a_site_named(&elevation.store, "gone.local").await;
        elevation.require_hosts().await.unwrap();
        assert_eq!(
            mixengine_core::elevation::pending(&elevation.store)
                .await
                .unwrap()
                .len(),
            1
        );
        assert!(matches!(
            watching.next().await,
            Some(crate::api::events::Frame::Event(
                DaemonEvent::ElevationRequired { .. }
            ))
        ));

        sqlx::query("DELETE FROM sites")
            .execute(elevation.store.pool())
            .await
            .unwrap();
        elevation.require_hosts().await.unwrap();

        assert!(
            mixengine_core::elevation::pending(&elevation.store)
                .await
                .unwrap()
                .is_empty(),
            "the queue still holds a name this home no longer declares"
        );

        let Some(crate::api::events::Frame::Event(DaemonEvent::ElevationRequired { pending })) =
            watching.next().await
        else {
            panic!("a withdrawal publishes the queue that is left")
        };
        assert!(pending.is_empty(), "{pending:?}");
    }

    /// A machine this daemon could not read is a machine it knows nothing about, so the queue is
    /// left exactly as it was (the T179 design, D2).
    #[tokio::test]
    async fn a_hosts_file_that_cannot_be_read_withdraws_nothing() {
        let (home, elevation, _events, _machine) = registry(
            mock::Host::unable_to_read_the_hosts_file("/tmp/mixengine", "the file is not readable"),
        )
        .await;
        with_an_installed_helper(&home);

        sqlx::query(
            "INSERT INTO pending_privileged_ops (op, dedupe_key, requested_at) \
             VALUES ('{\"op\":\"probe\"}', 'hosts-apply', 1)",
        )
        .execute(elevation.store.pool())
        .await
        .unwrap();

        elevation.require_hosts().await.unwrap();

        assert_eq!(
            mixengine_core::elevation::pending(&elevation.store)
                .await
                .unwrap()
                .len(),
            1,
            "a failed reading withdrew a row"
        );
    }

    /// **T179**, for the resolver: a machine that already routes what it should has nothing waiting
    /// for it, whatever an earlier reading queued.
    #[tokio::test]
    async fn a_resolver_that_already_routes_withdraws_what_was_queued() {
        let (home, elevation, _events, _machine) = registry_resolving(
            mock::Host::with_resolver(
                "/tmp/mixengine",
                mixengine_platform::ResolverMethod::Nrpt,
                &mixengine_proto::domains::WIRED_TLDS,
            ),
            crate::dns::Dns::wired_for_tests(),
        )
        .await;
        with_an_installed_helper(&home);

        sqlx::query(
            "INSERT INTO pending_privileged_ops (op, dedupe_key, requested_at) \
             VALUES ('{\"op\":\"probe\"}', 'resolver', 1)",
        )
        .execute(elevation.store.pool())
        .await
        .unwrap();

        elevation.require_resolver().await.unwrap();

        assert!(
            mixengine_core::elevation::pending(&elevation.store)
                .await
                .unwrap()
                .is_empty(),
            "the resolver row outlived the machine agreeing"
        );
    }

    /// **T179**, for the trust store: a home with no authority has nothing to be trusted, so an
    /// install queued for an authority that is gone goes with it (the design's Settled 1).
    #[tokio::test]
    async fn a_home_with_no_authority_withdraws_a_queued_trust_install() {
        let (home, elevation, _events, _machine) =
            registry(mock::Host::with_home("/tmp/mixengine")).await;
        with_an_installed_helper(&home);

        sqlx::query(
            "INSERT INTO pending_privileged_ops (op, dedupe_key, requested_at) \
             VALUES ('{\"op\":\"probe\"}', 'trust-store', 1)",
        )
        .execute(elevation.store.pool())
        .await
        .unwrap();

        elevation.require_trust_store(None).await.unwrap();

        assert!(
            mixengine_core::elevation::pending(&elevation.store)
                .await
                .unwrap()
                .is_empty()
        );
    }

    /// **T179**, for the port grant: a binary that already holds what it needs withdraws the grant
    /// nobody answered. **Withdrawing is not revoking** — the machine keeps the capability.
    #[tokio::test]
    async fn a_binary_that_already_answers_withdraws_the_grant_nobody_answered() {
        let (home, elevation, _events, _machine) = registry(mock::Host::with_port_access(
            "/tmp/mixengine",
            mixengine_platform::PortAccessMethod::Capability,
        ))
        .await;
        with_an_installed_helper(&home);

        sqlx::query(
            "INSERT INTO pending_privileged_ops (op, dedupe_key, requested_at) \
             VALUES ('{\"op\":\"probe\"}', 'port-access', 1)",
        )
        .execute(elevation.store.pool())
        .await
        .unwrap();

        elevation
            .require_port_access(Some(Path::new("/packages/caddy/caddy")))
            .await
            .unwrap();

        assert!(
            mixengine_core::elevation::pending(&elevation.store)
                .await
                .unwrap()
                .is_empty()
        );
    }

    /// A probe that could not read the machine says nothing about what to stop asking for, so a
    /// home with no front end leaves the queue alone (the T179 design, D2).
    #[tokio::test]
    async fn a_home_with_no_front_end_withdraws_nothing() {
        let (home, elevation, _events, _machine) =
            registry(mock::Host::with_home("/tmp/mixengine")).await;
        with_an_installed_helper(&home);

        sqlx::query(
            "INSERT INTO pending_privileged_ops (op, dedupe_key, requested_at) \
             VALUES ('{\"op\":\"probe\"}', 'port-access', 1)",
        )
        .execute(elevation.store.pool())
        .await
        .unwrap();

        elevation.require_port_access(None).await.unwrap();

        assert_eq!(
            mixengine_core::elevation::pending(&elevation.store)
                .await
                .unwrap()
                .len(),
            1,
            "a home that cannot read the machine withdrew a row anyway"
        );
    }

    /// **T180.** A grant that carried a firewall plan records it, so the producer can tell a plan
    /// this machine already holds from one it does not.
    #[tokio::test]
    async fn a_settled_firewall_plan_is_recorded() {
        let (_home, elevation, _events, _machine) =
            registry(mock::Host::with_home("/tmp/mixengine")).await;

        let plan = mixengine_proto::privileged::FirewallPlan {
            ports: vec![80, 443],
            label: "MixEngine — shared sites".to_owned(),
        };
        let waiting = vec![mixengine_proto::PendingOp {
            id: mixengine_proto::PendingOpId(7),
            op: PrivilegedOp::FirewallApply { plan: plan.clone() },
            description: "open two ports".to_owned(),
            requested_at: mixengine_proto::Timestamp(1),
        }];

        for outcome in [
            OpOutcome::Applied {
                detail: "two rules".to_owned(),
            },
            OpOutcome::AlreadyDone,
            OpOutcome::Unmanaged {
                reason: "this machine has no firewall to write".to_owned(),
                manual: "open the port yourself".to_owned(),
            },
        ] {
            mixengine_core::updates::records::clear(&elevation.store, FIREWALL_APPLIED)
                .await
                .unwrap();

            elevation
                .remember_the_firewall(&waiting, &[(mixengine_proto::PendingOpId(7), outcome)])
                .await;

            let recorded: Option<mixengine_proto::privileged::FirewallPlan> =
                mixengine_core::updates::records::get(&elevation.store, FIREWALL_APPLIED)
                    .await
                    .unwrap();
            assert_eq!(recorded.as_ref(), Some(&plan));
        }
    }

    /// An operation that was refused, unsupported or failed changed nothing, so it records nothing:
    /// the next sharing change has to ask again.
    #[tokio::test]
    async fn a_firewall_plan_that_did_not_land_records_nothing() {
        let (_home, elevation, _events, _machine) =
            registry(mock::Host::with_home("/tmp/mixengine")).await;

        let plan = mixengine_proto::privileged::FirewallPlan {
            ports: vec![80],
            label: "MixEngine — shared sites".to_owned(),
        };
        let waiting = vec![mixengine_proto::PendingOp {
            id: mixengine_proto::PendingOpId(3),
            op: PrivilegedOp::FirewallApply { plan },
            description: "open one port".to_owned(),
            requested_at: mixengine_proto::Timestamp(1),
        }];

        for outcome in [
            OpOutcome::Refused {
                reason: "no".to_owned(),
            },
            OpOutcome::Unsupported {
                reason: "not here".to_owned(),
            },
            OpOutcome::Failed {
                message: "the tool said no".to_owned(),
            },
        ] {
            elevation
                .remember_the_firewall(
                    &waiting,
                    &[(mixengine_proto::PendingOpId(3), outcome.clone())],
                )
                .await;

            let recorded: Option<mixengine_proto::privileged::FirewallPlan> =
                mixengine_core::updates::records::get(&elevation.store, FIREWALL_APPLIED)
                    .await
                    .unwrap();
            assert_eq!(recorded, None, "{outcome:?} recorded a plan");
        }
    }
}
