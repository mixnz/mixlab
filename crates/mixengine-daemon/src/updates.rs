//! Checking whether a newer MixEngine exists, and putting one on this machine — roadmap task
//! **T88**.
//!
//! # The whole sequence happens here, and `mix` prompts and reconnects
//!
//! `mixengine-cli` may depend on `mixengine-platform` and `mixengine-proto` and on nothing else —
//! `workspace_layering.rs` enforces it — and verifying a signature, unpacking an archive and
//! swapping files are all `mixengine-core`'s. So the client's whole part in an update is asking,
//! waiting for the endpoint to go quiet, and starting the new daemon, which is `Autostart::run` and
//! has been there since T9.
//!
//! # Silent on failure, and that is a requirement rather than a shrug
//!
//! `docs/features/updates.md`: *an offline machine must never see an error, and never a slower
//! startup*. Both background callers here — the check at start and the 24 h clock — log at `debug!`
//! and change nothing when the network is not there, and `mixengine_core::index::Client` keeps the
//! last document it verified rather than losing it. What a person who *asked* gets is different:
//! `mix self-update` reports the transport failure, because they are standing there.
//!
//! # What this module deliberately cannot do
//!
//! Elevate. Nothing here has an elevation path and nothing here ever will: an updater that could ask
//! for root would be the local privilege-escalation vector `docs/features/updates.md` calls the
//! single most important rule on the page. A copy of MixEngine installed where this account cannot
//! write is refused in words, before a byte is downloaded — `mixengine_core::updates::placement`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Mutex;

use mixengine_core::install::{Installer, NotAnArchive, Watcher};
use mixengine_core::paths::Paths;
use mixengine_core::store::Store;
use mixengine_core::updates::{self, Feed};
use mixengine_proto::{
    Error, ErrorCode, ServiceId, Timestamp, UpdateApplied, UpdateDecision, UpdateHandedOver,
    UpdateInstaller, UpdateOffer, UpdatePlacement, UpdateRelease, UpdateStatus,
};

use crate::error::ToWire as _;

/// Where the update feed comes from, and what verifies it.
///
/// The index's mechanism verbatim — `--update-url` requires `--update-key`, and neither is read
/// below `main`. A URL that could move while the key could not would be a setting that can only ever
/// fail, since nobody but us can sign with our key.
#[derive(Debug, Clone)]
pub(crate) struct FeedSource {
    /// Where `latest.json` is.
    pub(crate) url: String,

    /// The minisign public key it is checked against.
    pub(crate) public_key: String,
}

impl Default for FeedSource {
    fn default() -> Self {
        Self {
            url: updates::DEFAULT_URL.to_owned(),
            public_key: updates::PUBLIC_KEY.to_owned(),
        }
    }
}

/// The last feed this daemon verified.
#[derive(Debug, Clone)]
struct Checked {
    /// What it said.
    feed: Feed,

    /// When this daemon read it.
    at: Timestamp,

    /// Whether that reading came out of a cache the daemon could not refresh.
    stale: bool,
}

/// One reading of the daemon binary on disk: its `(device, inode)`, and the version it reported.
type OnDisk = ((u64, u64), Option<String>);

/// A payload that is on disk and has been run once.
#[derive(Debug)]
pub(crate) struct Staged {
    /// Where the install's binaries are.
    pub(crate) directory: PathBuf,

    /// Where the payload was unpacked.
    pub(crate) staged: PathBuf,

    /// Executable name to its path inside the payload.
    pub(crate) provides: BTreeMap<String, String>,

    /// The version the feed says this payload is.
    pub(crate) to: String,
}

/// Everything `update.*` needs.
#[derive(Debug)]
pub(crate) struct Updates {
    /// Where the staging directory goes, and where the partial download lives.
    paths: Paths,

    /// Where skip, later and the restore records are kept.
    store: Store,

    /// The verified feed, cached under `cache/`.
    client: updates::Client,

    /// The download pipeline, with its partial downloads in the same place.
    installer: Installer,

    /// Whether this copy of MixEngine may replace itself, read once at start.
    ///
    /// **Once and not per call**: it is a property of how MixEngine was installed, the answer cannot
    /// change while this daemon runs without somebody having moved its binaries underneath it, and
    /// a probe on every `daemon.status` would be a file created and removed on every status line.
    placement: updates::Placement,

    /// The event stream, for [`DaemonEvent::UpdateAvailable`](mixengine_proto::DaemonEvent).
    events: crate::api::Events,

    /// The last feed read, and how.
    last: Mutex<Option<Checked>>,

    /// The versions this daemon has already announced.
    ///
    /// `certs::renewal`'s `newly` rule: a producer reports a change and not a heartbeat, and a check
    /// that runs every 24 h for a month must not spend a client's stream allowance restating one
    /// fact.
    announced: Mutex<BTreeSet<String>>,

    /// The machine, for the package receipt and Installer.app — roadmap task **T88f**.
    host: std::sync::Arc<dyn mixengine_platform::Host>,

    /// Where this daemon's own binary is, which `update.status` asks for its version while a `.pkg`
    /// is handed over (T88f, D6).
    daemon_exe: Option<PathBuf>,

    /// The last reading of that binary: its `(device, inode)` and what it said it was.
    ///
    /// **Keyed by inode, not by modification time**: Installer.app writes a new inode and keeps the
    /// time the file had in the package (the T88f readings, M3). So a status poll costs one `stat`
    /// until the file is replaced.
    installed: Mutex<Option<OnDisk>>,
}

