//! Installing a staged update: completing it, restoring what it stopped, and starting again.

use mixengine_core::updates::{self, Feed};
use mixengine_proto::{ServiceId, UpdateInstaller, UpdateRelease};

use super::{Updates, host, read};

impl Updates {
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
    pub(super) async fn start_again(
        &self,
        services: &crate::services::Registry,
        restore: &[String],
    ) {
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
    pub(super) fn release(&self, feed: &Feed) -> UpdateRelease {
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
    pub(super) fn installer_of(&self, feed: Option<&Feed>) -> Option<UpdateInstaller> {
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
}
