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
    match security_framework::passwords::delete_generic_password(service, account) {
        // **What another program wrote, Apple's own tool deletes** — the T182a walk. The Keychain
        // refuses a delete from a program that is not in the item's access list, and to it every
        // release of this program, and the window, is another program: `errSecInvalidOwnerEdit`
        // (-25244), or `errSecAuthFailed` (-25293) on an older system. `security` is let through,
        // and it deletes by service and account exactly as the call above would have.
        Err(error) if the_owner_refused(&error) => {
            let output = std::process::Command::new(SECURITY)
                .args(["delete-generic-password", "-s", service, "-a", account])
                .output()
                .map_err(|error| KeyringError::PlatformFailure(Box::new(error)))?;
            tool_said(
                output.status.code().unwrap_or(-1),
                &String::from_utf8_lossy(&output.stderr),
            )
        }
        deleted => forgotten(deleted),
    }
}

/// Part of the base system, and named absolutely so the caller's `PATH` cannot decide what runs.
const SECURITY: &str = "/usr/bin/security";

/// Whether the Keychain refused the delete over who wrote the item.
fn the_owner_refused(error: &security_framework::base::Error) -> bool {
    /// `errSecInvalidOwnerEdit`, and `errSecAuthFailed`.
    const OWNER: [i32; 2] = [-25244, -25293];
    OWNER.contains(&error.code())
}

/// `security delete-generic-password`'s answer, in `keyring`'s words: deleted, or never there.
fn tool_said(code: i32, stderr: &str) -> Result<(), KeyringError> {
    if code == 0 || stderr.contains("could not be found") {
        return Ok(());
    }
    Err(KeyringError::PlatformFailure(Box::new(
        std::io::Error::other(format!("security exited with {code}: {}", stderr.trim())),
    )))
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

    /// The Keychain refuses to let one program delete what another wrote, with
    /// `errSecInvalidOwnerEdit` or `errSecAuthFailed`, and every release of this program is another
    /// program to it. Those two go to Apple's own tool, which the Keychain lets through; nothing
    /// else does.
    #[test]
    fn only_a_refusal_over_the_owner_goes_to_the_apple_tool() {
        use security_framework::base::Error;

        assert!(the_owner_refused(&Error::from_code(-25244)));
        assert!(the_owner_refused(&Error::from_code(-25293)));
        assert!(!the_owner_refused(&Error::from_code(-25300)));
        assert!(!the_owner_refused(&Error::from_code(-25291)));
    }

    /// What the tool says is read the way `forgotten` reads the API: gone or never there is done.
    #[test]
    fn the_apple_tools_answer_is_read_like_the_apis() {
        assert!(tool_said(0, "").is_ok());
        assert!(tool_said(44, "security: SecKeychainSearchCopyNext: The specified item could not be found in the keychain.").is_ok());
        assert!(matches!(
            tool_said(1, "security: something else"),
            Err(KeyringError::PlatformFailure(_))
        ));
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
