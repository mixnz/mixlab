//! The `bin_commands` table: tools somebody installed into a runtime — roadmap task **T131**.
//!
//! This module owns every read and write of that table, on [`crate::packages`]' rule. What it holds
//! is a **projection**: [`record`] replaces it whole, the way `etc/` is rebuilt from the database
//! rather than patched, so a row never outlives the file it describes by more than one scan.
//!
//! # Two readers, one row each
//!
//! The daemon reads [`all`] to compose `<root>/bin`, and a shim reads [`kind`] — one query, one
//! answer — to find out which language's version resolution decides which copy of `yarn` it is
//! about to become. That is the whole reason the table exists: a shim is handed only the name it
//! was invoked by, and deriving the language from that would mean listing the global directory of
//! every installed runtime before running any command at all.
//!
//! # What it deliberately does not hold
//!
//! **Where the file is.** A global tool belongs to *a version*, not to an install — `yarn` under
//! Node 24 is not `yarn` under Node 22 — so the path is resolved at run time, against whichever
//! version the working directory means, and a recorded path would be the wrong one the moment
//! somebody `cd`s into a project that pins another Node. What is recorded is the one fact that does
//! not vary that way: which language it belongs to.

use std::collections::BTreeMap;

use mixengine_proto::RuntimeKind;

use crate::{Error, Result, Store};

/// Every discovered command, by the name it is typed under.
///
/// # Errors
///
/// [`Error::Database`] when the table cannot be read, and [`RuntimeError::UnreadablePackageRow`] for a row
/// whose `kind` this build does not know — a hand-edited database, or a downgrade past a release
/// that fronted a language this one does not. Refused rather than skipped, because a skipped row is
/// a name in `bin/` the shim would then answer with "nothing installed here answers to this".
///
/// [`RuntimeError::UnreadablePackageRow`]: crate::RuntimeError::UnreadablePackageRow
pub async fn all(store: &Store) -> Result<BTreeMap<String, RuntimeKind>> {
    let rows = sqlx::query!("SELECT name, kind FROM bin_commands ORDER BY name")
        .fetch_all(store.pool())
        .await
        .map_err(|source| store.failure("read", source))?;

    rows.into_iter()
        .map(|row| {
            let kind = RuntimeKind::parse(&row.kind).ok_or_else(|| {
                Error::Runtime(crate::RuntimeError::UnreadablePackageRow {
                    column: "bin_commands.kind",
                    value: row.kind.clone(),
                })
            })?;

            Ok((row.name, kind))
        })
        .collect()
}

/// Which language owns one command name, and [`None`] for a name nothing discovered.
///
/// **The shim's own query**, and the reason this is not `all(...).get(name)`: a `yarn` in somebody's
/// terminal pays for one row rather than for every tool every runtime on the machine holds, and
/// roadmap task T29's budget is fifteen milliseconds for the whole of a command's resolution.
///
/// # Errors
///
/// As [`all`].
pub async fn kind(store: &Store, name: &str) -> Result<Option<RuntimeKind>> {
    let folded = fold(name);

    let found = sqlx::query_scalar!("SELECT kind FROM bin_commands WHERE name = ?", folded)
        .fetch_optional(store.pool())
        .await
        .map_err(|source| store.failure("read", source))?;

    found
        .map(|kind| {
            RuntimeKind::parse(&kind).ok_or(Error::Runtime(
                crate::RuntimeError::UnreadablePackageRow {
                    column: "bin_commands.kind",
                    value: kind,
                },
            ))
        })
        .transpose()
}

/// Replace the whole projection with what the last scan found.
///
/// **One transaction, and a delete before the insert.** A tool that was uninstalled has to leave the
/// table in the same instant the tools that are still there arrive in it: a shim reading between a
/// delete and an insert would find a `bin/` full of names it could not dispatch, and a merge would
/// leave a `yarn` in the table for ever after its Node was removed.
///
/// # Errors
///
/// [`Error::Database`] when the table cannot be written.
pub async fn record(store: &Store, found: &BTreeMap<String, RuntimeKind>) -> Result<()> {
    let mut transaction = store
        .pool()
        .begin()
        .await
        .map_err(|source| store.failure("write", source))?;

    sqlx::query!("DELETE FROM bin_commands")
        .execute(&mut *transaction)
        .await
        .map_err(|source| store.failure("write", source))?;

    for (name, kind) in found {
        let name = fold(name);
        let kind = kind.as_str();

        sqlx::query!(
            "INSERT INTO bin_commands (name, kind) VALUES (?, ?)
             ON CONFLICT (name) DO UPDATE SET kind = excluded.kind",
            name,
            kind
        )
        .execute(&mut *transaction)
        .await
        .map_err(|source| store.failure("write", source))?;
    }

    transaction
        .commit()
        .await
        .map_err(|source| store.failure("write", source))?;

    Ok(())
}

