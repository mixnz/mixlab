//! Whether a `keyring` failure means this machine has no credential store at all.
//!
//! **The shortest of the three, and the most exact.** `keyring`'s Credential Manager backend spends
//! `NoStorageAccess` on one Windows error and no other: `ERROR_NO_SUCH_LOGON_SESSION`. That is the
//! whole of the absent case here, because the Credential Manager is part of Windows rather than
//! something installed beside it — what a caller can lack is not the store but a logon session to
//! read it under.
//!
//! Which is a shape MixEngine can genuinely meet: a service running as `LocalSystem` or under a
//! virtual account has no user profile loaded, so there is no per-user credential vault to open.

use keyring::error::Error as KeyringError;

/// The workaround for a machine with no credential store, or `None` when it has one.
pub(crate) fn absent_store(source: &KeyringError) -> Option<&'static str> {
    matches!(source, KeyringError::NoStorageAccess(_)).then_some(
        "this process has no logon session, so Windows has no per-user Credential Manager vault to \
         open; run MixEngine as a signed-in user rather than as a service account without a \
         loaded profile",
    )
}

/// The keys under `service` — T186.
///
/// `keyring` writes every generic credential with the key as its user name and `<key>.<service>` as
/// its target. `CredEnumerateW`'s filter takes a prefix and nothing else, so this lists every
/// credential of this user and keeps the ones [`key_of`] recognises.
#[expect(
    unsafe_code,
    reason = "`CredEnumerateW` and `CredFree` are the only way to list the Credential Manager"
)]
pub(crate) fn keys(service: &str) -> Result<Vec<String>, KeyringError> {
    use windows_sys::Win32::Foundation::{
        ERROR_NO_SUCH_LOGON_SESSION, ERROR_NOT_FOUND, GetLastError,
    };
    use windows_sys::Win32::Security::Credentials::{
        CRED_TYPE_GENERIC, CREDENTIALW, CredEnumerateW, CredFree,
    };

    let mut count = 0u32;
    let mut list: *mut *mut CREDENTIALW = std::ptr::null_mut();

    // SAFETY: a null filter lists everything; `count` and `list` are written by the call.
    if unsafe { CredEnumerateW(std::ptr::null(), 0, &mut count, &mut list) } == 0 {
        // SAFETY: read straight after the failing call, on the same thread.
        let code = unsafe { GetLastError() };
        let error = Box::new(std::io::Error::from_raw_os_error(code.cast_signed()));

        return match code {
            ERROR_NOT_FOUND => Ok(Vec::new()),
            ERROR_NO_SUCH_LOGON_SESSION => Err(KeyringError::NoStorageAccess(error)),
            _ => Err(KeyringError::PlatformFailure(error)),
        };
    }

    let mut keys = Vec::new();

    // SAFETY: on success `list` holds `count` valid credential pointers until `CredFree`.
    unsafe {
        for &credential in std::slice::from_raw_parts(list, count as usize) {
            let credential = &*credential;
            if credential.Type != CRED_TYPE_GENERIC
                || credential.TargetName.is_null()
                || credential.UserName.is_null()
            {
                continue;
            }
            let (target, user) = (wide(credential.TargetName), wide(credential.UserName));
            keys.extend(key_of(&target, &user, service));
        }
        CredFree(list.cast());
    }

    Ok(keys)
}

/// The key of a credential `keyring` wrote under `service`, or `None` for anybody else's.
///
/// The target alone is ambiguous: `key.foo.mixengine` ends in `.mixengine` and belongs to service
/// `foo.mixengine`. The user name is what the target was made of, so the two together are exact.
fn key_of(target: &str, user: &str, service: &str) -> Option<String> {
    (target.len() == user.len() + 1 + service.len()
        && target.starts_with(user)
        && target[user.len()..].starts_with('.')
        && target.ends_with(service))
    .then(|| user.to_owned())
}

/// A NUL-terminated UTF-16 string, lossily.
///
/// # Safety
///
/// `pointer` points at a NUL-terminated wide string.
#[expect(
    unsafe_code,
    reason = "reads a string the Credential Manager handed out"
)]
unsafe fn wide(pointer: *const u16) -> String {
    let mut length = 0;
    // SAFETY: the caller promises a terminator.
    while unsafe { *pointer.add(length) } != 0 {
        length += 1;
    }
    // SAFETY: `length` units were just read.
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(pointer, length) })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T186: a credential is this service's only when its target is exactly `<user>.<service>`, so a
    /// service whose name ends in `.<service>` never lends it a key.
    #[test]
    fn a_key_is_read_from_the_user_name_the_target_was_made_of() {
        assert_eq!(
            key_of(
                "0123/mariadb@main/root.mixengine",
                "0123/mariadb@main/root",
                "mixengine"
            ),
            Some("0123/mariadb@main/root".to_owned())
        );
        assert_eq!(
            key_of("a.b.mixengine", "a.b", "mixengine"),
            Some("a.b".to_owned())
        );

        // `key` under a service named `foo.mixengine`.
        assert_eq!(key_of("key.foo.mixengine", "key", "mixengine"), None);
        assert_eq!(key_of("vault.MixLab", "vault", "mixengine"), None);
    }

    /// The two answers, and nothing between them.
    #[test]
    fn only_no_storage_access_is_read_as_an_absent_vault() {
        let absent = KeyringError::NoStorageAccess(Box::new(std::io::Error::other(
            "ERROR_NO_SUCH_LOGON_SESSION",
        )));

        assert!(absent_store(&absent).is_some_and(|advice| !advice.is_empty()));

        // A vault that is there and refused — the case the whole distinction exists for.
        let refused =
            KeyringError::PlatformFailure(Box::new(std::io::Error::other("ERROR_ACCESS_DENIED")));

        assert_eq!(absent_store(&refused), None);
        assert_eq!(absent_store(&KeyringError::NoEntry), None);
    }
}