/// A [`Watcher`] that reports to nobody.
///
/// **`update.apply` is not a job**, and that is argued rather than assumed: a job whose completion
/// is the daemon exiting is a job nothing can ever observe finishing, and `mix`'s HTTP client sets
/// no request timeout, so a call that takes two minutes cannot fail for being long. What a payload
/// of this size needs is its size printed before the fetch starts, which is what the consent prompt
/// already prints.
struct Quiet;

impl Watcher for Quiet {
    async fn report(&self, percent: u8, message: &str) {
        tracing::debug!(percent, message, "updating");
    }

    fn is_cancelled(&self) -> bool {
        false
    }
}

impl Updates {
    /// Point a feed client and an installer at `source`, caching under the home's `cache/`.
    ///
    /// `http` is the daemon's own transport, shared with the package index client and the
    /// extension registry client rather than built fresh here — roadmap task **T72b**, on
    /// `runtimes::Fetcher`'s own "one per daemon" reasoning, one layer down.
    ///
    /// # Errors
    ///
    /// The wire error of a public key that is not one, which means a broken build or an unusable
    /// `--update-key` and fails the daemon's start rather than the first call: a daemon that can
    /// never verify an update should say so while somebody is watching.
    pub(crate) fn new(
        paths: &Paths,
        store: &Store,
        source: &FeedSource,
        daemon_exe: Option<&std::path::Path>,
        host: std::sync::Arc<dyn mixengine_platform::Host>,
        events: crate::api::Events,
        http: reqwest::Client,
    ) -> Result<std::sync::Arc<Self>, Error> {
        let placement = match daemon_exe {
            Some(exe) => updates::placement::of(
                exe,
                std::env::var_os(APPIMAGE)
                    .filter(|value| !value.is_empty())
                    .as_deref(),
                // Asked first, and passed in: the receipt is the platform's to read (T88f, D1).
                host.installers().receipt_of(exe).as_deref(),
            ),
            // A daemon whose own path the operating system will not name. Refused in words rather
            // than assumed writable: this is the one field an update is not allowed to guess at.
            None => updates::Placement::Managed {
                directory: PathBuf::new(),
                because:
                    "this operating system will not say where this daemon's own binary is, so \
                          MixEngine cannot tell whether it may replace it"
                        .to_owned(),
            },
        };

        Ok(std::sync::Arc::new(Self {
            paths: paths.clone(),
            store: store.clone(),
            client: updates::Client::with_transport(
                &source.url,
                &source.public_key,
                paths.cache(),
                http,
            )
            .map_err(|error| error.to_wire())?,
            installer: Installer::new(paths.cache()).map_err(|error| error.to_wire())?,
            placement,
            events,
            last: Mutex::new(None),
            announced: Mutex::new(BTreeSet::new()),
            host,
            daemon_exe: daemon_exe.map(std::path::Path::to_path_buf),
            installed: Mutex::new(None),
        }))
    }

    /// Whether this copy of MixEngine may replace its own binaries — roadmap task **T88a**'s
    /// caller as well as T88's.
    ///
    /// A privileged helper a package manager put where it is is replaced by that package manager,
    /// on exactly the reasoning `mix self-update` already refuses one with.
    pub(crate) fn placement(&self) -> &updates::Placement {
        &self.placement
    }

