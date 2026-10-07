//! The versions an update left installed beside the one it moved to.
//!
//! An update within a line can end with the old version still there: the person asked to keep
//! it, a project pins it, or removing it failed. The listing would then offer the very same update
//! again forever, since the old version is still installed and the new one is still newer. This
//! remembers which update each kept version already had, so the listing can leave it out — and
//! offer the next release of the line, when there is one, as it would any other.
//!
//! Kept in one `settings` row rather than a column: it is a note about a past decision, read only
//! by the listing, and forgetting it costs one question asked again.

use std::collections::BTreeMap;

use crate::{Result, Store};

/// The `settings` key. Its value maps [`subject`] to the version the update moved to.
pub const KEY: &str = "upgrade.kept";

/// What one entry is about: a runtime or a package, and the version that was kept.
#[must_use]
pub fn subject(family: Family, name: &str, version: &str) -> String {
    let family = match family {
        Family::Runtime => "runtime",
        Family::Package => "package",
    };
    format!("{family}/{name}/{version}")
}

/// Runtimes and packages each have their own updates, and a name may be both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// A language runtime: `runtime.upgrade`.
    Runtime,
    /// A server package: `package.upgrade`.
    Package,
}

/// Every version kept beside an update, and the version each was kept beside.
///
/// # Errors
///
/// [`crate::Error`] when the database cannot be read.
pub async fn all(store: &Store) -> Result<BTreeMap<String, String>> {
    Ok(crate::updates::records::get(store, KEY)
        .await?
        .unwrap_or_default())
}

/// Remembers that `version` was kept when the update moved it to `to`.
///
/// # Errors
///
/// [`crate::Error`] when the row cannot be read or written.
pub async fn remember(
    store: &Store,
    family: Family,
    name: &str,
    version: &str,
    to: &str,
) -> Result<()> {
    let mut kept = all(store).await?;
    kept.insert(subject(family, name, version), to.to_owned());
    crate::updates::records::set(store, KEY, &kept).await
}

/// Forgets `version`: it was uninstalled, and a copy installed again later starts with no history.
///
/// # Errors
///
/// [`crate::Error`] when the row cannot be read or written.
pub async fn forget(store: &Store, family: Family, name: &str, version: &str) -> Result<()> {
    let mut kept = all(store).await?;
    if kept.remove(&subject(family, name, version)).is_none() {
        return Ok(());
    }
    if kept.is_empty() {
        crate::updates::records::clear(store, KEY).await
    } else {
        crate::updates::records::set(store, KEY, &kept).await
    }
}

/// Whether the listing should leave out the update from `version` to `to`: the update already ran
/// and kept `version`, and `to` is still installed. A newer `to`, or `to` uninstalled since, is
/// offered again.
#[must_use]
pub fn already_kept(
    kept: &BTreeMap<String, String>,
    family: Family,
    name: &str,
    version: &str,
    to: &str,
    to_installed: bool,
) -> bool {
    to_installed
        && kept
            .get(&subject(family, name, version))
            .is_some_and(|beside| beside == to)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kept() -> BTreeMap<String, String> {
        BTreeMap::from([(
            subject(Family::Runtime, "node", "24.19.0"),
            "24.21.0".to_owned(),
        )])
    }

    #[test]
    fn an_update_that_kept_its_version_is_not_offered_again() {
        assert!(already_kept(
            &kept(),
            Family::Runtime,
            "node",
            "24.19.0",
            "24.21.0",
            true
        ));
    }

    #[test]
    fn a_newer_release_is_offered_again() {
        assert!(!already_kept(
            &kept(),
            Family::Runtime,
            "node",
            "24.19.0",
            "24.22.0",
            true
        ));
    }

    #[test]
    fn an_update_whose_target_was_uninstalled_is_offered_again() {
        assert!(!already_kept(
            &kept(),
            Family::Runtime,
            "node",
            "24.19.0",
            "24.21.0",
            false
        ));
    }

    #[test]
    fn a_package_of_the_same_name_is_another_subject() {
        assert!(!already_kept(
            &kept(),
            Family::Package,
            "node",
            "24.19.0",
            "24.21.0",
            true
        ));
    }
}
