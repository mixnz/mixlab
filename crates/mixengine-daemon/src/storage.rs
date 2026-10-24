//! Applying `--runtimes`, `--packages`, `--data` and `--logs`, and answering `--storage`.
//!
//! **The one place on this binary where a flag writes the home rather than configuring the
//! process.** Every other flag here — `--log-level`, `--index-url`, `--update-url` — says something
//! about this run and is forgotten when it ends. These four say something about the home, and they
//! have to: `runtime_installs.install_path` and `services.data_dir` are stored strings, so a
//! relocation that applied to one process would let a daemon a service manager started and one a
//! terminal started disagree about where the same home's runtimes are, while the database agreed
//! with neither. `mix runtime list` would print paths that are not there and nothing would say why.
//!
//! **Three outcomes, and the second is what makes the flag usable at all.** A value that differs
//! from what the file says is written, while the window [`storage::changeable`] describes is still
//! open. A value equal to what the file says writes nothing and complains about nothing, so a
//! launchd plist or a shell alias may carry the flag for the life of that plist rather than working
//! once. A value that differs after something has been installed fails the start — the reasoning
//! `--log-format` already carries on this binary: *fails the start rather than being ignored*,
//! because a daemon that quietly ran with its data somewhere other than where it was just told to
//! put it is the same defect with more at stake.
//!
//! **And one read that creates nothing**, which is [`report`]: the screen that offers the choice is
//! drawn before there is a daemon to ask, so the answer has to come from a process that can open a
//! database — and it must not be the reason a home, a configuration file or a database exists.

use std::path::Path;

use mixengine_core::Paths;
use mixengine_core::config::{PathOverrides, RequestedPaths};
use mixengine_core::storage::{self, Changeable};
use mixengine_core::store::Store;
use mixengine_proto::{
    Error, ErrorCode, StorageChoice, StorageDirectory, StoragePaths, StorageReport,
};

use crate::error::ToWire as _;

/// What [`apply`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Applied {
    /// Nothing was asked for, or what was asked for is what the file already said.
    Nothing,

    /// `config.toml` was rewritten, and these keys are the ones that changed.
    Written(Vec<&'static str>),
}

/// Write what this start was asked for, or say why it cannot be.
///
/// # Errors
///
/// [`ErrorCode::PreconditionFailed`] when something is installed and the request would move it,
/// and whatever the write itself refuses with — a value `[paths]` would not accept, a file that
/// cannot be parsed or replaced.
pub(crate) async fn apply(
    store: &Store,
    config_file: &Path,
    current: &PathOverrides,
    requested: &RequestedPaths,
) -> Result<Applied, Error> {
    if requested.is_empty() {
        return Ok(Applied::Nothing);
    }

    // **Before the database is asked anything.** The common case for a daemon carrying these flags
    // is one that has carried them since the day somebody chose a disk, so the answer is almost
    // always "the file already says this" — and that answer costs no query and, more importantly,
    // cannot fail once the home fills up.
    let differing = requested.differing_from(current);
    if differing.is_empty() {
        return Ok(Applied::Nothing);
    }

    let changeable = storage::changeable(store)
        .await
        .map_err(|error| error.to_wire())?;

    if let taken @ Changeable::Taken { .. } = changeable {
        return Err(Error::new(
            ErrorCode::PreconditionFailed,
            format!(
                "this home cannot be moved now: {taken}, and where they are is recorded in the \
                 database rather than worked out each time",
            ),
        )
        .with_hint(format!(
            "remove {} from the command that starts this daemon, or edit [paths] in {} and move \
             what is already there by hand",
            differing
                .iter()
                .map(|key| format!("--{key}"))
                .collect::<Vec<_>>()
                .join(" and "),
            config_file.display()
        )));
    }

    let written = mixengine_core::config::set_paths(config_file, requested)
        .map_err(|error| error.to_wire())?;

    tracing::info!(
        keys = ?written,
        config = %config_file.display(),
        "the directories that grow were moved"
    );

    Ok(Applied::Written(written))
}