/// The spelling a name is stored and looked up under.
///
/// [`crate::shims::dispatch`]' rule, and the filesystem's rather than a courtesy: `Yarn` and `yarn`
/// are one file on Windows and two here, so two rows there would be two copies of the shim racing
/// for one name, and folding on Unix would let a tool genuinely called `YARN` answer for the other.
fn fold(name: &str) -> String {
    match cfg!(windows) {
        true => name.to_lowercase(),
        false => name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn store() -> (tempfile::TempDir, Store) {
        let home = tempfile::tempdir().expect("a temporary directory");
        let store = Store::open(&home.path().join(crate::paths::DATABASE_FILE_NAME))
            .await
            .expect("a database");
        (home, store)
    }

    fn found(entries: &[(&str, RuntimeKind)]) -> BTreeMap<String, RuntimeKind> {
        entries
            .iter()
            .map(|(name, kind)| ((*name).to_owned(), *kind))
            .collect()
    }

    /// **Written whole and read back whole.** This is a projection, so the second write is what the
    /// disk said the second time and not a merge with what it said the first — a `yarn` whose Node
    /// was uninstalled has to leave.
    #[tokio::test]
    async fn recording_replaces_what_was_there() {
        let (_home, store) = store().await;

        record(
            &store,
            &found(&[("yarn", RuntimeKind::Node), ("poetry", RuntimeKind::Python)]),
        )
        .await
        .expect("a first pass");

        record(
            &store,
            &found(&[
                ("poetry", RuntimeKind::Python),
                ("rails", RuntimeKind::Ruby),
            ]),
        )
        .await
        .expect("a second pass");

        assert_eq!(
            all(&store).await.expect("a listing"),
            found(&[
                ("poetry", RuntimeKind::Python),
                ("rails", RuntimeKind::Ruby)
            ])
        );
    }

    /// A home nothing has scanned yet answers with nothing rather than failing.
    #[tokio::test]
    async fn a_home_with_no_global_tools_lists_nothing() {
        let (_home, store) = store().await;

        assert!(all(&store).await.expect("a listing").is_empty());
    }

    /// The shim's own question: one name, one kind, and [`None`] for a name nothing discovered.
    #[tokio::test]
    async fn one_name_answers_with_its_kind() {
        let (_home, store) = store().await;

        record(&store, &found(&[("yarn", RuntimeKind::Node)]))
            .await
            .expect("a pass");

        assert_eq!(
            kind(&store, "yarn").await.expect("an answer"),
            Some(RuntimeKind::Node)
        );
        assert_eq!(kind(&store, "pnpm").await.expect("an answer"), None);
    }

    /// A word this build cannot read as a language is an error and not a row quietly skipped: a
    /// skipped row is a name in `bin/` that the shim then says nothing answers to.
    #[tokio::test]
    async fn a_kind_this_build_cannot_read_is_refused() {
        let (_home, store) = store().await;

        sqlx::query("INSERT INTO bin_commands (name, kind) VALUES ('cpanm', 'perl')")
            .execute(store.pool())
            .await
            .expect("a hand-written row");

        let error = all(&store).await.expect_err("a refusal");
        assert!(format!("{error}").contains("perl"), "{error}");

        let error = kind(&store, "cpanm").await.expect_err("a refusal");
        assert!(format!("{error}").contains("perl"), "{error}");
    }

    /// **A name is stored the way this filesystem spells one.** Windows makes `Yarn` and `yarn` one
    /// file, so it has to make them one row and answer to either; here they are two files, so a
    /// program genuinely called `YARN` must not be answered for by the other.
    ///
    /// One test over a `cfg!` rather than two behind `#[cfg]`, which is the workspace's rule and
    /// `crates/mixengine-proto/tests/workspace_layering.rs` is what holds it: both arms compile and
    /// type-check on every system, so neither can rot on the one nobody is running.
    #[tokio::test]
    async fn a_name_is_stored_the_way_this_filesystem_spells_it() {
        let (_home, store) = store().await;

        record(&store, &found(&[("Yarn", RuntimeKind::Node)]))
            .await
            .expect("a pass");

        let asked = match cfg!(windows) {
            true => "YARN",
            false => "Yarn",
        };

        assert_eq!(
            kind(&store, asked).await.expect("an answer"),
            Some(RuntimeKind::Node)
        );

        if !cfg!(windows) {
            assert_eq!(
                kind(&store, "YARN").await.expect("an answer"),
                None,
                "a different file is a different command on this system"
            );
        }
    }
}
