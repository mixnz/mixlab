//! Reading a credential, and the one compatibility path there is — roadmap task **T126**.
//!
//! # Why a module and not a method call
//!
//! T126 put the home in front of every credential address, because the OS credential store is one
//! store per *user* and `MIXENGINE_HOME` means a user can have several homes. Every address this
//! build **writes** is `<home-id>/<service-id>/<user>`; every address a home wrote before it is
//! `<service-id>/<user>`, and those entries are somebody's running servers.
//!
//! So a read that finds nothing at the new address looks at the old one, and **moves what it
//! finds**. That is the whole of the migration, and the reason it is shaped this way rather than as
//! a pass over the table at startup: there is no table. A service's ritual credentials could be
//! enumerated from the recipes, but a *database account's* cannot — the account names arrive from
//! whoever asks (`database.credentials`, `database.open`), and the only thing that knows them all
//! is the server, which cannot be asked without the credential being migrated. A lazy move covers
//! both with one rule.
//!
//! # What it deliberately does not do
//!
//! **It does not delete the old entry.** Another home on this machine may still be running a build
//! that reads it, and a migration that took the credential away from a service nobody has upgraded
//! yet would cause exactly the outage T126 exists to end. What is left behind is an entry nothing
//! writes any more; `mix doctor` is where that becomes visible, and removing it is a later task.
//!
//! **It does not check whether the value it moves is right.** An entry at the old address may
//! already be another home's — that is the bug being fixed, and by the time a home reads it the
//! damage is done. Moving it changes nothing about that and keeps the failure in one place, where
//! `services::databases` can say what it means.

use std::sync::Arc;

use mixengine_core::services::handoff;
use mixengine_platform::{Host, KEYRING_SERVICE};
use mixengine_proto::{Error, ErrorCode};

use crate::error::ToWire as _;

/// Read the credential at `address`, moving one found at its pre-T126 address.
///
/// Blocking, and called from inside a `spawn_blocking` by both of its callers: the keyring blocks,
/// and on Linux it blocks on a D-Bus round trip to a daemon that may be prompting somebody to
/// unlock it (`docs/standards/rust.md`).
///
/// # Errors
///
/// Whatever the machine's credential store says — a Linux with no secret service running, or one
/// that refused. **A failed *move* is not one of them**: the value has been read by then and the
/// caller's question is answered, so a store that would not accept the write is logged and the
/// answer is returned. The next read tries again.
pub(crate) fn secret_blocking(
    host: &dyn Host,
    service: &str,
    address: &str,
) -> mixengine_platform::Result<Option<String>> {
    let keyring = host.keyring();

    if let Some(found) = keyring.secret(service, address)? {
        return Ok(Some(found));
    }

    let Some(before) = handoff::secret_key_before_homes(address) else {
        return Ok(None);
    };

    let Some(found) = keyring.secret(service, before)? else {
        return Ok(None);
    };

    match keyring.set_secret(service, address, &found) {
        Ok(()) => tracing::info!(
            address,
            before,
            "a credential written before addresses named their home was moved to this home's"
        ),
        Err(error) => tracing::warn!(
            address,
            before,
            %error,
            "a credential written before addresses named their home could not be moved; it is \
             still readable at the old address and the next read will try again"
        ),
    }

    Ok(Some(found))
}

/// What the store says around an address a read has just missed — **names and counts, never a
/// value** — for `daemon.log`.
///
/// Written for CI run 37973392777, where a MariaDB first run stored its root credential and the
/// start 4ms later read nothing at the same address, on a Windows Credential Manager with no cache in
/// front of it. That log could not tell a credential never written from one written and then not
/// found, so a miss now says: whether a second read finds it, whether the store *lists* the address,
/// and how many entries the same home holds. A listed address a read cannot find is the store; an
/// unlisted one in a home holding others is a write that went elsewhere or a delete.
///
/// Blocking, like [`secret_blocking`], and every failure is folded into the sentence: this runs on
/// a path that has already failed, and a diagnosis that could fail it a second time would hide the
/// first reason.
pub(crate) fn after_a_miss(host: &dyn Host, service: &str, address: &str) -> String {
    let keyring = host.keyring();

    let again = match keyring.secret(service, address) {
        Ok(Some(_)) => "a second read found it".to_owned(),
        Ok(None) => "a second read found nothing either".to_owned(),
        Err(error) => format!("a second read failed: {error}"),
    };

    let listed = match keyring.keys_without_asking(service) {
        None => "the store was not listed, since listing it here could raise a dialog".to_owned(),
        Some(Ok(keys)) => {
            let named = keys.iter().any(|key| key == address);
            let home = address
                .split_once('/')
                .map(|(home, _)| home)
                .filter(|_| handoff::secret_key_before_homes(address).is_some());

            let neighbours = match home {
                Some(home) => {
                    let prefix = format!("{home}/");
                    let held = keys.iter().filter(|key| key.starts_with(&prefix)).count();
                    format!(", and {held} under this home ({home})")
                }
                None => String::new(),
            };

            format!(
                "the store {} it among {} entries for {service}{neighbours}",
                if named { "lists" } else { "does not list" },
                keys.len()
            )
        }
        Some(Err(error)) => format!("the store could not be listed: {error}"),
    };

    format!("{again}; {listed}")
}

