//! `update.status`, `update.check`, `update.decide`, `update.apply`, `update.hand_over` and
//! `update.finish`: this daemon replacing itself.
//!
//! The feed, the download and the swap are [`crate::updates`]'s. What is here is the stop, the swap
//! and the end of this process, in the order that leaves a working daemon when any of them fails.

use mixengine_proto::{
    Error, ServiceId, UpdateApplied, UpdateApply, UpdateCheck, UpdateDecide, UpdateFinish,
    UpdateHandOver, UpdateStatus,
};

use super::Api;
use crate::error::ToWire as _;

impl Api {
    /// `update.status` — what this daemon knows about updating itself, touching no network.
    pub(super) async fn update_status(&self) -> UpdateStatus {
        self.updates.status(self.running_services().await).await
    }

    /// `update.check` — read the published feed now.
    ///
    /// # Errors
    ///
    /// The transport failure of a machine that is offline with nothing cached to fall back to.
    /// **Which the background callers never see**, because they never call this: they call
    /// `Updates::check` directly and swallow it. This is the path somebody typed a command on.
    pub(super) async fn update_check(&self, check: UpdateCheck) -> Result<UpdateStatus, Error> {
        self.updates
            .check(check.force, self.running_services().await)
            .await
    }

    /// `update.decide` — remember *skip this version* or *remind me later*.
    pub(super) async fn update_decide(&self, decide: UpdateDecide) -> Result<UpdateStatus, Error> {
        self.updates
            .decide(
                &decide.version,
                decide.decision,
                self.running_services().await,
            )
            .await
    }

    /// `update.apply` — install it, and then stop.
    ///
    /// **The order is download → verify → unpack → smoke → stop → swap → answer → exit**, and the
    /// first four happening before the stop is this method's one departure from
    /// `docs/features/updates.md`'s wording: a download that fails after the stop has cost an
    /// outage, and one that succeeds could have happened while everything was still up. What is
    /// down is the swap and the restart, which is seconds.
    ///
    /// **A swap that fails leaves a working daemon**, which is why the [`Going`](super::Going) guard
    /// is taken *after* the swap and not before it. The rollback puts the binaries back, starts the
    /// services the stop stopped — nothing else on the machine intends to — clears the records, and
    /// returns the failure. This daemon is then the daemon it was before the attempt.
    ///
    /// **A client that goes away mid-apply does not stop the apply**, on `daemon_shutdown`'s rule
    /// and for its reason: abandoning the work between the stop and the swap would leave a home with
    /// everything down and half its binaries renamed. Before the stop there is nothing to protect —
    /// an abandoned download leaves a `.part` file, which is what the next attempt resumes from.
    ///
    /// # Errors
    ///
    /// `precondition_failed` when the version asked for is not the one offered, or when this copy of
    /// MixEngine was installed by something that is not MixEngine; and whatever the download, the
    /// checksum, the unpacking, the smoke test or the swap reported.
    pub(super) async fn update_apply(&self, apply: UpdateApply) -> Result<UpdateApplied, Error> {
        // Everything that can refuse, before anything is stopped.
        let staged = self.updates.stage(&apply.version).await?;

        // Read before the stop, because the stop walk reaches every service in the plan and
        // reports an already-stopped one as stopped: what the new daemon starts again is the
        // services that were running, not the whole machine (`updates::restorable`).
        let running = self.running_services().await;
        let stopped = self.stop_everything().await.map_err(|error| {
            // The services are still running: `stop_everything` builds the plan before it walks it,
            // and this is the failure to build one.
            tracing::warn!(%error, "an update could not work out the order to stop services in");
            error
        })?;
        let restarting = crate::updates::restorable(&stopped.reached, &running);

        self.updates.remember(&staged.to, &restarting).await?;

        let swapped = match mixengine_core::updates::apply::swap(
            &staged.staged,
            &staged.provides,
            &staged.directory,
        ) {
            Ok(swapped) => swapped,

            Err(error) => {
                tracing::warn!(
                    %error,
                    "an update was rolled back; starting again what it stopped"
                );

                // The binaries are already back — `swap` unwinds its own renames. What is left is
                // the stop, which nothing else on this machine is going to undo.
                self.updates.roll_back(&self.services).await;

                return Err(error.to_wire());
            }
        };

        // **Taken here and not earlier**, which is the whole of the paragraph above: from this line
        // on there is no way of leaving this daemon running, and every path that could still have
        // failed is behind it.
        let _going = self.shutdown.begun();

        tracing::info!(
            from = env!("CARGO_PKG_VERSION"),
            to = %staged.to,
            replaced = ?swapped.replaced,
            kept = ?swapped.kept,
            restarting = restarting.len(),
            "this daemon has been replaced and is stopping so the new one can start"
        );

        Ok(crate::updates::applied(&staged, &swapped, restarting))
    }

