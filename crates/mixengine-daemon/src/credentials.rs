//! Which credential store this daemon keeps passwords in — roadmap tasks **T184** and **T194**,
//! ADRs 0052 and 0059.
//!
//! **A property of the home, recorded in `settings`** (T194, D1). The build decides the default a
//! home starts with — a release keeps the operating system's store, anything else a file in its own
//! home — and a person may change it once: on a release, only to the file, only on Linux, and only
//! where the operating system has no store at all (D2). Every start reads the row, so a start with
//! no flag — `mix`'s autostart, a systemd unit, the window — gets the store the home chose.
//!
//! The rules are the pure functions below; the two keyring reads they need are made by the
//! caller, off the runtime's threads.

use std::path::Path;

use mixengine_core::home::HomeId;
use mixengine_platform::{Credentials, KEYRING_SERVICE, Keyring};
use mixengine_proto::Error;

use crate::error::ToWire as _;

/// The `settings` key the choice is recorded under.
pub(crate) const SETTING: &str = "credential_store";

/// The sentence a release gives for a file it will not keep, where the system is not Linux.
const NOT_HERE: &str = "--credential-store home is for development builds and for a Linux \
                        machine with no credential store: a release on this system keeps its \
                        passwords in the operating system's credential store, and will not start \
                        keeping them in a file";

/// What `--credential-store` accepts, and what the `settings` row holds.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Store {
    /// The operating system's credential store.
    Os,
    /// `<root>/credentials.json`.
    Home,
}

impl Store {
    /// The store a home with no recorded choice uses: ADR 0052's default.
    pub(crate) fn default_for(release: bool) -> Self {
        if release { Self::Os } else { Self::Home }
    }

    /// What the platform layer is handed.
    pub(crate) fn at(self, credentials_file: &Path) -> Credentials {
        match self {
            Self::Os => Credentials::Os,
            Self::Home => Credentials::File(credentials_file.to_path_buf()),
        }
    }
}

/// What this daemon knows about its store for the run: which one it built its host with, and
/// whether it would accept a switch — `daemon.status` answers both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Facts {
    pub(crate) store: Store,
    pub(crate) choosable: bool,
}

/// Refuse, before the home is opened, the one request no recorded row can make right.
///
/// # Errors
///
/// A sentence for stderr when a release on Windows or macOS is asked to keep its passwords in a
/// file.
pub(crate) fn refuse_early(release: bool, os: &str, asked: Option<Store>) -> Result<(), String> {
    if release && asked == Some(Store::Home) && os != "linux" {
        return Err(NOT_HERE.to_owned());
    }
    Ok(())
}

/// What a start does with the flag it was given and the row the home holds.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AtStart {
    /// Build the host on this store; record nothing.
    Use(Store),
    /// A release was asked for a store the home has not chosen: check it, record it, then use it.
    Switch { from: Store, to: Store },
}

/// D1's precedence: the flag, then the row, then the build's default.
pub(crate) fn at_start(release: bool, asked: Option<Store>, recorded: Option<Store>) -> AtStart {
    let current = recorded.unwrap_or(Store::default_for(release));
    match asked {
        None => AtStart::Use(current),
        // A development build's flag is for this start: CI sets it for one run (ADR 0052).
        Some(store) if !release => AtStart::Use(store),
        Some(store) if store == current => AtStart::Use(current),
        Some(to) => AtStart::Switch { from: current, to },
    }
}

/// The addresses `keyring` holds under this home, or why it would not say.
///
/// **An absent store holds nothing** — the headless machine this exists for — and **a store that
/// refuses is not empty**: a locked keyring or an unreadable file is a reason to leave the store as
/// it is, never a reading of "nothing to lose".
///
/// # Errors
///
/// A sentence naming the store's own complaint.
pub(crate) fn held_for(keyring: &dyn Keyring, home: &HomeId) -> Result<Vec<String>, String> {
    match keyring.keys(KEYRING_SERVICE) {
        Ok(keys) => {
            let prefix = format!("{home}/");
            Ok(keys
                .into_iter()
                .filter(|key| key.starts_with(&prefix))
                .collect())
        }
        Err(mixengine_platform::Error::UnsupportedPlatform { .. }) => Ok(Vec::new()),
        Err(error) => Err(format!(
            "the credential store this home uses would not say what it holds ({}), so it stays \
             the store",
            with_causes(&error)
        )),
    }
}

