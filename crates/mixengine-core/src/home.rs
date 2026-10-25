//! Which home this is.
//!
//! The OS credential store is one store per *user*, not one per home, and `MIXENGINE_HOME` means a
//! user can have several: a sandbox for a real run, a suite's temporary home, a second install for
//! a different set of projects. Until this module a secret's address said only which service it
//! belonged to, so all of those wrote to one entry and the last one to bootstrap won — leaving
//! every earlier home with a server whose password it could no longer produce.
//!
//! So a home carries an identity, and [`services::handoff::secret_key`](crate::services::handoff::secret_key)
//! puts it in front of every address. What that identity is, and why it is not derived from the
//! path, is argued in `migrations-archive/0021_home_id.sql` and in
//! [ADR 0032](https://github.com/mixnz/mixlab/blob/master/docs/decisions/0032-a-keyring-address-names-the-home-it-belongs-to.md).

use crate::{Error, Result, Store};

/// The `settings` key the id lives under. Written by `0001_initial.sql`, never by this crate.
const KEY: &str = "home.id";

/// One home's identity, as every credential address spells it.
///
/// A newtype rather than a `String` because it is a *prefix on a key in the user's credential
/// store*: a value that reached one of those addresses by accident — a project name, a service id
/// — would send a read somewhere plausible and wrong, and a wrong read here is a service that
/// cannot authenticate against its own data directory.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HomeId(String);

impl HomeId {
    /// The id as it is spelled in an address.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// One from a string, or [`None`] for a value that is not the shape a home's id has.
    ///
    /// **Validated and not merely wrapped**, for the reason the newtype exists: this string is
    /// concatenated into an address in the user's credential store, and a value holding a `/`
    /// would move a read to some other service's entry rather than fail.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        is_an_id(value).then(|| Self(value.to_owned()))
    }
}

impl std::fmt::Display for HomeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// This home's id.
///
/// **Read, never minted.** `0001_initial.sql` writes it at the first migration and
/// `ON CONFLICT DO NOTHING` keeps it across every later one, so a home always has exactly one — and
/// a second one generated here, on a home whose row somehow went missing, would orphan every
/// credential already written under the first rather than recover anything.
///
/// # Errors
///
/// [`Error::Database`] when `settings` cannot be read, and [`Error::HomeHasNoId`] when the row is
/// absent or holds something that is not an id — a database that skipped the migration, or one
/// edited by hand. Both are refusals rather than a generated replacement, for the reason above.
pub async fn id(store: &Store) -> Result<HomeId> {
    let row = sqlx::query_scalar!("SELECT value_json FROM settings WHERE key = ?", KEY)
        .fetch_optional(store.pool())
        .await
        .map_err(|source| store.failure("read", source))?;

    let value: Option<String> = row
        .as_deref()
        .and_then(|json| serde_json::from_str::<String>(json).ok());

    value
        .as_deref()
        .and_then(HomeId::parse)
        .ok_or(Error::HomeHasNoId)
}

/// Whether `value` is the shape `0001_initial.sql` writes: lowercase hex, and not empty.
///
/// Checked on the way out rather than trusted, because this string is concatenated into an address
/// in the user's credential store: anything holding a `/` would move the read to another service's
/// entry, and the charset that cannot spell one is the charset this migration already produces.
fn is_an_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn home() -> (tempfile::TempDir, Store) {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let store = Store::open(&directory.path().join(crate::paths::DATABASE_FILE_NAME))
            .await
            .expect("a database");

        (directory, store)
    }

    /// The migration gives every home one, and reading it twice gives the same answer.
    #[tokio::test]
    async fn a_migrated_home_has_an_id_and_keeps_it() {
        let (_directory, store) = home().await;

        let first = id(&store).await.expect("a home has an id");
        let second = id(&store).await.expect("and keeps it");

        assert_eq!(first, second);
        assert!(is_an_id(first.as_str()), "{first}");
        assert_eq!(first.as_str().len(), 12, "six bytes of hex: {first}");
    }

    /// **Two homes are two ids**, which is the whole of what this exists for.
    #[tokio::test]
    async fn two_homes_are_told_apart() {
        let (_one, first) = home().await;
        let (_two, second) = home().await;

        assert_ne!(
            id(&first).await.expect("an id"),
            id(&second).await.expect("an id"),
            "two homes sharing an id share their credentials, which is what a home id prevents"
        );
    }

    /// A row that is not an id is refused rather than replaced — see [`id`].
    #[tokio::test]
    async fn a_row_that_is_not_an_id_is_refused() {
        let (_directory, store) = home().await;

        for written in ["\"\"", "\"AB12\"", "\"has/a/slash\"", "\"zz\"", "17"] {
            sqlx::query("UPDATE settings SET value_json = ? WHERE key = 'home.id'")
                .bind(written)
                .execute(store.pool())
                .await
                .expect("the row");

            assert!(
                matches!(id(&store).await, Err(Error::HomeHasNoId)),
                "{written} is not an id"
            );
        }
    }
}