    /// `update.hand_over` — the next `.pkg`, verified and open in Installer.app. Nothing is
    /// stopped, and the daemon keeps running.
    pub(super) async fn update_hand_over(
        &self,
        hand_over: UpdateHandOver,
    ) -> Result<mixengine_proto::UpdateHandedOver, Error> {
        self.updates.hand_over(&hand_over.version).await
    }

    /// `update.finish` — the second half of `update.apply`, for a copy the `.pkg` installed.
    ///
    /// Refused until the new binary is on disk. Then stop, remember, answer, exit: this process
    /// runs the old image, which kept running when Installer.app replaced the file (measured on a
    /// Mac), so the stop order is the one the running daemon knows.
    pub(super) async fn update_finish(
        &self,
        _finish: UpdateFinish,
    ) -> Result<UpdateApplied, Error> {
        let to = self.updates.finishable().await?;

        // As in `update_apply`: the running services, read before the stop walk reaches the rest.
        let running = self.running_services().await;
        let stopped = self.stop_everything().await.map_err(|error| {
            tracing::warn!(%error, "an update could not work out the order to stop services in");
            error
        })?;
        let restarting = crate::updates::restorable(&stopped.reached, &running);

        self.updates.remember(&to, &restarting).await?;
        self.updates.forget_handover().await;

        // From here there is no way of leaving this daemon running, which is `update_apply`'s rule.
        let _going = self.shutdown.begun();

        tracing::info!(
            from = env!("CARGO_PKG_VERSION"),
            %to,
            restarting = restarting.len(),
            "the .pkg has installed; this daemon is stopping so the new one can start"
        );

        Ok(UpdateApplied {
            from: env!("CARGO_PKG_VERSION").to_owned(),
            to,
            directory: self.updates.directory().display().to_string(),
            replaced: Vec::new(),
            kept: Vec::new(),
            restarting,
        })
    }