    /// Which kind of package updates this copy: `pkg`, `deb` or `rpm` — the T182b design, D5.
    ///
    /// Which installer an update hands over depends on it: the headless package for a copy that has
    /// no window, the window's package otherwise. Asked of the machine each time rather than
    /// remembered, since installing the other flavour is exactly what changes the answer.
    fn installer_kind(&self) -> &'static str {
        match &self.placement {
            updates::Placement::Installer { receipt, .. } => {
                updates::placement::installer_kind(receipt).unwrap_or("pkg")
            }
            _ => "pkg",
        }
    }

    /// Is this a copy without the window?
    fn headless(&self) -> bool {
        !matches!(
            self.host.desktop_apps().locate_window(
                mixengine_core::window::EXECUTABLE,
                mixengine_core::window::BUNDLE
            ),
            Ok(mixengine_platform::Located::Installed(_))
        )
    }

    /// The privileged helper this release publishes for this machine — roadmap task **T88a**.
    ///
    /// Reads the feed the way `update.status` does, through the cache, so a machine that checked an
    /// hour ago does not fetch again for this.
    ///
    /// # Errors
    ///
    /// The wire error of a feed that could not be read with nothing cached to fall back to, and
    /// `mixengine_core::Error::HelperUnavailable` when the release published none for this pair —
    /// which is also what a release from before T88a answers, since it lists none at all.
    pub(crate) async fn published_helper(
        &self,
    ) -> Result<(String, updates::HelperArtifact), Error> {
        let catalogue = self
            .client
            .catalogue()
            .await
            .map_err(|error| error.to_wire())?;

        let (os, arch) = host()?;
        let helper = catalogue
            .index
            .helper(os, arch)
            .ok_or_else(|| {
                mixengine_core::Error::HelperUnavailable {
                    os: format!("{os:?}").to_lowercase(),
                    arch: format!("{arch:?}").to_lowercase(),
                }
                .to_wire()
            })?
            .clone();

        // The helper's own version where the feed names one (T182b, D1), and the release's where
        // it predates that, which is what the helper carried then.
        let version = helper.version.clone().unwrap_or(catalogue.index.version);

        Ok((version, helper))
    }

    /// The download pipeline, for the one caller that fetches something which is not an archive:
    /// `mixengine_core::updates::helper`, which pulls the privileged helper and its signature.
    pub(crate) fn installer(&self) -> &Installer {
        &self.installer
    }

    /// The one line `daemon.status` carries, or [`None`].
    ///
    /// Every failure is [`None`]: a settings row that will not read and a daemon that has not
    /// checked mean the same thing to a client, which is that there is nothing to show.
    pub(crate) async fn offer(&self) -> Option<UpdateOffer> {
        let checked = self.last.lock().ok()?.clone()?;
        let decision = self.decision(&checked.feed).await;

        decision.offered.then(|| UpdateOffer {
            version: checked.feed.version.clone(),
            published_at: checked.feed.published_at.to_string(),
        })
    }

    /// `update.status` — what this daemon knows, without going to the network.
    pub(crate) async fn status(&self, will_restart: Vec<ServiceId>) -> UpdateStatus {
        let current = env!("CARGO_PKG_VERSION").to_owned();
        let placement = placement(&self.placement);
        let checked = self.last.lock().ok().and_then(|last| last.clone());

        self.settle_handover(checked.as_ref().map(|checked| &checked.feed))
            .await;
        let installer = self.installer_of(checked.as_ref().map(|checked| &checked.feed));
        let installed = self.installed_version().await;

        let Some(checked) = checked else {
            return UpdateStatus {
                current,
                available: None,
                offered: false,
                because: None,
                checked_at: None,
                stale: false,
                placement,
                will_restart,
                installer,
                installed,
            };
        };

        let decision = self.decision(&checked.feed).await;

        UpdateStatus {
            current,
            available: Some(self.release(&checked.feed)),
            offered: decision.offered,
            because: decision.because,
            checked_at: Some(checked.at),
            stale: checked.stale,
            placement,
            will_restart,
            installer,
            installed,
        }
    }

    /// Read the published feed, and remember what it said.
    ///
    /// **Never an error to the caller when there is a cached document**, which is
    /// `index::Client`'s own rule: an unreachable server, a signature that does not verify and a
    /// feed offered from before the one held all mean the same thing to this call, and the last
    /// document verified is still the last document verified.
    ///
    /// # Errors
    ///
    /// The wire error of a fetch that failed with nothing cached to fall back to. The background
    /// callers swallow it; `mix self-update` prints it, because somebody asked.
    pub(crate) async fn check(
        &self,
        force: bool,
        will_restart: Vec<ServiceId>,
    ) -> Result<UpdateStatus, Error> {
        let catalogue = match force {
            true => self.client.refresh().await,
            false => self.client.catalogue().await,
        }
        .map_err(|error| error.to_wire())?;

        let version = catalogue.index.version.clone();
        let published_at = catalogue.index.published_at.to_string();
        let decision = self.decision(&catalogue.index).await;

        if let Ok(mut last) = self.last.lock() {
            *last = Some(Checked {
                feed: catalogue.index,
                at: Timestamp::from_system_time(std::time::SystemTime::now()),
                stale: catalogue.freshness.is_stale(),
            });
        }

        if decision.offered && self.newly(&version) {
            tracing::info!(%version, "a newer MixEngine has been published");

            self.events
                .publish(mixengine_proto::DaemonEvent::UpdateAvailable {
                    version,
                    published_at,
                });
        }

        Ok(self.status(will_restart).await)
    }

    /// `update.decide` — remember *skip this version* or *remind me later*.
    ///
    /// # Errors
    ///
    /// The wire error of a settings row that could not be written.
    pub(crate) async fn decide(
        &self,
        version: &str,
        decision: UpdateDecision,
        will_restart: Vec<ServiceId>,
    ) -> Result<UpdateStatus, Error> {
        match decision {
            UpdateDecision::Skip => {
                updates::records::set(&self.store, updates::records::SKIPPED_VERSION, &version)
                    .await
                    .map_err(|error| error.to_wire())?;
            }

            UpdateDecision::Later => {
                let due = Timestamp(
                    Timestamp::from_system_time(std::time::SystemTime::now()).0
                        + updates::records::LATER_SECONDS * 1_000,
                );

                updates::records::set(&self.store, updates::records::REMIND_AFTER, &due)
                    .await
                    .map_err(|error| error.to_wire())?;
            }
        }

        Ok(self.status(will_restart).await)
    }

    /// Everything an apply does before anything is stopped: check, refuse, download, verify, unpack,
    /// smoke-test.
    ///
    /// **In that order and before the stop** — the T88 design, D5. Taken literally,
    /// `docs/features/updates.md`'s *"stop → download → verify → install"* would leave a
    /// developer's database down for the length of a download, on a connection nobody promised
    /// anything about, to gain nothing: a download that fails after the stop has cost an outage, and
    /// one that succeeds could have happened while everything was still up.
    ///
    /// # Errors
    ///
    /// `precondition_failed` when `version` is no longer what the feed offers or when this copy of
    /// MixEngine is one a package manager installed; and whatever the download, the checksum, the
    /// unpacking or the smoke test reported. Every one of them leaves the installed binaries
    /// untouched.
    pub(crate) async fn stage(&self, version: &str) -> Result<Staged, Error> {
        let checked = self.checked()?;

        if checked.feed.version != version {
            return Err(mixengine_core::Error::UpdateNotOffered {
                asked: version.to_owned(),
                offered: Some(checked.feed.version.clone()),
            }
            .to_wire());
        }

        let directory = match &self.placement {
            updates::Placement::SelfUpdatable { directory } => directory,
            updates::Placement::Managed { directory, because } => {
                return Err(mixengine_core::Error::UpdateNotWritable {
                    directory: directory.clone(),
                    because: because.clone(),
                }
                .to_wire());
            }
            updates::Placement::Installer { directory, .. } => {
                return Err(mixengine_core::Error::UpdateUsesInstaller {
                    directory: directory.clone(),
                }
                .to_wire());
            }
        };

        let (os, arch) = host()?;
        let artifact = checked
            .feed
            .artifact(os, arch)
            .ok_or_else(|| {
                mixengine_core::Error::UpdateUnavailable {
                    os: format!("{os:?}").to_lowercase(),
                    arch: format!("{arch:?}").to_lowercase(),
                }
                .to_wire()
            })?
            .clone();

        let into = self.staging_for(version);
        let staged = updates::apply::stage(&self.installer, &artifact, &into, &Quiet)
            .await
            .map_err(|error| error.to_wire())?;

        Ok(Staged {
            directory: directory.clone(),
            staged,
            provides: artifact.provides,
            to: checked.feed.version,
        })
    }

    /// Write down what is about to happen, before the binaries move.
    ///
    /// Read by the daemon that comes up next — or, when the swap fails, by the rollback in this
    /// same process. That is why it is written before rather than after: both paths read it, and one
    /// of them never gets to a line that ran later.
    ///
    /// # Errors
    ///
    /// The wire error of a settings row that could not be written.
    pub(crate) async fn remember(&self, to: &str, restore: &[ServiceId]) -> Result<(), Error> {
        let applied = updates::records::Applied {
            from: env!("CARGO_PKG_VERSION").to_owned(),
            to: to.to_owned(),
            at: Timestamp::from_system_time(std::time::SystemTime::now()),
        };
        let restore: Vec<String> = restore.iter().map(|id| id.as_str().to_owned()).collect();

        updates::records::set(&self.store, updates::records::APPLIED, &applied)
            .await
            .map_err(|error| error.to_wire())?;
        updates::records::set(&self.store, updates::records::RESTORE, &restore)
            .await
            .map_err(|error| error.to_wire())
    }

    /// Undo an update that got as far as the stop and no further.
    ///
    /// **The half of the rollback that `swap` cannot do.** `swap` puts every binary it moved back
    /// under its own name; what it cannot put back is the *stop* that preceded it, and nothing else
    /// on the machine intends to. So this starts the services again and forgets the records — and it
    /// deliberately does **not** run the version check [`Updates::restore_after_update`] runs: this
    /// daemon is still the version it was, nothing was installed, and marking the release as skipped
    /// over a full disk would refuse it for ever.
    ///
    /// Failures are logged and not returned: the caller is already reporting the failure that
    /// brought it here, and a second one about the tidying up would replace the diagnosis with the
    /// housekeeping.
    pub(crate) async fn roll_back(&self, services: &crate::services::Registry) {
        let restore: Option<Vec<String>> = read(&self.store, updates::records::RESTORE).await;

        for key in [updates::records::APPLIED, updates::records::RESTORE] {
            if let Err(error) = updates::records::clear(&self.store, key).await {
                tracing::warn!(key, %error, "an abandoned update's record could not be removed");
            }
        }

        if let Some(restore) = restore {
            self.start_again(services, &restore).await;
        }
    }

    /// Complete this install from the payload it was updated from — roadmap task **T185a**,
    /// ADR 0054. Answers the names it added.
    ///
    /// **Before [`Self::restore_after_update`]**, because the staging directory is named after the
    /// version the feed declared, which only the `APPLIED` record still knows; with no record it is
    /// the running version's. The hash `complete` checks is what ties the payload to this build.
    ///
    /// Cheapest first: the placement, then a `stat` per completable name inside `complete`, then the
    /// staging directory; the hash only when all three say there is something to do. The staging
    /// directory belongs to this start (ADR 0054, 6): removed when nothing is left to do or when what
    /// failed would fail the same way again, kept only when a copy failed on the install's side so
    /// the next start can try again.
    pub(crate) async fn complete_install(&self) -> Vec<String> {
        let updates::Placement::SelfUpdatable { directory } = &self.placement else {
            return Vec::new();
        };
        let Some(running) = self.daemon_exe.clone() else {
            return Vec::new();
        };

        let applied: Option<updates::records::Applied> =
            read(&self.store, updates::records::APPLIED).await;
        let version = applied.map_or_else(
            || env!("CARGO_PKG_VERSION").to_owned(),
            |applied| applied.to,
        );
        let staged = self.staging_for(&version);

        if !staged.is_dir() {
            return Vec::new();
        }

        let (directory, from) = (directory.clone(), staged.clone());
        let completed = crate::api::on_a_blocking_thread(move || {
            Ok(updates::complete::complete(&directory, &from, &running))
        })
        .await
        .unwrap_or_default();

        for (name, why) in &completed.failed {
            tracing::warn!(
                name,
                why,
                "the install could not complete itself from its payload"
            );
        }

        if !completed.added.is_empty() {
            tracing::info!(added = ?completed.added, "the install completed itself from its payload");

            let mut recorded: Vec<String> = read(&self.store, updates::records::COMPLETED)
                .await
                .unwrap_or_default();

            for name in &completed.added {
                if !recorded.contains(name) {
                    recorded.push(name.clone());
                }
            }

            if let Err(error) =
                updates::records::set(&self.store, updates::records::COMPLETED, &recorded).await
            {
                tracing::warn!(%error, "what the install added to itself could not be recorded");
            }
        }

        if completed.failed.is_empty() || !completed.worth_retrying {
            let _ = tokio::fs::remove_dir_all(&staged).await;
        }

        completed.added
    }

    /// The pass a daemon makes at start when the one before it replaced these binaries.
    ///
    /// Four things, in this order:
    ///
    /// 1. **Read both records and delete them**, before either is acted on. A record that survived
    ///    being read is replayed by every later start — so somebody who updates, stops MariaDB
    ///    because they are done with it, and reboots would get MariaDB back for ever.
    /// 2. **Compare this build's own version with what the feed said the payload was.** This is the
    ///    first moment anything can answer that honestly, because the running binary is the only
    ///    thing that knows what it is. A mismatch writes `updates.skipped_version` and warns, so a
    ///    mislabelled release costs one pointless update instead of being offered every 24 h for
    ///    ever.
    /// 3. **Remove the `.old` files.** A daemon that is answering has proved they are not needed —
    ///    which is exactly the condition for discarding the only way back. On Windows the
    ///    `mix.exe.old` still held open by the `mix` that ran the update is left for the start after
    ///    this one.
    /// 4. **Start what was running.** In the reverse of the order it was stopped in, dependencies
    ///    first, through the same graph `service.start` uses — a client that read the list and
    ///    issued the calls itself would be deciding an order, which `CLAUDE.md` forbids.
    pub(crate) async fn restore_after_update(
        &self,
        services: &crate::services::Registry,
        replaced: &[String],
    ) {
        let applied: Option<updates::records::Applied> =
            read(&self.store, updates::records::APPLIED).await;
        let restore: Option<Vec<String>> = read(&self.store, updates::records::RESTORE).await;

        if applied.is_none() && restore.is_none() {
            return;
        }

        for key in [updates::records::APPLIED, updates::records::RESTORE] {
            if let Err(error) = updates::records::clear(&self.store, key).await {
                tracing::warn!(key, %error, "an update's record could not be removed after it was read");
            }
        }

        if let Some(applied) = &applied {
            let running = env!("CARGO_PKG_VERSION");

            if applied.to == running {
                tracing::info!(from = %applied.from, to = %applied.to, "this daemon is the update");
            } else {
                tracing::warn!(
                    expected = %applied.to,
                    running,
                    "the release that was installed is not the version its feed declared; it will \
                     not be offered again"
                );

                if let Err(error) = updates::records::set(
                    &self.store,
                    updates::records::SKIPPED_VERSION,
                    &applied.to,
                )
                .await
                {
                    tracing::warn!(%error, "a mislabelled release could not be marked as skipped");
                }
            }
        }

        if let updates::Placement::SelfUpdatable { directory } = &self.placement {
            let discarded = updates::apply::discard_old(directory, replaced);
            tracing::debug!(discarded, "removed what an update kept as its way back");
        }

        let Some(restore) = restore else {
            return;
        };

        self.start_again(services, &restore).await;
    }

    /// Start the services an update stopped, dependencies first.
    async fn start_again(&self, services: &crate::services::Registry, restore: &[String]) {
        let wanted: Vec<ServiceId> = restore
            .iter()
            .filter_map(|id| ServiceId::parse(id).ok())
            .collect();

        if wanted.is_empty() {
            return;
        }

        let graph = match services.graph().await {
            Ok(graph) => graph,
            Err(error) => {
                tracing::warn!(
                    ?error,
                    "this home's services could not be read, so nothing an update stopped was \
                     started again"
                );
                return;
            }
        };

        let plan = match graph.start_plan(wanted.iter()) {
            Ok(plan) => plan,
            Err(error) => {
                tracing::warn!(
                    ?error,
                    "no order could be worked out for the services an update stopped"
                );
                return;
            }
        };

        let walk = services.start(&graph, &plan).await;

        tracing::info!(
            started = walk.reached.len(),
            wanted = wanted.len(),
            refused = walk.failed.as_ref().map(|(id, _)| id.as_str()),
            "started the services an update stopped"
        );
    }

    /// The wire shape of a release, sized for *this* machine.
    ///
    /// `size` is what this machine would download and not the largest file published: it is shown
    /// in a consent prompt, and a number from another architecture would be a number about somebody
    /// else's download. For a copy the `.pkg` installed, that is the `.pkg` (T88f).
    fn release(&self, feed: &Feed) -> UpdateRelease {
        let size = host().ok().map_or(0, |(os, arch)| match &self.placement {
            updates::Placement::Installer { .. } => feed
                .installer(os, arch, self.installer_kind(), self.headless())
                .map_or(0, |installer| installer.size),
            _ => feed.artifact(os, arch).map_or(0, |artifact| artifact.size),
        });

        UpdateRelease {
            version: feed.version.clone(),
            published_at: feed.published_at.to_string(),
            notes: feed.notes.clone(),
            notes_url: feed.notes_url.clone(),
            size,
        }
    }

    /// The installer this copy is updated with, present exactly when the `.pkg` installed it —
    /// T88f, D3. The size is the one the feed names for this machine, or 0 before a feed is read.
    fn installer_of(&self, feed: Option<&Feed>) -> Option<UpdateInstaller> {
        let updates::Placement::Installer { .. } = &self.placement else {
            return None;
        };

        let size = feed
            .zip(host().ok())
            .and_then(|(feed, (os, arch))| {
                feed.installer(os, arch, self.installer_kind(), self.headless())
            })
            .map_or(0, |installer| installer.size);

        Some(UpdateInstaller {
            kind: "pkg".to_owned(),
            size,
        })
    }

    /// `update.hand_over` — download and verify the next `.pkg`, open it in Installer.app, and write
    /// the handover down (T88f, D4 and D5). **Nothing is stopped at any point**: a person who
    /// cancels the installer has lost nothing.
    ///
    /// # Errors
    ///
    /// `precondition_failed` when no feed has been read, the version is not the one offered, this
    /// copy is not the `.pkg`'s, or the release has no installer for this machine; whatever the
    /// download and the checksum reported; and the platform's error, with the command that installs
    /// the verified file without a window as its hint, when the installer would not open.
    pub(crate) async fn hand_over(&self, version: &str) -> Result<UpdateHandedOver, Error> {
        let checked = self.checked()?;

        if checked.feed.version != version {
            return Err(mixengine_core::Error::UpdateNotOffered {
                asked: version.to_owned(),
                offered: Some(checked.feed.version.clone()),
            }
            .to_wire());
        }

        let updates::Placement::Installer { .. } = &self.placement else {
            return Err(Error::new(
                ErrorCode::PreconditionFailed,
                "this copy of MixEngine was not placed by an installer",
            )
            .with_hint("`mix self-update` updates it in place"));
        };

        let (os, arch) = host()?;
        let installer = checked
            .feed
            .installer(os, arch, self.installer_kind(), self.headless())
            .ok_or_else(|| {
                mixengine_core::Error::InstallerUnavailable {
                    os: format!("{os:?}").to_lowercase(),
                    arch: format!("{arch:?}").to_lowercase(),
                }
                .to_wire()
            })?;

        let package = self.fetch_package(version, installer).await?;

        // The platform's error keeps its own code; the hint is the line that installs the verified
        // file without a window, which is also what over SSH somebody would need (D5, M4).
        //
        // **No installer to open is not a failure** — T182b, D5. A Linux machine with no desktop
        // session has the verified package and the command that installs it, which is the whole
        // handover there.
        let opened = match self.host.installers().open(&package) {
            Ok(()) => true,
            Err(mixengine_platform::Error::UnsupportedPlatform { reason, .. }) => {
                tracing::info!(%reason, "no software installer to open the package in");
                false
            }
            Err(error) => return Err(error.to_wire().with_hint(install_command(&package))),
        };

        updates::records::set(
            &self.store,
            updates::records::HANDED_OVER,
            &updates::records::HandedOver {
                version: version.to_owned(),
                at: Timestamp::from_system_time(std::time::SystemTime::now()),
            },
        )
        .await
        .map_err(|error| error.to_wire())?;

        tracing::info!(%version, package = %package.display(), opened, "a package was handed over");

        Ok(UpdateHandedOver {
            version: version.to_owned(),
            package: package.display().to_string(),
            command: install_command(&package),
            opened,
        })
    }

    /// Download and verify the package, or reuse the one already verified for this version.
    ///
    /// **Reused**, because the ordinary way back into this is a person who cancelled Installer.app
    /// and pressed Update again, and 70 MB fetched twice is a cost with nothing bought by it. The
    /// file is hashed again first: it sits in the user's home, where any local process can change
    /// it.
    async fn fetch_package(
        &self,
        version: &str,
        installer: &updates::feed::InstallerArtifact,
    ) -> Result<PathBuf, Error> {
        let into = self.staging_for(version);

        if let Some(existing) = only_file_in(&into) {
            let hashed = existing.clone();
            // A digest or nothing: a file that cannot be hashed is fetched again, so the error is
            // dropped where it is made rather than carried out of the closure.
            let digest = tokio::task::spawn_blocking(move || {
                mixengine_core::install::sha256_of(&hashed).ok()
            })
            .await
            .ok()
            .flatten();

            if digest.as_deref() == Some(installer.sha256.as_str()) {
                return Ok(existing);
            }
        }

        // Anything else there is a half-finished or wrong download, and `install` refuses a
        // directory that exists: an update must not trip over its own leftovers.
        let _ = tokio::fs::remove_dir_all(&into).await;

        self.installer
            .install(
                &installer.as_artifact(),
                &into,
                None,
                NotAnArchive::OneFile,
                &Quiet,
            )
            .await
            .map_err(|error| error.to_wire())?;

        only_file_in(&into).ok_or_else(|| {
            Error::new(
                ErrorCode::Internal,
                format!("{} holds no package after the download", into.display()),
            )
        })
    }

    /// The version on disk, while a handover is recorded and the binary there says it is that
    /// version — T88f, D6. [`None`] otherwise, including for a different version somebody installed.
    async fn installed_version(&self) -> Option<String> {
        let handed: updates::records::HandedOver =
            read(&self.store, updates::records::HANDED_OVER).await?;
        let exe = self.daemon_exe.clone()?;
        let identity = mixengine_platform::install::file_identity(&exe)?;

        let cached = self.installed.lock().ok()?.clone();
        let on_disk = match cached {
            Some((seen, version)) if seen == identity => version,
            _ => {
                let version =
                    tokio::task::spawn_blocking(move || updates::installed::version_of(&exe))
                        .await
                        .ok()
                        .flatten();

                if let Ok(mut installed) = self.installed.lock() {
                    *installed = Some((identity, version.clone()));
                }

                version
            }
        };

        on_disk.filter(|version| *version == handed.version)
    }

    /// Forget a handover that no longer applies — T88f, D6's last paragraph.
    ///
    /// Two ways: this daemon already is the version handed over (somebody installed and restarted
    /// another way), or the feed now offers a different release than the one handed over.
    async fn settle_handover(&self, feed: Option<&Feed>) {
        let Some(handed) =
            read::<updates::records::HandedOver>(&self.store, updates::records::HANDED_OVER).await
        else {
            return;
        };

        let running = handed.version == env!("CARGO_PKG_VERSION");
        let superseded = feed.is_some_and(|feed| feed.version != handed.version);

        if running || superseded {
            self.forget(&handed.version).await;
        }
    }

    /// The version `update.finish` may finish, or why not — T88f, D6.
    ///
    /// # Errors
    ///
    /// `precondition_failed` when nothing was handed over, or when the binary on disk is not yet
    /// the version that was.
    pub(crate) async fn finishable(&self) -> Result<String, Error> {
        let Some(handed) =
            read::<updates::records::HandedOver>(&self.store, updates::records::HANDED_OVER).await
        else {
            return Err(Error::new(
                ErrorCode::PreconditionFailed,
                "nothing has been handed to the installer",
            )
            .with_hint("`mix self-update` opens the next .pkg in Installer.app"));
        };

        match self.installed_version().await {
            Some(version) => Ok(version),
            None => Err(mixengine_core::Error::UpdateNotInstalled {
                version: handed.version,
            }
            .to_wire()),
        }
    }

    /// Clear the handover `update.finish` has just finished.
    pub(crate) async fn forget_handover(&self) {
        if let Some(handed) =
            read::<updates::records::HandedOver>(&self.store, updates::records::HANDED_OVER).await
        {
            self.forget(&handed.version).await;
        }
    }

    /// Clear the record and the package it named.
    async fn forget(&self, version: &str) {
        if let Err(error) =
            updates::records::clear(&self.store, updates::records::HANDED_OVER).await
        {
            tracing::warn!(%error, "a finished handover's record could not be removed");
        }

        let _ = tokio::fs::remove_dir_all(self.staging_for(version)).await;
    }

    /// The directory this copy's binaries are in, whatever installed them.
    pub(crate) fn directory(&self) -> &std::path::Path {
        match &self.placement {
            updates::Placement::SelfUpdatable { directory }
            | updates::Placement::Managed { directory, .. }
            | updates::Placement::Installer { directory, .. } => directory,
        }
    }

    /// The last feed this daemon read, or the refusal a caller that needs one answers with.
    fn checked(&self) -> Result<Checked, Error> {
        self.last
            .lock()
            .ok()
            .and_then(|last| last.clone())
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::PreconditionFailed,
                    "this daemon has not read the update feed yet",
                )
                .with_hint("`mix self-update --check` reads it")
            })
    }

    /// Where a payload is unpacked. One directory per version, under `cache/`.
    fn staging_for(&self, version: &str) -> PathBuf {
        // Not `join`ed from the feed's string without thought: a version out of a document is a
        // path component here, so it goes through the same validation every runtime version does,
        // and anything that will not parse lands in one fixed directory instead.
        let component = mixengine_proto::PackageVersion::parse(version).map_or_else(
            |_| "unnamed".to_owned(),
            |version| version.as_str().to_owned(),
        );

        self.paths.cache().join(STAGING_DIR).join(component)
    }

    /// Whether this version is one nobody has been told about yet.
    fn newly(&self, version: &str) -> bool {
        self.announced
            .lock()
            .is_ok_and(|mut announced| announced.insert(version.to_owned()))
    }

    /// Whether a feed's release is offered here, and if not, why.
    async fn decision(&self, feed: &Feed) -> updates::Decision {
        let skipped: Option<String> = read(&self.store, updates::records::SKIPPED_VERSION).await;
        let remind_after: Option<Timestamp> =
            read(&self.store, updates::records::REMIND_AFTER).await;
        // A copy the `.pkg` installed is offered what it can install: the next `.pkg` (T88f).
        let has_build = host().is_ok_and(|(os, arch)| match &self.placement {
            updates::Placement::Installer { .. } => feed
                .installer(os, arch, self.installer_kind(), self.headless())
                .is_some(),
            _ => feed.artifact(os, arch).is_some(),
        });

        updates::offer::decide(
            env!("CARGO_PKG_VERSION"),
            &feed.version,
            has_build,
            skipped.as_deref(),
            remind_after,
            Timestamp::from_system_time(std::time::SystemTime::now()),
        )
    }
}

