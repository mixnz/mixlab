//! Removing the program the macOS `.pkg` placed — roadmap task **T182a**, spec D2.
//!
//! Every path is `mixengine_platform::install::package_paths()`'s, compiled in; the request carries
//! none, on `helper::remove`'s rule. The bundle goes only when its `Info.plist` names MixLab, so
//! somebody else's application at that path is left where it is and named in the outcome.

use mixengine_platform::install::{self, PackageRemoval};
use mixengine_proto::privileged::OpOutcome;

/// `PackageRemove {}`: the four binaries, the bundle when it is MixLab's, then the receipt.
pub(crate) fn remove() -> OpOutcome {
    let Some(paths) = install::package_paths() else {
        return OpOutcome::Unsupported {
            reason: "only the macOS package places the program this way".to_owned(),
        };
    };

    match install::remove_package(&paths, &mut install::forget_receipt) {
        Ok(removal) => outcome(removal),
        Err(error) => OpOutcome::Failed {
            message: mixengine_proto::flatten(&error),
        },
    }
}

/// What a person reads in the report and the audit log: each thing that went, a bundle left alone
/// with the identity it carried, and the receipt. Nothing of any of it is `AlreadyDone`.
fn outcome(removal: PackageRemoval) -> OpOutcome {
    if removal.removed.is_empty() && removal.kept_bundle.is_none() && !removal.receipt_forgotten {
        return OpOutcome::AlreadyDone;
    }

    let mut parts = Vec::new();
    if !removal.removed.is_empty() {
        let paths: Vec<String> = removal
            .removed
            .iter()
            .map(|path| path.display().to_string())
            .collect();
        parts.push(format!("removed {}", paths.join(", ")));
    }
    if let Some(identity) = &removal.kept_bundle {
        parts.push(format!(
            "left the application bundle alone, because it is {identity} and not MixLab"
        ));
    }
    if removal.receipt_forgotten {
        parts.push("forgot the package receipt".to_owned());
    }

    OpOutcome::Applied {
        detail: parts.join("; "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Off macOS there is nothing to remove this way, and the answer says so rather than touching a
    /// path that means something else there.
    #[test]
    fn off_macos_the_operation_is_unsupported() {
        if cfg!(target_os = "macos") {
            return;
        }
        assert!(matches!(remove(), OpOutcome::Unsupported { .. }));
    }

    /// The outcome sentence is built from what was removed, not from the request: a kept bundle is
    /// named with the identity it carried.
    #[test]
    fn a_kept_bundle_is_named_in_the_outcome() {
        let removal = mixengine_platform::install::PackageRemoval {
            removed: vec![std::path::PathBuf::from("/usr/local/bin/mix")],
            kept_bundle: Some("com.example.other".to_owned()),
            receipt_forgotten: false,
        };
        let OpOutcome::Applied { detail } = outcome(removal) else {
            panic!("applied");
        };
        assert!(detail.contains("/usr/local/bin/mix"), "{detail}");
        assert!(detail.contains("com.example.other"), "{detail}");
    }

    /// A second run finds nothing, and that is not a change.
    #[test]
    fn nothing_removed_and_no_receipt_is_already_done() {
        assert!(matches!(
            outcome(mixengine_platform::install::PackageRemoval::default()),
            OpOutcome::AlreadyDone
        ));
    }
}