    /// The services an update would stop and start again.
    ///
    /// **What is running now**, which is what the stop walk will reach. Reported so a consent prompt
    /// can say *"3 services will be stopped and started again"* — `docs/features/updates.md`'s
    /// *"never update while a supervised service is under load without asking"* in the only form
    /// that rule can take once consent is always required.
    async fn running_services(&self) -> Vec<ServiceId> {
        self.services.supervised().into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use mixengine_proto::rpc;
    use serde_json::Value;

    use super::*;
    use crate::api::test_daemon::{Daemon, daemon, daemon_reading, web_and_db};

    /// A daemon with no feed to read answers the question anyway.
    ///
    /// **What it must not do is fail.** `docs/features/updates.md` says an offline machine never
    /// sees an error and never a slower startup, and this is the same rule one call along: the
    /// fixture's feed URL answers nothing, and `update.status` still says what version is running,
    /// where it lives, and that it has been offered nothing.
    #[tokio::test]
    async fn update_status_answers_on_a_machine_that_has_never_checked() {
        let daemon = daemon(web_and_db(), &[]).await;

        let status: UpdateStatus = daemon.expect(rpc::method::UPDATE_STATUS, Value::Null).await;

        assert_eq!(status.current, env!("CARGO_PKG_VERSION"));
        assert!(status.available.is_none(), "{status:?}");
        assert!(!status.offered);
        assert!(status.checked_at.is_none(), "{status:?}");
        assert!(!status.stale);

        daemon.quiet().await;
    }

    /// This test binary is not an installed MixEngine, and the fixture's daemon lives in a temporary
    /// home the test owns — so the placement it reports is `self_updatable`, and the assertion worth
    /// making is that the answer is *a placement* rather than a panic on a path nobody installed.
    #[tokio::test]
    async fn update_status_says_where_this_copy_lives() {
        let daemon = daemon(web_and_db(), &[]).await;

        let status: UpdateStatus = daemon.expect(rpc::method::UPDATE_STATUS, Value::Null).await;

        let mixengine_proto::UpdatePlacement::SelfUpdatable { directory } = &status.placement
        else {
            panic!("a directory this test made is one this account can write: {status:?}");
        };
        assert!(directory.contains("installed-beside"), "{directory}");

        daemon.quiet().await;
    }

    /// `update.apply` takes the version the client showed the user. Without that check, a feed that
    /// changed between the prompt and the answer would install something nobody read the notes for —
    /// and on a daemon that has read no feed at all, *every* version is that case.
    #[tokio::test]
    async fn applying_a_version_this_daemon_was_never_offered_is_refused() {
        let daemon = daemon(web_and_db(), &[]).await;

        let answer = daemon
            .ask(
                rpc::method::UPDATE_APPLY,
                serde_json::json!({ "version": "9.9.9" }),
            )
            .await;

        assert_eq!(
            answer["error"]["data"]["code"], "precondition_failed",
            "{answer}"
        );

        daemon.quiet().await;
    }

    /// Skip is remembered, and it is remembered by the daemon rather than by the client — which is
    /// what makes *"declining an update does not re-prompt for that version"* a property of the
    /// product rather than of whichever client somebody happened to use.
    #[tokio::test]
    async fn a_skipped_version_is_written_down_where_the_next_daemon_will_read_it() {
        let daemon = daemon(web_and_db(), &[]).await;

        let _: UpdateStatus = daemon
            .expect(
                rpc::method::UPDATE_DECIDE,
                serde_json::json!({ "version": "9.9.9", "decision": "skip" }),
            )
            .await;

        assert_eq!(
            mixengine_core::updates::records::get::<String>(
                &daemon.api.store,
                mixengine_core::updates::records::SKIPPED_VERSION,
            )
            .await
            .expect("a read"),
            Some("9.9.9".to_owned())
        );

        daemon.quiet().await;
    }

    /// What the test release's `.pkg` holds. Any bytes: the daemon hashes them and opens the file.
    const PKG_BYTES: &[u8] = b"not a package, and never installed by a test";

    /// The version the test feed offers.
    const PKG_OFFERED: &str = "99.0.0";

    /// A daemon the `.pkg` installed, reading a feed served in-process, and the registry serving
    /// it.
    ///
    /// `installers` says whether the feed lists the package at all (a feed from before installers
    /// were published does not), and `sha256` is the digest it claims for it: [`None`] claims the
    /// right one.
    async fn installed_by_pkg(
        installers: bool,
        sha256: Option<String>,
    ) -> (Daemon, mixengine_testkit::MockRegistry) {
        let registry =
            mixengine_testkit::MockRegistry::start(&serde_json::json!({ "schema": 1 })).await;
        let packed = mixengine_testkit::Packed::one_file(
            "mixengine-99.0.0-macos-universal-headless.pkg",
            PKG_BYTES,
        );
        let url = registry.publish_asset(&packed.path(), packed.bytes.clone());
        // Headless, because the mock host has no window installed: the updater hands a machine the
        // flavour it has.
        let row = serde_json::json!({
            "os": std::env::consts::OS, "arch": std::env::consts::ARCH, "kind": "pkg",
            "flavour": "headless",
            "url": url, "size": PKG_BYTES.len(), "sha256": sha256.unwrap_or(packed.sha256),
        });

        registry.publish(&serde_json::json!({
            "schema": 1,
            "generated_at": "2026-09-24T00:00:00Z",
            "version": PKG_OFFERED,
            "published_at": "2026-09-24T00:00:00Z",
            "notes": "feat(updates): hand a .pkg to Installer.app",
            "artifacts": [],
            "installers": if installers { vec![row] } else { Vec::new() },
        }));

        let daemon = daemon_reading(
            web_and_db(),
            &[],
            |home| {
                mixengine_platform::mock::Host::with_receipt(
                    home,
                    mixengine_core::updates::placement::PKG_RECEIPT,
                )
            },
            Some(crate::updates::FeedSource {
                url: registry.url(),
                public_key: registry.public_key().to_owned(),
            }),
        )
        .await;

        (daemon, registry)
    }

    /// Read the feed, as `mix self-update` does first.
    async fn checked(daemon: &Daemon) -> UpdateStatus {
        daemon
            .expect(
                rpc::method::UPDATE_CHECK,
                serde_json::json!({ "force": true }),
            )
            .await
    }

    fn hand_over_params() -> Value {
        serde_json::json!({ "version": PKG_OFFERED })
    }

    /// The .pkg path on the mock: offered, handed over, verified, opened, and the daemon still up.
    #[tokio::test]
    async fn a_pkg_copy_is_handed_to_the_installer_and_keeps_running() {
        let (daemon, _registry) = installed_by_pkg(true, None).await;
        let status = checked(&daemon).await;
        assert!(
            status.offered,
            "the .pkg copy is offered the release: {status:?}"
        );

        let handed: mixengine_proto::UpdateHandedOver = daemon
            .expect(rpc::method::UPDATE_HAND_OVER, hand_over_params())
            .await;

        assert_eq!(
            daemon.host.opened(),
            vec![std::path::PathBuf::from(&handed.package)],
            "exactly the verified file is opened: {handed:?}"
        );
        assert_eq!(
            std::fs::read(&handed.package).expect("the package is kept"),
            PKG_BYTES
        );
        assert!(handed.command.contains(&handed.package), "{handed:?}");
        assert!(
            handed.opened,
            "the mock has an installer to open: {handed:?}"
        );
        assert!(
            !daemon.api.shutdown.token().is_cancelled(),
            "hand_over never ends the daemon"
        );

        daemon.quiet().await;
    }

    /// A package whose bytes are not the ones the signed feed named is refused, and nothing opens.
    #[tokio::test]
    async fn a_package_that_is_not_what_the_feed_named_is_refused_and_opens_nothing() {
        let (daemon, _registry) = installed_by_pkg(true, Some("00".repeat(32))).await;
        checked(&daemon).await;

        let answer = daemon
            .ask(rpc::method::UPDATE_HAND_OVER, hand_over_params())
            .await;

        assert!(
            answer.get("error").is_some(),
            "a mismatch is an error: {answer}"
        );
        assert!(
            daemon.host.opened().is_empty(),
            "nothing unverified reaches Installer.app: {answer}"
        );

        daemon.quiet().await;
    }

    /// Cancel in Installer.app, then Update again: the verified file is reused, not fetched again.
    #[tokio::test]
    async fn a_second_hand_over_reuses_the_verified_package() {
        let (daemon, registry) = installed_by_pkg(true, None).await;
        checked(&daemon).await;

        let first: mixengine_proto::UpdateHandedOver = daemon
            .expect(rpc::method::UPDATE_HAND_OVER, hand_over_params())
            .await;
        let second: mixengine_proto::UpdateHandedOver = daemon
            .expect(rpc::method::UPDATE_HAND_OVER, hand_over_params())
            .await;

        assert_eq!(first.package, second.package);
        assert_eq!(
            registry.asset_ranges().len(),
            1,
            "the package was downloaded once: {:?}",
            registry.asset_ranges()
        );
        assert_eq!(daemon.host.opened().len(), 2, "and opened twice");

        daemon.quiet().await;
    }

    /// `update.apply` would swap files in a directory the .pkg owns. Refused, naming the way.
    #[tokio::test]
    async fn applying_in_place_is_refused_for_a_pkg_copy() {
        let (daemon, _registry) = installed_by_pkg(true, None).await;
        checked(&daemon).await;

        let answer = daemon
            .ask(rpc::method::UPDATE_APPLY, hand_over_params())
            .await;

        assert_eq!(
            answer["error"]["data"]["code"], "precondition_failed",
            "{answer}"
        );
        assert!(answer.to_string().contains(".pkg"), "{answer}");

        daemon.quiet().await;
    }

    /// Old clients see `managed`; new ones see the installer and its size.
    #[tokio::test]
    async fn the_status_of_a_pkg_copy_says_managed_and_carries_the_installer() {
        let (daemon, _registry) = installed_by_pkg(true, None).await;

        let status = checked(&daemon).await;

        assert!(
            matches!(
                status.placement,
                mixengine_proto::UpdatePlacement::Managed { .. }
            ),
            "{status:?}"
        );
        assert_eq!(
            status.installer,
            Some(mixengine_proto::UpdateInstaller {
                kind: "pkg".to_owned(),
                size: PKG_BYTES.len() as u64,
            }),
            "{status:?}"
        );

        daemon.quiet().await;
    }

    /// A feed from before installers were published: nothing to offer a .pkg copy, a sentence why,
    /// and no error.
    #[tokio::test]
    async fn a_feed_without_installers_offers_nothing_to_a_pkg_copy() {
        let (daemon, _registry) = installed_by_pkg(false, None).await;

        let status = checked(&daemon).await;

        assert!(!status.offered, "{status:?}");
        assert!(status.because.is_some(), "a sentence says why: {status:?}");

        daemon.quiet().await;
    }

    /// `update.finish` before the new binary is on disk: refused, and nothing is stopped.
    #[tokio::test]
    async fn finishing_before_the_install_is_refused_and_stops_nothing() {
        let (daemon, _registry) = installed_by_pkg(true, None).await;
        checked(&daemon).await;
        let _: mixengine_proto::UpdateHandedOver = daemon
            .expect(rpc::method::UPDATE_HAND_OVER, hand_over_params())
            .await;

        let answer = daemon
            .ask(rpc::method::UPDATE_FINISH, serde_json::json!({}))
            .await;

        assert_eq!(
            answer["error"]["data"]["code"], "precondition_failed",
            "{answer}"
        );
        assert!(
            answer.to_string().contains(PKG_OFFERED),
            "the refusal names the version: {answer}"
        );
        assert!(
            !daemon.api.shutdown.token().is_cancelled(),
            "the daemon is still up"
        );

        daemon.quiet().await;
    }

    /// Somebody installed and restarted another way: this daemon already is the recorded version.
    #[tokio::test]
    async fn a_handover_of_the_running_version_is_forgotten() {
        let (daemon, _registry) = installed_by_pkg(true, None).await;
        use mixengine_core::updates::records;
        records::set(
            &daemon.api.store,
            records::HANDED_OVER,
            &records::HandedOver {
                version: env!("CARGO_PKG_VERSION").to_owned(),
                at: mixengine_proto::Timestamp(0),
            },
        )
        .await
        .expect("a record");

        let _: UpdateStatus = daemon.expect(rpc::method::UPDATE_STATUS, Value::Null).await;

        let left = records::get::<records::HandedOver>(&daemon.api.store, records::HANDED_OVER)
            .await
            .expect("a read");
        assert!(
            left.is_none(),
            "a handover of the running version is done: {left:?}"
        );

        daemon.quiet().await;
    }

    /// A newer release than the one handed over is offered: the old handover no longer applies.
    #[tokio::test]
    async fn a_handover_is_forgotten_when_another_release_is_offered() {
        let (daemon, _registry) = installed_by_pkg(true, None).await;
        use mixengine_core::updates::records;
        records::set(
            &daemon.api.store,
            records::HANDED_OVER,
            &records::HandedOver {
                version: "98.0.0".to_owned(),
                at: mixengine_proto::Timestamp(0),
            },
        )
        .await
        .expect("a record");

        checked(&daemon).await;

        let left = records::get::<records::HandedOver>(&daemon.api.store, records::HANDED_OVER)
            .await
            .expect("a read");
        assert!(
            left.is_none(),
            "98.0.0 is not what is offered now: {left:?}"
        );

        daemon.quiet().await;
    }

    /// Put a program at `exe` that answers `--version` with `version` the way Installer.app writes
    /// one: a new file renamed over the old, keeping the old modification time (the readings, M3).
    ///
    /// A shell script, so the tests that use it return early on Windows, where there is no inode to
    /// change and no `.pkg` to have changed it.
    fn install_over(exe: &std::path::Path, version: &str) {
        let before = std::fs::metadata(exe).and_then(|meta| meta.modified()).ok();
        let fresh = exe.with_extension("new");
        std::fs::write(
            &fresh,
            format!("#!/bin/sh\necho \"mixengined {version}\"\n"),
        )
        .expect("the new binary");
        mixengine_platform::install::make_executable(&fresh).expect("executable");
        std::fs::rename(&fresh, exe).expect("renamed over the old one");

        if let Some(before) = before {
            std::fs::File::options()
                .write(true)
                .open(exe)
                .and_then(|file| file.set_modified(before))
                .expect("the old modification time put back");
        }
    }

    #[tokio::test]
    async fn the_install_is_noticed_by_inode_even_with_the_same_modification_time() {
        if cfg!(windows) {
            return; // `install_over` writes a shell script; see there.
        }

        let (daemon, _registry) = installed_by_pkg(true, None).await;
        install_over(&daemon.installed_daemon, "0.0.0");
        checked(&daemon).await;
        let _: mixengine_proto::UpdateHandedOver = daemon
            .expect(rpc::method::UPDATE_HAND_OVER, hand_over_params())
            .await;
        let before: UpdateStatus = daemon.expect(rpc::method::UPDATE_STATUS, Value::Null).await;
        assert_eq!(before.installed, None, "{before:?}");

        install_over(&daemon.installed_daemon, PKG_OFFERED);

        let after: UpdateStatus = daemon.expect(rpc::method::UPDATE_STATUS, Value::Null).await;
        assert_eq!(
            after.installed.as_deref(),
            Some(PKG_OFFERED),
            "a new inode with the old modification time is a new binary: {after:?}"
        );

        daemon.quiet().await;
    }

    /// Somebody installed a different version by hand: not taken for the one handed over.
    #[tokio::test]
    async fn an_install_of_another_version_is_not_taken_for_this_one() {
        if cfg!(windows) {
            return; // `install_over` writes a shell script; see there.
        }

        let (daemon, _registry) = installed_by_pkg(true, None).await;
        checked(&daemon).await;
        let _: mixengine_proto::UpdateHandedOver = daemon
            .expect(rpc::method::UPDATE_HAND_OVER, hand_over_params())
            .await;

        install_over(&daemon.installed_daemon, "0.0.1");

        let status: UpdateStatus = daemon.expect(rpc::method::UPDATE_STATUS, Value::Null).await;
        assert_eq!(status.installed, None, "{status:?}");

        daemon.quiet().await;
    }

    #[tokio::test]
    async fn finishing_stops_remembers_and_ends_the_daemon() {
        if cfg!(windows) {
            return; // `install_over` writes a shell script; see there.
        }

        let (daemon, _registry) = installed_by_pkg(true, None).await;
        checked(&daemon).await;
        let _: mixengine_proto::UpdateHandedOver = daemon
            .expect(rpc::method::UPDATE_HAND_OVER, hand_over_params())
            .await;
        install_over(&daemon.installed_daemon, PKG_OFFERED);

        let applied: UpdateApplied = daemon
            .expect(rpc::method::UPDATE_FINISH, serde_json::json!({}))
            .await;

        use mixengine_core::updates::records;
        assert_eq!(applied.to, PKG_OFFERED, "{applied:?}");
        assert!(
            applied.replaced.is_empty(),
            "the installer did the replacing: {applied:?}"
        );
        assert!(
            records::get::<records::Applied>(&daemon.api.store, records::APPLIED)
                .await
                .expect("a read")
                .is_some(),
            "the next daemon is told what was applied"
        );
        assert!(
            records::get::<records::HandedOver>(&daemon.api.store, records::HANDED_OVER)
                .await
                .expect("a read")
                .is_none(),
            "the handover is over"
        );
        assert!(
            daemon.api.shutdown.token().is_cancelled(),
            "the daemon is on its way out"
        );

        daemon.quiet().await;
    }

    /// And *later* writes a moment rather than a version, because it is about every version.
    #[tokio::test]
    async fn remind_me_later_writes_a_moment_inside_the_clamp() {
        let daemon = daemon(web_and_db(), &[]).await;
        let now = mixengine_proto::Timestamp::from_system_time(std::time::SystemTime::now());

        let _: UpdateStatus = daemon
            .expect(
                rpc::method::UPDATE_DECIDE,
                serde_json::json!({ "version": "9.9.9", "decision": "later" }),
            )
            .await;

        let due = mixengine_core::updates::records::get::<mixengine_proto::Timestamp>(
            &daemon.api.store,
            mixengine_core::updates::records::REMIND_AFTER,
        )
        .await
        .expect("a read")
        .expect("a moment");

        // Inside the clamp `updates::offer` reads it against, or the reminder would be disbelieved
        // the moment it was written.
        let ahead = due.0 - now.0;
        assert!(ahead > 0, "{ahead}");
        assert!(
            ahead <= mixengine_core::updates::records::REMIND_CLAMP_SECONDS * 1_000,
            "{ahead}"
        );

        daemon.quiet().await;
    }
}