/// The environment variable a running AppImage sets, and the one thing that identifies one.
const APPIMAGE: &str = "APPIMAGE";

/// Why a copy the `.pkg` installed is not swapped in place, as an old client reads it — T88f.
const INSTALLER_BECAUSE: &str = "the .pkg installed this copy, and MixEngine updates it by opening the next .pkg in Installer.app";

/// The line that installs `package` without a window — T88f, D5. Quoted, because the path is in
/// `Application Support` on macOS and has a space in it.
fn install_command(package: &std::path::Path) -> String {
    // By the file's own kind: the next package of a `.deb` copy is a `.deb` (T182b, D5).
    match package.extension().and_then(|extension| extension.to_str()) {
        Some("deb") => format!("sudo apt install '{}'", package.display()),
        Some("rpm") => format!("sudo dnf install '{}'", package.display()),
        _ => format!("sudo installer -pkg '{}' -target /", package.display()),
    }
}

/// The one file in `directory`, or [`None`] when there is not exactly one.
fn only_file_in(directory: &std::path::Path) -> Option<PathBuf> {
    let mut files = std::fs::read_dir(directory)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file());
    let first = files.next()?;

    files.next().is_none().then_some(first)
}

/// Where payloads are unpacked, under the home's `cache/`.
const STAGING_DIR: &str = "updates";

