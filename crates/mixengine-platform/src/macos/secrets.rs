//! Whether a `keyring` failure means this machine has no credential store at all.
//!
//! **The short answer on this system, and the reason `linux/secrets.rs` is long.** `keyring`'s
//! Keychain backend spends `NoStorageAccess` on exactly the four `Security.framework` codes that
//! mean there is no keychain to open — `errSecNotAvailable`, `errSecReadOnly`, `errSecNoSuchKeychain`
//! and `errSecInvalidKeychain` — so its own vocabulary already says what this module has to answer,
//! and nothing has to be read out of a boxed source.
//!
//! A login Keychain is part of a macOS account, so the absent case is a machine used in a way macOS
//! does not really offer: a `launchd` daemon in the system context, or an SSH login into an account
//! that has never been logged into at the console.

use keyring::error::Error as KeyringError;

/// The workaround for a machine with no credential store, or `None` when it has one.
pub(crate) fn absent_store(source: &KeyringError) -> Option<&'static str> {
    matches!(source, KeyringError::NoStorageAccess(_)).then_some(
        "this session has no login Keychain to open; log in at the console once so macOS creates \
         it, or run MixEngine as a user that has",
    )
}

/// The keys under `service` — T186.
///
/// **Attributes only, never data**, so the Keychain has nothing to ask about: listing is not
/// reading a secret, whatever program does it.
pub(crate) fn keys(service: &str) -> Result<Vec<String>, KeyringError> {
    use security_framework::item::{ItemClass, ItemSearchOptions, Limit, SearchResult};

    /// `errSecItemNotFound`: nothing under this service.
    const NOT_FOUND: i32 = -25300;

    let found = ItemSearchOptions::new()
        .class(ItemClass::generic_password())
        .service(service)
        .load_attributes(true)
        .limit(Limit::All)
        .search();

    match found {
        Ok(results) => Ok(results
            .iter()
            .filter_map(SearchResult::simplify_dict)
            .filter_map(|mut attributes| attributes.remove("acct"))
            .collect()),
        Err(error) if error.code() == NOT_FOUND => Ok(Vec::new()),
        Err(error) => Err(KeyringError::PlatformFailure(Box::new(error))),
    }
}

/// Forget `service`/`account` — roadmap task **T182a**'s walk.
///
/// **By query, never by reading first.** `keyring`'s delete finds the item together with its secret
/// before it deletes, which runs the Keychain's access control: an item another build of this
/// program wrote — every release is another build, and the window is another program — was refused,
/// and the uninstall reported it still there. `SecItemDelete` by service and account asks nothing
/// of the secret, so nothing prompts and nothing refuses. Not there is already forgotten.
pub(crate) fn forget(service: &str, account: &str) -> Result<(), KeyringError> {
    forgotten(security_framework::passwords::delete_generic_password(
        service, account,
    ))
}

/// `SecItemDelete`'s answer in `keyring`'s words: not found is nothing to do.
fn forgotten(deleted: Result<(), security_framework::base::Error>) -> Result<(), KeyringError> {
    /// `errSecItemNotFound`.
    const NOT_FOUND: i32 = -25300;

    match deleted {
        Ok(()) => Ok(()),
        Err(error) if error.code() == NOT_FOUND => Ok(()),
        Err(error) => Err(KeyringError::PlatformFailure(Box::new(error))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two answers, and nothing between them.
    #[test]
    fn only_no_storage_access_is_read_as_an_absent_keychain() {
        let absent =
            KeyringError::NoStorageAccess(Box::new(std::io::Error::other("errSecNoSuchKeychain")));

        assert!(absent_store(&absent).is_some_and(|advice| !advice.is_empty()));

        // A keychain that is there and refused — the case the whole distinction exists for.
        let refused =
            KeyringError::PlatformFailure(Box::new(std::io::Error::other("errSecAuthFailed")));

        assert_eq!(absent_store(&refused), None);
        assert_eq!(absent_store(&KeyringError::NoEntry), None);
    }

    /// An item that is not there is already forgotten: `mix uninstall` run twice must not fail the
    /// second time. Every other refusal is carried as the store's own error.
    #[test]
    fn an_item_not_there_is_already_forgotten() {
        use security_framework::base::Error;

        assert!(forgotten(Err(Error::from_code(-25300))).is_ok());
        assert!(matches!(
            forgotten(Err(Error::from_code(-25293))),
            Err(KeyringError::PlatformFailure(_))
        ));
        assert!(forgotten(Ok(())).is_ok());
    }

    /// **Deleted by query, never by reading first** — the T182a walk. `keyring`'s delete finds the
    /// item *with its secret* before deleting, which runs the Keychain's access control: an item
    /// another build of this program wrote (every release is one) was refused, and the uninstall
    /// left it. `SecItemDelete` by service and account asks nothing of the secret. Ignored because
    /// it writes the login Keychain of whoever runs it; run by hand with `--ignored`.
    #[test]
    #[ignore = "writes and removes an item in this user's login Keychain"]
    fn a_flat_item_is_forgotten_by_query() {
        let service = "mixengine-test";
        let account = "deadbeef0000/forget-by-query";
        security_framework::passwords::set_generic_password(service, account, b"x").unwrap();
        assert!(keys(service).unwrap().contains(&account.to_owned()));

        forget(service, account).unwrap();

        assert!(!keys(service).unwrap().contains(&account.to_owned()));
        assert!(
            forget(service, account).is_ok(),
            "a second run reads absent"
        );
    }
}
