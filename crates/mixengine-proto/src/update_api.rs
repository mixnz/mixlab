//! What `update.*` answers.
//!
//! **The daemon decides and the client renders.** Whether a release is offered is
//! [`UpdateStatus::offered`] and *why not* is [`UpdateStatus::because`], as a whole sentence: the
//! four reasons a perfectly good release is not shown — it is not newer, there is no build for this
//! machine, somebody skipped it, somebody asked to be reminded later — would otherwise be four codes
//! that every client had to spell back out in English, which is the business-logic-in-a-client bug
//! `CLAUDE.md` forbids.
//!
//! **Nothing here names the elevated helper's update path.** `mixengine-elevate` is excluded from
//! the swap by name and reported in [`UpdateApplied::kept`]; replacing it needs its own elevation
//! prompt with a minisign check inside the elevated context, which is
//! [`PrivilegedOp::HelperReplace`](crate::privileged::PrivilegedOp::HelperReplace).

use crate::{ServiceId, Timestamp};

/// What `update.check` takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UpdateCheck {
    /// Go to the network even if what is cached is still fresh.
    ///
    /// What `mix self-update --check` sets, because `docs/features/updates.md` says that command
    /// forces an immediate check. The daemon's own startup check leaves it `false`, so a daemon
    /// restarted ten times in an hour makes one request.
    #[serde(default)]
    pub force: bool,
}

/// What `update.decide` takes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UpdateDecide {
    /// The version being decided about.
    ///
    /// Named rather than implied, so that skipping the release somebody was shown cannot skip a
    /// different one that arrived between the prompt and the answer.
    pub version: String,

    /// What they answered.
    pub decision: UpdateDecision,
}

/// The two answers that are not *install*.
///
/// **Both are real and both are remembered** — `docs/features/updates.md`. A decline that was
/// forgotten is a prompt that comes back tomorrow, which is how an update prompt becomes something
/// people dismiss without reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum UpdateDecision {
    /// Never offer this version again. A later one is still offered.
    Skip,

    /// Offer it again in a few days.
    Later,
}

/// What `update.apply` takes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UpdateApply {
    /// The version the client showed the user.
    ///
    /// **The daemon refuses if this is no longer what the feed offers.** Without it, a check that
    /// landed between the prompt and the answer would install something nobody read the notes for.
    pub version: String,
}

/// Everything this daemon knows about updating itself.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UpdateStatus {
    /// The version running now — `CARGO_PKG_VERSION`, the string `mixengined --version` prints.
    pub current: String,

    /// The release the feed names, whether or not it is offered.
    ///
    /// Present-and-not-offered is a real state and the interesting one: it is what lets a client
    /// say *"0.3.0 exists; you skipped it"* rather than *"no update"*.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available: Option<UpdateRelease>,

    /// Whether a client should put it in front of somebody.
    pub offered: bool,

    /// Why not, phrased for a person. [`None`] exactly when [`UpdateStatus::offered`] is `true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub because: Option<String>,

    /// When the feed was last read, or [`None`] on a daemon that has never managed to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<Timestamp>,

    /// Whether that reading came from a cache the daemon could not refresh.
    ///
    /// **An answer and not an error**, on `index::Freshness::Stale`'s reasoning: the signature was
    /// checked exactly as it would have been on a fresh copy, so an offer made from a document read
    /// three days ago is a genuine offer. It is carried because *"checked 3 days ago"* is a
    /// different sentence from *"checked just now"*, and a client working that out from
    /// [`UpdateStatus::checked_at`] and its own clock would be deriving what the daemon knows.
    #[serde(default)]
    pub stale: bool,

    /// Where this copy of MixEngine lives, and whether it may replace itself.
    ///
    /// Carried by `update.status` so a client can render the refusal *before* anybody commits to
    /// anything, rather than after a download.
    pub placement: UpdatePlacement,

    /// The services an update would stop and start again.
    ///
    /// What makes a consent prompt able to say *"3 services will be stopped and started again"* —
    /// which is `docs/features/updates.md`'s *"never update while a supervised service is under
    /// load without asking"* in the only form that rule can take once consent is always required.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub will_restart: Vec<ServiceId>,

    /// The installer this copy is updated with, present exactly when the `.pkg` installed it.
    ///
    /// **A member of its own and not a new [`UpdatePlacement`] variant**: that stays `managed` on
    /// the wire, because a tag an old client does not know would fail the whole status for it. A
    /// client that sees this offers the installer instead of the refusal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installer: Option<UpdateInstaller>,

    /// The version the daemon binary on disk reports, while it is the one handed to the installer
    /// and not the one running. It means *installed, restart to finish*.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed: Option<String>,
}

/// The installer a copy of MixEngine is updated with.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UpdateInstaller {
    /// What kind: `pkg`.
    pub kind: String,

    /// How big the download is, for this machine.
    pub size: u64,
}

/// What `update.hand_over` takes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UpdateHandOver {
    /// The version the client showed the user, refused if no longer offered, as
    /// [`UpdateApply::version`] is.
    pub version: String,
}

/// What `update.hand_over` answers: the verified package is open in the installer, and the daemon
/// is still running.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UpdateHandedOver {
    /// The version handed over.
    pub version: String,

    /// Where the verified package is. Kept until the update finishes or a newer one is offered.
    pub package: String,

    /// The line that installs [`UpdateHandedOver::package`] without a window, for a person who is
    /// not at this Mac's screen: over SSH, the installer opens on the desktop anyway, as measured
    /// on a real Mac. Written by the daemon so no client composes it.
    pub command: String,

    /// Whether a software installer opened on this machine's screen. `false` on a Linux machine
    /// with no desktop session: the package is verified and waiting, and
    /// [`UpdateHandedOver::command`] is the only way to install it.
    pub opened: bool,
}