/// `error` and every cause under it, so a refusal carries the store's own complaint and not only
/// what MixEngine was trying to do.
fn with_causes(error: &dyn std::error::Error) -> String {
    let mut sentence = error.to_string();
    let mut cause = error.source();
    while let Some(next) = cause {
        sentence.push_str(": ");
        sentence.push_str(&next.to_string());
        cause = next.source();
    }
    sentence
}

/// Whether `keyring` — the operating system's — is absent from this session altogether.
pub(crate) fn os_store_absent(keyring: &dyn Keyring) -> bool {
    matches!(
        keyring.keys(KEYRING_SERVICE),
        Err(mixengine_platform::Error::UnsupportedPlatform { .. })
    )
}

/// Whether a switch to `to` is accepted (D2, D4).
///
/// # Errors
///
/// A sentence saying why not.
pub(crate) fn switch(
    release: bool,
    os: &str,
    to: Store,
    os_store_absent: bool,
    held: Result<Vec<String>, String>,
) -> Result<(), String> {
    if release && to == Store::Home {
        if os != "linux" {
            return Err(NOT_HERE.to_owned());
        }
        if !os_store_absent {
            return Err(
                "this machine has a credential store, and a release keeps a home's \
                        passwords there; a file is for a Linux machine that has none"
                    .to_owned(),
            );
        }
    }

    let held = held?;
    if !held.is_empty() {
        return Err(format!(
            "this home already keeps {} in its credential store ({}), and a switch would leave \
             them where nothing reads them; the store stays as it is",
            if held.len() == 1 {
                "a password"
            } else {
                "passwords"
            },
            held.join(", ")
        ));
    }

    Ok(())
}

/// Whether this daemon offers a switch at all: `daemon.status`'s `choosable`.
pub(crate) fn choosable(release: bool, os: &str, running: Store, os_store_absent: bool) -> bool {
    !release || running == Store::Home || (os == "linux" && os_store_absent)
}

/// The home's recorded choice, if it made one.
///
/// # Errors
///
/// The wire error of a database that cannot be read. A row this build cannot decode reads as no
/// row.
pub(crate) async fn recorded(store: &mixengine_core::Store) -> Result<Option<Store>, Error> {
    mixengine_core::updates::records::get::<Store>(store, SETTING)
        .await
        .map_err(|error| error.to_wire())
}

/// Record the home's choice.
///
/// # Errors
///
/// The wire error of a row that cannot be written.
pub(crate) async fn record(store: &mixengine_core::Store, to: Store) -> Result<(), Error> {
    mixengine_core::updates::records::set(store, SETTING, &to)
        .await
        .map_err(|error| error.to_wire())
}

/// Check a switch from `from` to `to` against the machine: the two keyring reads, off the
/// runtime's threads, then [`switch`].
///
/// # Errors
///
/// A sentence saying why not, or that the check could not be made.
pub(crate) async fn check_switch(
    store: &mixengine_core::Store,
    file: &Path,
    release: bool,
    os: &'static str,
    from: Store,
    to: Store,
) -> Result<(), String> {
    let home = mixengine_core::home::id(store)
        .await
        .map_err(|error| error.to_string())?;
    let file = file.to_path_buf();

    tokio::task::spawn_blocking(move || {
        let absent =
            release && to == Store::Home && os_store_absent(mixengine_platform::host().keyring());
        let current = mixengine_platform::host_with(from.at(&file));
        switch(release, os, to, absent, held_for(current.keyring(), &home))
    })
    .await
    .map_err(|_| "the check of this home's credential store did not finish".to_owned())?
}

/// The startup log line's words for `credentials`.
pub(crate) fn describe(credentials: &Credentials, release: bool) -> String {
    match credentials {
        Credentials::Os => "the operating system's credential store".to_owned(),
        Credentials::File(path) if release => format!("{} (chosen for this home)", path.display()),
        Credentials::File(path) => format!("{} (a development build)", path.display()),
    }
}

impl From<Store> for mixengine_proto::CredentialStore {
    fn from(store: Store) -> Self {
        match store {
            Store::Os => Self::Os,
            Store::Home => Self::Home,
        }
    }
}

impl From<mixengine_proto::CredentialStore> for Store {
    fn from(store: mixengine_proto::CredentialStore) -> Self {
        match store {
            mixengine_proto::CredentialStore::Os => Self::Os,
            mixengine_proto::CredentialStore::Home => Self::Home,
        }
    }
}