/// Read one record, treating every failure as absent.
///
/// The whole of this module's error policy for the `settings` table in one place: a row that cannot
/// be read means what an absent one means — go and ask again — and no update question is worth
/// failing a `daemon.status` over.
async fn read<T: serde::de::DeserializeOwned>(store: &Store, key: &str) -> Option<T> {
    match updates::records::get(store, key).await {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(key, %error, "an update record could not be read");
            None
        }
    }
}

/// This machine's pair, as the feed spells it.
///
/// # Errors
///
/// `unsupported` on an operating system or architecture this product does not publish for, which is
/// a build nobody made rather than a machine anybody has.
fn host() -> Result<(mixengine_core::index::Os, mixengine_core::index::Arch), Error> {
    let os = mixengine_core::index::Os::host();
    let arch = mixengine_core::index::Arch::host();

    match (os, arch) {
        (Some(os), Some(arch)) => Ok((os, arch)),
        _ => Err(Error::new(
            ErrorCode::UnsupportedPlatform,
            format!(
                "MixEngine publishes no builds for {}/{}",
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
        )),
    }
}

/// The wire shape of a placement.
fn placement(placement: &updates::Placement) -> UpdatePlacement {
    match placement {
        updates::Placement::SelfUpdatable { directory } => UpdatePlacement::SelfUpdatable {
            directory: directory.display().to_string(),
        },
        updates::Placement::Managed { directory, because } => UpdatePlacement::Managed {
            directory: directory.display().to_string(),
            because: because.clone(),
        },
        // `managed` on the wire, with a sentence still true for a client from before T88f: a new
        // tagged variant would make an old client fail to read the whole status (the design, D3).
        updates::Placement::Installer { directory, .. } => UpdatePlacement::Managed {
            directory: directory.display().to_string(),
            because: INSTALLER_BECAUSE.to_owned(),
        },
    }
}

/// What `update.apply` answers, built from what the swap actually did.
pub(crate) fn applied(
    staged: &Staged,
    swapped: &mixengine_core::updates::Swapped,
    restarting: Vec<ServiceId>,
) -> UpdateApplied {
    UpdateApplied {
        from: env!("CARGO_PKG_VERSION").to_owned(),
        to: staged.to.clone(),
        directory: staged.directory.display().to_string(),
        replaced: swapped.replaced.clone(),
        kept: swapped.kept.clone(),
        restarting,
    }
}

/// What the new daemon starts again after an update: the services the stop walk reached **that
/// were running before it** — roadmap task **T88**, "restore running services".
///
/// The stop walk takes the whole graph in reverse dependency order, and `stop_one` reports a
/// service that was already stopped as stopped, so `reached` holds every service in the plan.
/// Recording that list would start every service on the machine at the next start, including the
/// ones a person had stopped on purpose; the T88f check by hand found exactly that. The order is
/// the walk's, which is what `restore_after_update` replays.
pub(crate) fn restorable(reached: &[ServiceId], running: &[ServiceId]) -> Vec<ServiceId> {
    reached
        .iter()
        .filter(|id| running.contains(id))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(name: &str) -> ServiceId {
        ServiceId::parse(name).expect("a service id")
    }

    /// The stop walk reaches every service in the plan, running or not, and `stop_one` counts a
    /// service that was already stopped as stopped. Found by the T88f check by hand: a `.pkg`
    /// update with two services running started six, four of which the person had stopped.
    #[test]
    fn a_service_that_was_stopped_before_the_update_is_not_restored() {
        let reached = vec![id("redis@main"), id("mariadb@main"), id("caddy")];
        let running = vec![id("caddy"), id("redis@main")];

        assert_eq!(
            restorable(&reached, &running),
            vec![id("redis@main"), id("caddy")]
        );
    }

    /// The order is the stop walk's, which is reverse dependency order — what the new daemon
    /// replays in reverse to start them again.
    #[test]
    fn the_restore_list_keeps_the_stop_walk_order() {
        let reached = vec![id("php-fpm@8.4"), id("caddy"), id("mariadb@main")];
        let running = vec![id("mariadb@main"), id("caddy"), id("php-fpm@8.4")];

        assert_eq!(restorable(&reached, &running), reached);
    }
}