/// What `update.finish` takes: nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UpdateFinish {}

/// One published release, as a person reads it before deciding.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UpdateRelease {
    /// The version offered.
    pub version: String,

    /// When it was published, as `YYYY-MM-DDTHH:MM:SSZ`.
    ///
    /// A string for [`DaemonStatus`](crate::DaemonStatus)' reason: it is for reading, and nothing
    /// here joins or subtracts it.
    pub published_at: String,

    /// What changed.
    pub notes: String,

    /// Where the notes somebody may have edited after the release was signed live.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes_url: Option<String>,

    /// How large the payload for *this* machine is, in bytes.
    ///
    /// Zero when this release has no build for this machine, which is the case
    /// [`UpdateStatus::because`] names.
    pub size: u64,
}

/// Where this copy of MixEngine is installed, and what that means for updating it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum UpdatePlacement {
    /// MixEngine may replace its own binaries.
    SelfUpdatable {
        /// The directory holding them. A string, for [`UpdateRelease::published_at`]'s reason.
        directory: String,
    },

    /// Something else installed this copy, and something else updates it.
    Managed {
        /// Where it is.
        directory: String,

        /// Why MixEngine will not replace it, phrased for a person and rendered unchanged.
        because: String,
    },
}

/// What an update did on its way out.
///
/// **The last thing a client hears from the daemon it just replaced.** The connection closes a
/// moment later, which *is* the update rather than a failure of it — the same shape
/// [`DaemonShutdown`](crate::DaemonShutdown) has, and for the same reason.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UpdateApplied {
    /// The version that was running.
    pub from: String,

    /// The version now on disk.
    pub to: String,

    /// The directory the binaries were replaced in.
    ///
    /// With [`UpdateApplied::replaced`], this is what a client prints when the new daemon will not
    /// start: the `.old` paths, and the command that puts them back.
    pub directory: String,

    /// The binaries that were replaced, by name.
    pub replaced: Vec<String>,

    /// The binaries the payload carried that were deliberately not replaced.
    ///
    /// `mixengine-elevate` always, and anything the payload carries that this install does not have.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kept: Vec<String>,

    /// What was stopped, and what the new daemon will start again.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub restarting: Vec<ServiceId>,
}

/// The one line `daemon.status` carries about updates.
///
/// **Deliberately smaller than [`UpdateStatus`]**: `mix status` prints one sentence and everything
/// else about an update belongs to `update.status`, which is a screen. The same split
/// [`DaemonStatus::elevation`](crate::DaemonStatus::elevation) already makes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct UpdateOffer {
    /// The version waiting.
    pub version: String,

    /// When it was published, as `YYYY-MM-DDTHH:MM:SSZ`.
    pub published_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A client from before the `.pkg` updater reads a status carrying its two members, and one
    /// from after it reads them (ADR 0019).
    #[test]
    fn a_status_with_the_installer_members_reads() {
        let status = serde_json::json!({
            "current": "0.0.8", "offered": true, "stale": false,
            "placement": {
                "kind": "managed", "directory": "/usr/local/bin",
                "because": "the .pkg installed this copy"
            },
            "installer": { "kind": "pkg", "size": 68_638_683 },
            "installed": "0.0.9"
        });

        let read: UpdateStatus = serde_json::from_value(status).expect("a new status reads");

        assert_eq!(
            read.installer,
            Some(UpdateInstaller {
                kind: "pkg".to_owned(),
                size: 68_638_683
            })
        );
        assert_eq!(read.installed.as_deref(), Some("0.0.9"));
    }

    #[test]
    fn a_status_without_them_still_reads() {
        let status = serde_json::json!({
            "current": "0.0.8", "offered": false, "stale": false,
            "placement": { "kind": "self_updatable", "directory": "/opt/mixengine" }
        });

        let read: UpdateStatus = serde_json::from_value(status).expect("an old status reads");

        assert!(
            read.installer.is_none() && read.installed.is_none(),
            "{read:?}"
        );
    }

    /// Absent rather than `null`, so a status from a copy that is not the `.pkg`'s is byte for byte
    /// what it was before the `.pkg` updater.
    #[test]
    fn a_status_without_them_writes_neither() {
        let status = UpdateStatus {
            current: "0.0.8".to_owned(),
            available: None,
            offered: false,
            because: None,
            checked_at: None,
            stale: false,
            placement: UpdatePlacement::SelfUpdatable {
                directory: "/opt/mixengine".to_owned(),
            },
            will_restart: Vec::new(),
            installer: None,
            installed: None,
        };

        let written = serde_json::to_value(&status).expect("a status writes");

        assert!(
            written.get("installer").is_none() && written.get("installed").is_none(),
            "{written}"
        );
    }

    #[test]
    fn finish_takes_no_arguments() {
        assert!(serde_json::from_value::<UpdateFinish>(serde_json::json!({ "x": 1 })).is_err());
        serde_json::from_value::<UpdateFinish>(serde_json::json!({})).expect("an empty finish");
    }

    #[test]
    fn a_handover_names_the_package_and_the_command() {
        let handed: UpdateHandedOver = serde_json::from_value(serde_json::json!({
            "version": "0.0.9",
            "package": "/x/mixlab-0.0.9-macos-universal.pkg",
            "command": "sudo installer -pkg '/x/mixlab-0.0.9-macos-universal.pkg' -target /",
            "opened": true
        }))
        .expect("a handover reads");

        assert!(handed.command.contains(&handed.package));
        serde_json::from_value::<UpdateHandOver>(serde_json::json!({ "version": "0.0.9" }))
            .expect("hand_over takes a version");
    }
}