/// `mix doctor`'s line about the store — roadmap task T194, D5. A note and never a problem: a file
/// store is a choice somebody made, and there is nothing to repair.
pub(crate) fn check(store: Store, file: &Path) -> mixengine_proto::Check {
    use mixengine_proto::Outcome;

    mixengine_proto::Check {
        name: "where this home keeps its passwords".to_owned(),
        outcome: match store {
            Store::Os => Outcome::Ok {},
            Store::Home => Outcome::Note {
                because: format!(
                    "this home keeps its passwords in {}, a file only your account can read. \
                     Other accounts on this machine cannot read it; anyone holding the disk or a \
                     backup of this home can, and encrypting the whole disk is what protects a \
                     disk that leaves the machine",
                    file.display()
                ),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixengine_platform::{Host as _, mock};

    const HOME: &str = "0123456789ab";

    fn home() -> HomeId {
        HomeId::parse(HOME).expect("a home id")
    }

    #[test]
    fn a_release_refuses_home_early_everywhere_but_linux() {
        for os in ["windows", "macos"] {
            let refused = refuse_early(true, os, Some(Store::Home)).expect_err(os);
            assert!(refused.contains("--credential-store home"), "{refused}");
        }
        assert_eq!(refuse_early(true, "linux", Some(Store::Home)), Ok(()));
        assert_eq!(refuse_early(true, "windows", Some(Store::Os)), Ok(()));
        assert_eq!(refuse_early(false, "windows", Some(Store::Home)), Ok(()));
    }

    #[test]
    fn the_recorded_store_wins_over_the_default_and_a_flag_wins_over_both() {
        assert_eq!(at_start(true, None, None), AtStart::Use(Store::Os));
        assert_eq!(at_start(false, None, None), AtStart::Use(Store::Home));
        // Review Focus 1: no flag, a release that chose the file, keeps the file.
        assert_eq!(
            at_start(true, None, Some(Store::Home)),
            AtStart::Use(Store::Home)
        );
        assert_eq!(
            at_start(false, None, Some(Store::Os)),
            AtStart::Use(Store::Os)
        );
        assert_eq!(
            at_start(true, Some(Store::Os), None),
            AtStart::Use(Store::Os)
        );
        assert_eq!(
            at_start(true, Some(Store::Home), None),
            AtStart::Switch {
                from: Store::Os,
                to: Store::Home
            }
        );
        assert_eq!(
            at_start(true, Some(Store::Os), Some(Store::Home)),
            AtStart::Switch {
                from: Store::Home,
                to: Store::Os
            }
        );
    }

    /// Review Focus 4: CI sets `MIXENGINE_CREDENTIAL_STORE=os` for one run of a development build.
    #[test]
    fn a_development_builds_flag_is_for_that_start_and_records_nothing() {
        assert_eq!(
            at_start(false, Some(Store::Os), None),
            AtStart::Use(Store::Os)
        );
        assert_eq!(
            at_start(false, Some(Store::Os), Some(Store::Home)),
            AtStart::Use(Store::Os)
        );
        assert_eq!(
            at_start(false, Some(Store::Home), Some(Store::Os)),
            AtStart::Use(Store::Home)
        );
    }

    #[test]
    fn what_a_store_holds_is_what_is_under_this_homes_address() {
        let host = mock::Host::with_home(std::env::temp_dir());
        let keyring = host.keyring();
        keyring
            .set_secret(KEYRING_SERVICE, &format!("{HOME}/mariadb@main/root"), "a")
            .unwrap();
        keyring
            .set_secret(KEYRING_SERVICE, "fedcba987654/postgres@main/postgres", "b")
            .unwrap();

        assert_eq!(
            held_for(keyring, &home()),
            Ok(vec![format!("{HOME}/mariadb@main/root")])
        );
    }

    #[test]
    fn an_absent_store_holds_nothing() {
        let host = mock::Host::without_keyring(std::env::temp_dir(), "no secret service");
        assert_eq!(held_for(host.keyring(), &home()), Ok(Vec::new()));
        assert!(os_store_absent(host.keyring()));
    }

    /// Review Focus 3: a store that refuses is not an empty one.
    #[test]
    fn a_store_that_refuses_is_not_empty() {
        #[derive(Debug)]
        struct Locked;
        impl Keyring for Locked {
            fn secret(&self, _: &str, _: &str) -> mixengine_platform::Result<Option<String>> {
                unreachable!()
            }
            fn set_secret(&self, _: &str, _: &str, _: &str) -> mixengine_platform::Result<()> {
                unreachable!()
            }
            fn forget_secret(&self, _: &str, _: &str) -> mixengine_platform::Result<()> {
                unreachable!()
            }
            fn keys(&self, service: &str) -> mixengine_platform::Result<Vec<String>> {
                Err(mixengine_platform::Error::Secret {
                    action: "list",
                    service: service.to_owned(),
                    key: "*".to_owned(),
                    source: "the keyring is locked".into(),
                })
            }
        }

        let refused = held_for(&Locked, &home()).expect_err("a refusal");
        assert!(refused.contains("locked"), "{refused}");
        assert!(!os_store_absent(&Locked));
    }

    #[test]
    fn a_release_switches_to_the_file_only_on_linux_with_no_store() {
        assert_eq!(switch(true, "linux", Store::Home, true, Ok(vec![])), Ok(()));
        // Review Focus 2: a Linux desktop with a keyring.
        let refused = switch(true, "linux", Store::Home, false, Ok(vec![])).expect_err("a keyring");
        assert!(refused.contains("credential store"), "{refused}");
        for os in ["windows", "macos"] {
            let refused = switch(true, os, Store::Home, true, Ok(vec![])).expect_err(os);
            assert!(refused.contains("--credential-store home"), "{refused}");
        }
        assert_eq!(
            switch(false, "windows", Store::Home, false, Ok(vec![])),
            Ok(())
        );
        assert_eq!(switch(true, "macos", Store::Os, false, Ok(vec![])), Ok(()));
    }

    #[test]
    fn a_switch_is_refused_while_the_current_store_holds_anything_and_names_it() {
        let address = format!("{HOME}/mariadb@main/root");
        let refused = switch(true, "linux", Store::Os, false, Ok(vec![address.clone()]))
            .expect_err("something is stored");
        assert!(refused.contains(&address), "{refused}");

        let refused = switch(false, "linux", Store::Os, false, Err("locked".to_owned()))
            .expect_err("the store refused");
        assert!(refused.contains("locked"), "{refused}");
    }

    #[test]
    fn a_switch_is_offered_where_it_would_be_accepted() {
        assert!(choosable(false, "windows", Store::Home, false));
        assert!(choosable(true, "linux", Store::Home, false));
        assert!(choosable(true, "linux", Store::Os, true));
        assert!(!choosable(true, "linux", Store::Os, false));
        assert!(!choosable(true, "windows", Store::Os, true));
    }

    #[test]
    fn the_store_is_recorded_as_a_word() {
        assert_eq!(serde_json::to_string(&Store::Home).unwrap(), "\"home\"");
        assert_eq!(serde_json::from_str::<Store>("\"os\"").unwrap(), Store::Os);
    }

    #[test]
    fn the_file_is_the_one_the_home_names() {
        let file = Path::new("/home/me/MixEngine-dev/credentials.json");

        assert_eq!(Store::Home.at(file), Credentials::File(file.to_path_buf()));
        assert_eq!(Store::Os.at(file), Credentials::Os);
    }

    #[test]
    fn the_log_line_says_which_store_why_and_never_a_value() {
        let file = Path::new("/home/me/MixEngine-dev/credentials.json");

        assert_eq!(
            describe(&Credentials::File(file.to_path_buf()), false),
            "/home/me/MixEngine-dev/credentials.json (a development build)"
        );
        assert_eq!(
            describe(&Credentials::File(file.to_path_buf()), true),
            "/home/me/MixEngine-dev/credentials.json (chosen for this home)"
        );
        assert_eq!(
            describe(&Credentials::Os, true),
            "the operating system's credential store"
        );
    }

    /// T194, D5: a note for the file, never a problem, and nothing for the OS store.
    #[test]
    fn the_doctor_notes_a_file_store_and_passes_the_os_one() {
        use mixengine_proto::Outcome;

        let file = Path::new("/h/credentials.json");

        assert_eq!(check(Store::Os, file).outcome, Outcome::Ok {});
        match check(Store::Home, file).outcome {
            Outcome::Note { because } => {
                assert!(because.contains("/h/credentials.json"), "{because}");
                assert!(because.contains("disk"), "{because}");
            }
            other => panic!("a note, not {other:?}"),
        }
    }
}