/// [`secret_blocking`] in the `mixengine` namespace, off the runtime's threads.
///
/// # Errors
///
/// A wire error a client renders: a machine with no credential store, or one that refused.
pub(crate) async fn read(host: &Arc<dyn Host>, address: &str) -> Result<Option<String>, Error> {
    let (host, address) = (Arc::clone(host), address.to_owned());

    tokio::task::spawn_blocking(move || secret_blocking(host.as_ref(), KEYRING_SERVICE, &address))
        .await
        .map_err(|_| {
            Error::new(
                ErrorCode::Internal,
                "the task reading a credential did not finish".to_owned(),
            )
        })?
        .map_err(|error| error.to_wire())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixengine_platform::mock;

    fn machine() -> Arc<dyn Host> {
        Arc::new(mock::Host::with_home(std::env::temp_dir()))
    }

    const NEW: &str = "0123456789ab/mariadb@main/root";
    const OLD: &str = "mariadb@main/root";

    #[test]
    fn an_address_of_this_home_is_read_as_it_is() {
        let host = machine();
        host.keyring()
            .set_secret(KEYRING_SERVICE, NEW, "this home's")
            .expect("a keyring write");

        let found = secret_blocking(host.as_ref(), KEYRING_SERVICE, NEW).expect("a read");

        assert_eq!(found.as_deref(), Some("this home's"));
    }

    /// The migration: one written before T126 is found, answered, **and moved**.
    #[test]
    fn a_credential_written_before_homes_were_named_is_moved_to_this_home() {
        let host = machine();
        host.keyring()
            .set_secret(KEYRING_SERVICE, OLD, "written in 0.0.6")
            .expect("a keyring write");

        let found = secret_blocking(host.as_ref(), KEYRING_SERVICE, NEW).expect("a read");
        assert_eq!(found.as_deref(), Some("written in 0.0.6"));

        assert_eq!(
            host.keyring()
                .secret(KEYRING_SERVICE, NEW)
                .expect("a read")
                .as_deref(),
            Some("written in 0.0.6"),
            "the next read must not need the fallback again"
        );
        assert_eq!(
            host.keyring()
                .secret(KEYRING_SERVICE, OLD)
                .expect("a read")
                .as_deref(),
            Some("written in 0.0.6"),
            "and another home still running an older build must still find it"
        );
    }

    /// **This home's answer wins**, even where the old address holds something else — which on a
    /// machine that has met this bug is another home's password.
    #[test]
    fn this_homes_entry_is_never_overwritten_by_the_older_address() {
        let host = machine();
        host.keyring()
            .set_secret(KEYRING_SERVICE, NEW, "this home's")
            .expect("a keyring write");
        host.keyring()
            .set_secret(KEYRING_SERVICE, OLD, "somebody else's")
            .expect("a keyring write");

        assert_eq!(
            secret_blocking(host.as_ref(), KEYRING_SERVICE, NEW)
                .expect("a read")
                .as_deref(),
            Some("this home's")
        );
    }

    /// An address with no home in it has nothing older to fall back to.
    ///
    /// **One lookup and not two**, which is the point of asking whether the first segment *is* a
    /// home id rather than whether there is one: an address from before T126 would otherwise cost
    /// a second round trip to a key nothing ever wrote, on every miss, for ever.
    #[test]
    fn an_address_with_no_home_has_no_fallback() {
        let host = machine();

        for address in ["root", OLD, "extensions/phpmyadmin/config"] {
            assert_eq!(
                secret_blocking(host.as_ref(), KEYRING_SERVICE, address).expect("a read"),
                None,
                "{address}"
            );
        }
    }

    /// CI run 37973392777's question, from the side that can be asked: a miss says whether the
    /// store lists the address and how many entries its home holds — and never what one holds.
    #[test]
    fn a_miss_says_what_the_store_lists_and_nothing_it_holds() {
        let host = machine();
        host.keyring()
            .set_secret(
                KEYRING_SERVICE,
                "0123456789ab/postgres@main/postgres",
                "s3cret",
            )
            .expect("a keyring write");
        host.keyring()
            .set_secret(KEYRING_SERVICE, "fedcba987654/mariadb@main/root", "another")
            .expect("a keyring write");

        let said = after_a_miss(host.as_ref(), KEYRING_SERVICE, NEW);

        assert_eq!(
            said,
            "a second read found nothing either; the store does not list it among 2 entries for \
             mixengine, and 1 under this home (0123456789ab)"
        );
        assert!(
            !said.contains("s3cret") && !said.contains("another"),
            "{said}"
        );
    }

    /// The other answer the log could not give: the address is there, and a read misses it anyway.
    #[test]
    fn a_miss_on_an_address_the_store_holds_says_so() {
        let host = machine();
        host.keyring()
            .set_secret(KEYRING_SERVICE, NEW, "this home's")
            .expect("a keyring write");

        assert_eq!(
            after_a_miss(host.as_ref(), KEYRING_SERVICE, NEW),
            "a second read found it; the store lists it among 1 entries for mixengine, and 1 \
             under this home (0123456789ab)"
        );
    }

    /// An address with no home in front has no home to count.
    #[test]
    fn a_miss_on_an_address_with_no_home_counts_no_home() {
        let host = machine();

        assert_eq!(
            after_a_miss(
                host.as_ref(),
                KEYRING_SERVICE,
                "extensions/phpmyadmin/config"
            ),
            "a second read found nothing either; the store does not list it among 0 entries for \
             mixengine"
        );
    }
}