/// Remove the directories the old layout named, where the move left them empty.
///
/// **`open_home` has already created the default layout by the time [`apply`] may run** — the
/// window is a question for the database, and there is no database until the home exists. So a
/// start that relocates `data/` creates `<root>/data`, writes the file, and creates the chosen
/// directory as well. Leaving the first one there would put an empty `data/` in the home of every
/// person who ever chose a disk, which is exactly the place they would go looking for their
/// databases.
///
/// **`remove_dir` and not `remove_dir_all`, and that is the whole safety argument.** It refuses a
/// directory with anything in it, so "only if it is empty" is enforced by the operating system in
/// the same call that does the removal rather than by a check this code makes first and races
/// against. Nothing here can delete a file.
///
/// A failure is not reported to anybody: the relocation succeeded, and an empty directory that
/// could not be removed is untidiness rather than a fault. `logs/` reaches this holding
/// `daemon.log` — this start has been writing to it since before the flag was read — so it stays,
/// which is what `--logs` says it does.
pub(crate) fn tidy(before: &Paths, after: &Paths) {
    let moved = [
        (before.runtimes(), after.runtimes()),
        (before.packages(), after.packages()),
        (before.data(), after.data()),
        (before.logs(), after.logs()),
    ];

    for (old, new) in moved {
        if old == new {
            continue;
        }

        match std::fs::remove_dir(old) {
            Ok(()) => tracing::info!(
                directory = %old.display(),
                now = %new.display(),
                "the directory this one replaced was empty and has been removed"
            ),
            Err(error) => tracing::debug!(
                directory = %old.display(),
                %error,
                "the directory this one replaced was left where it is"
            ),
        }
    }
}

/// Read where this home's directories are and whether that is still a question.
///
/// **It creates nothing, and that is the whole of the care this function needs.** The caller is a
/// window drawing a "choose a disk" screen before any daemon has run, so the home it is asking
/// about may not exist — and a read that answered by creating the thing it was asked about would
/// make the screen itself the reason a choice was no longer free. So: no `open_home`, which creates
/// and bootstraps; no `config::load_or_create`, which writes the template; and the database is
/// opened only once something has established that there is one, because opening SQLite is how an
/// empty database comes into existence.
///
/// A machine before its first start therefore answers with the default layout and
/// [`StorageChoice::Free`], which is both true and the answer the screen needs.
///
/// # Errors
///
/// Whatever resolving the home refuses with, a `config.toml` that does not parse, and a database
/// that is there and cannot be read.
pub(crate) async fn report(
    root_override: Option<&Path>,
    host: &dyn mixengine_platform::Host,
) -> Result<StorageReport, Error> {
    let root = mixengine_core::paths::resolve_root(root_override, host)
        .map_err(|error| error.to_wire())?;

    // `load` and not `load_or_create`: the second writes the template, and this function is a read.
    let config_file = root.join(mixengine_core::config::FILE_NAME);
    let config = if config_file.is_file() {
        mixengine_core::config::load(&config_file).map_err(|error| error.to_wire())?
    } else {
        mixengine_core::config::Config::default()
    };

    let paths = Paths::new(root.clone(), &config.paths);

    // **Asked whether the file is there before it is opened.** `Store::open_read_only` does not
    // create one either — it is the shim's door, for this reason — but reaching for it at all on a
    // machine that has never run a daemon would be asking sqlx a question about a file that is not
    // the subject: there is no database, so nothing is installed, so the choice is free.
    let changeable = if paths.database_file().is_file() {
        let store = Store::open_read_only(paths.database_file())
            .await
            .map_err(|error| error.to_wire())?;
        let answer = storage::changeable(&store)
            .await
            .map_err(|error| error.to_wire())?;
        store.close().await;
        answer
    } else {
        Changeable::Free
    };

    let directory = |path: &Path| StorageDirectory {
        path: path.display().to_string(),

        // The test `uninstall::inventory` uses to decide whether a directory needs a row of its
        // own, restated in one expression rather than in a client.
        relocated: !path.starts_with(&root),
    };

    Ok(StorageReport {
        root: root.display().to_string(),
        paths: StoragePaths {
            runtimes: directory(paths.runtimes()),
            packages: directory(paths.packages()),
            data: directory(paths.data()),
            logs: directory(paths.logs()),
        },
        changeable: choice(changeable),
    })
}

