//! Whether the directories that grow may still be moved.
//!
//! **The location of a runtime is not a lookup, it is a row.** `runtime_installs.install_path` and
//! `services.data_dir` are `TEXT` columns holding absolute paths, written when the install happened
//! and read forever after. So `[paths]` is free to change for exactly as long as no row names a
//! path, and after that changing it is a file move and a rewrite of those rows — which this module
//! does not do and does not pretend to.
//!
//! **Three counts and no walk of the disk.** A directory that has files in it says nothing: a
//! `runtimes/` somebody copied there by hand is not an install, and an empty `data/` beside a
//! service row is a service whose data directory is about to be recreated. The rows are what record
//! a path, so the rows are what close the window.
//!
//! `logs/` is deliberately not consulted. Nothing records where a log line went, so relocating
//! `logs/` breaks nothing that already exists — it leaves the old `daemon.log` where it was, which
//! is a sentence for a person rather than a reason to refuse.

use std::fmt;

use crate::Result;
use crate::store::Store;

/// Whether `[paths]` may still be changed without moving anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Changeable {
    /// Nothing is installed. Every key may be set.
    Free,

    /// Something is, and this is what — for a sentence a person can act on.
    Taken {
        /// Rows in `runtime_installs`.
        runtimes: u32,
        /// Rows in `packages`.
        packages: u32,
        /// Rows in `services`.
        services: u32,
    },
}

impl Changeable {
    /// Is this a home where the four keys may still be set?
    #[must_use]
    pub fn is_free(&self) -> bool {
        matches!(self, Self::Free)
    }
}

impl fmt::Display for Changeable {
    /// What is installed, in words, for a refusal and for a window.
    ///
    /// Only the non-zero counts are named: *"1 runtime and 2 services are installed"* rather than a
    /// sentence carrying a zero the reader has to discard.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self::Taken {
            runtimes,
            packages,
            services,
        } = self
        else {
            return f.write_str("nothing is installed yet");
        };

        let counted = [
            (*runtimes, "runtime"),
            (*packages, "package"),
            (*services, "service"),
        ];

        let mut named: Vec<String> = Vec::new();
        for (count, noun) in counted {
            if count > 0 {
                named.push(format!(
                    "{count} {noun}{}",
                    if count == 1 { "" } else { "s" }
                ));
            }
        }

        // Unreachable through `changeable`, which only produces `Taken` when something is there.
        // Written rather than asserted because this is a `Display` and a panic in one is a panic
        // inside whatever was formatting an error message.
        let Some(last) = named.pop() else {
            return f.write_str("nothing is installed yet");
        };

        if named.is_empty() {
            return write!(f, "{last} is installed");
        }

        write!(f, "{} and {last} are installed", named.join(", "))
    }
}

/// Read whether this home's `[paths]` may still be changed.
///
/// # Errors
///
/// [`Error::Database`](crate::Error::Database) when a table cannot be read.
pub async fn changeable(store: &Store) -> Result<Changeable> {
    // Three statements rather than one `UNION ALL`: `sqlx::query_scalar!` checks each against the
    // schema at compile time, and a union would hand it one row of three columns whose names come
    // from nowhere the checker can see.
    let runtimes = sqlx::query_scalar!("SELECT COUNT(*) FROM runtime_installs")
        .fetch_one(store.pool())
        .await
        .map_err(|source| store.failure("read", source))?;

    let packages = sqlx::query_scalar!("SELECT COUNT(*) FROM packages")
        .fetch_one(store.pool())
        .await
        .map_err(|source| store.failure("read", source))?;

    let services = sqlx::query_scalar!("SELECT COUNT(*) FROM services")
        .fetch_one(store.pool())
        .await
        .map_err(|source| store.failure("read", source))?;

    if runtimes == 0 && packages == 0 && services == 0 {
        return Ok(Changeable::Free);
    }

    // Saturating rather than fallible: these are counts of things on one machine, and a home with
    // four billion runtimes in it has a problem this sentence is not going to help with.
    let count = |value: i64| u32::try_from(value).unwrap_or(u32::MAX);

    Ok(Changeable::Taken {
        runtimes: count(runtimes),
        packages: count(packages),
        services: count(services),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A store with nothing in it, at its own temporary path.
    async fn a_store() -> (tempfile::TempDir, Store) {
        let home = tempfile::tempdir().expect("a temporary directory");
        let store = Store::open(&home.path().join("mixengine.db"))
            .await
            .expect("a fresh database");

        (home, store)
    }

    #[tokio::test]
    async fn an_empty_home_may_still_choose() {
        let (_home, store) = a_store().await;

        assert_eq!(changeable(&store).await.unwrap(), Changeable::Free);
        assert!(changeable(&store).await.unwrap().is_free());
    }

    /// One row is enough, and it is the row rather than the directory that closes the window.
    #[tokio::test]
    async fn one_runtime_row_closes_it() {
        let (_home, store) = a_store().await;

        sqlx::query(
            "INSERT INTO runtime_installs
                 (kind, version, channel, install_path, installed_at, size_bytes, source_url,
                  sha256)
             VALUES ('php', '8.3.12', 'stable', '/somewhere/runtimes/php/8.3.12', '2026-09-16',
                     1, 'https://example.invalid/php.tar.gz', 'abc')",
        )
        .execute(store.pool())
        .await
        .expect("the row");

        assert_eq!(
            changeable(&store).await.unwrap(),
            Changeable::Taken {
                runtimes: 1,
                packages: 0,
                services: 0,
            }
        );
    }

    /// The sentence names what is there and never a zero.
    #[test]
    fn what_is_installed_reads_as_a_sentence() {
        assert_eq!(
            Changeable::Free.to_string(),
            "nothing is installed yet",
            "the free case still has to render, because a window prints it"
        );

        assert_eq!(
            Changeable::Taken {
                runtimes: 1,
                packages: 0,
                services: 0,
            }
            .to_string(),
            "1 runtime is installed"
        );

        assert_eq!(
            Changeable::Taken {
                runtimes: 3,
                packages: 0,
                services: 2,
            }
            .to_string(),
            "3 runtimes and 2 services are installed"
        );

        assert_eq!(
            Changeable::Taken {
                runtimes: 3,
                packages: 1,
                services: 2,
            }
            .to_string(),
            "3 runtimes, 1 package and 2 services are installed"
        );
    }
}