/// One `Changeable` as the wire says it.
///
/// The sentence comes from [`Changeable`]'s own `Display`, so the daemon owns it and the three
/// readers — the window, `mix storage`, a person reading JSON — are all shown the same words.
fn choice(changeable: Changeable) -> StorageChoice {
    match changeable {
        Changeable::Free => StorageChoice::Free,
        taken @ Changeable::Taken {
            runtimes,
            packages,
            services,
        } => StorageChoice::Taken {
            runtimes,
            packages,
            services,
            explanation: taken.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// A home with a `config.toml` and a database, both where `Paths` would put them.
    async fn a_home() -> (tempfile::TempDir, Store, PathBuf) {
        let home = tempfile::tempdir().expect("a temporary directory");
        let config_file = home.path().join(mixengine_core::config::FILE_NAME);
        mixengine_core::config::write_template(&config_file).expect("the template");

        let store = Store::open(&home.path().join("mixengine.db"))
            .await
            .expect("a fresh database");

        (home, store, config_file)
    }

    /// Asking for `data` and nothing else.
    ///
    /// **Relative on purpose**: `/bulk/data` is a drive-less root on Windows, which `[paths]`
    /// refuses, so an absolute Unix path here is a test that fails on one system for a reason that
    /// is not the subject. `tests/storage.rs` covers an absolute value, built from a `TempDir`.
    fn asking(directory: &str) -> RequestedPaths {
        RequestedPaths {
            data: Some(PathBuf::from(directory)),
            ..RequestedPaths::default()
        }
    }

    #[tokio::test]
    async fn nothing_asked_for_is_nothing_written() {
        let (_home, store, config_file) = a_home().await;
        let before = std::fs::read_to_string(&config_file).unwrap();

        let applied = apply(
            &store,
            &config_file,
            &PathOverrides::default(),
            &RequestedPaths::default(),
        )
        .await
        .unwrap();

        assert_eq!(applied, Applied::Nothing);
        assert_eq!(std::fs::read_to_string(&config_file).unwrap(), before);
    }

    #[tokio::test]
    async fn a_new_value_is_written_while_the_window_is_open() {
        let (_home, store, config_file) = a_home().await;

        let applied = apply(
            &store,
            &config_file,
            &PathOverrides::default(),
            &asking("bulk/data"),
        )
        .await
        .unwrap();

        assert_eq!(applied, Applied::Written(vec!["data"]));
        assert_eq!(
            mixengine_core::config::load(&config_file)
                .unwrap()
                .paths
                .data,
            Some(PathBuf::from("bulk/data"))
        );
    }

    /// **The silent no-op**, and the reason a plist may carry the flag forever.
    #[tokio::test]
    async fn asking_for_what_the_file_says_writes_nothing() {
        let (_home, store, config_file) = a_home().await;
        apply(
            &store,
            &config_file,
            &PathOverrides::default(),
            &asking("bulk/data"),
        )
        .await
        .unwrap();

        let after_first = std::fs::read_to_string(&config_file).unwrap();
        let held = mixengine_core::config::load(&config_file).unwrap().paths;

        let applied = apply(&store, &config_file, &held, &asking("bulk/data"))
            .await
            .unwrap();

        assert_eq!(applied, Applied::Nothing);
        assert_eq!(std::fs::read_to_string(&config_file).unwrap(), after_first);
    }

    /// One row is enough to close it, and the refusal names what is there and leaves the file alone.
    #[tokio::test]
    async fn a_move_after_an_install_is_refused_and_writes_nothing() {
        let (_home, store, config_file) = a_home().await;
        let before = std::fs::read_to_string(&config_file).unwrap();

        sqlx::query(
            "INSERT INTO runtime_installs
                 (kind, version, channel, install_path, installed_at, size_bytes, source_url,
                  sha256)
             VALUES ('php', '8.3.12', 'stable', '/old/runtimes/php/8.3.12', '2026-09-16', 1,
                     'https://example.invalid/php.tar.gz', 'abc')",
        )
        .execute(store.pool())
        .await
        .expect("the row");

        let error = apply(
            &store,
            &config_file,
            &PathOverrides::default(),
            &asking("bulk/data"),
        )
        .await
        .unwrap_err();

        assert_eq!(error.code, ErrorCode::PreconditionFailed);
        assert!(error.message.contains("1 runtime is installed"), "{error}");
        assert!(
            error
                .hint
                .as_deref()
                .is_some_and(|hint| hint.contains("--data")),
            "{error}"
        );
        assert_eq!(
            std::fs::read_to_string(&config_file).unwrap(),
            before,
            "a refused start left the file changed"
        );
    }

    /// And a request that only repeats what the file says is **not** refused by a full home: the
    /// window is about changing something, and this changes nothing.
    #[tokio::test]
    async fn a_full_home_still_accepts_the_flag_it_was_started_with() {
        let (_home, store, config_file) = a_home().await;
        apply(
            &store,
            &config_file,
            &PathOverrides::default(),
            &asking("bulk/data"),
        )
        .await
        .unwrap();
        let held = mixengine_core::config::load(&config_file).unwrap().paths;

        sqlx::query(
            "INSERT INTO runtime_installs
                 (kind, version, channel, install_path, installed_at, size_bytes, source_url,
                  sha256)
             VALUES ('php', '8.3.12', 'stable', '/bulk/runtimes/php/8.3.12', '2026-09-16', 1,
                     'https://example.invalid/php.tar.gz', 'abc')",
        )
        .execute(store.pool())
        .await
        .expect("the row");

        assert_eq!(
            apply(&store, &config_file, &held, &asking("bulk/data"))
                .await
                .unwrap(),
            Applied::Nothing
        );
    }
}
