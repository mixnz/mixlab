//! The rows for the passwords this home keeps, in the system's credential store or in its own file.

use mixengine_proto::{Removal, Residue, ResidueId};

use super::super::Uninstall;

use super::{Found, is_the_windows_home};

/// Read the store for [`credential_rows`].
pub(crate) async fn credentials(uninstall: &Uninstall) -> Found {
    let home = mixengine_core::home::id(&uninstall.store)
        .await
        .ok()
        .map(|id| id.as_str().to_owned());
    let window = is_the_windows_home(uninstall);
    let host = std::sync::Arc::clone(&uninstall.host);

    let (daemon, window) = tokio::task::spawn_blocking(move || {
        let keyring = host.keyring();
        (
            keyring.keys(mixengine_platform::KEYRING_SERVICE),
            window.then(|| keyring.keys(mixengine_core::window::KEYRING_SERVICE)),
        )
    })
    .await
    .unwrap_or_else(|error| {
        let failed = || {
            Err(mixengine_platform::Error::Secret {
                action: "list",
                service: mixengine_platform::KEYRING_SERVICE.to_owned(),
                key: "*".to_owned(),
                source: error.to_string().into(),
            })
        };
        (failed(), None)
    });

    Found {
        home,
        daemon,
        window,
    }
}

/// `keys` that belong to `home`.
pub(crate) fn ours(home: &str, keys: &[String]) -> Vec<String> {
    let prefix = format!("{home}/");
    keys.iter()
        .filter(|key| key.starts_with(&prefix))
        .cloned()
        .collect()
}

/// **10d.** This home's passwords and the window's — T182d.
///
/// They follow the home, as the window's data does. A store with nothing of ours has no row. A
/// machine with no store at all is the same answer, because it has nothing stored in one, and a
/// row saying "failed" there would fail every uninstall on a headless Linux. A store that is there
/// and refused is `Failed`, never absent.
pub(crate) fn credential_rows(found: &Found, keep_home: bool) -> Vec<Residue> {
    let row = |id, what: String, location: &str| Residue {
        id,
        what,
        location: location.to_owned(),
        outcome: match keep_home {
            true => Removal::Kept {
                because: "you asked for this home's data to be left where it is, and the \
                          passwords it needs stay with it"
                    .to_owned(),
            },
            false => Removal::Planned {
                how: "forget each one, wherever this home keeps them".to_owned(),
            },
        },
    };
    let failed = |id, what: &str, location: &str, because: String| Residue {
        id,
        what: what.to_owned(),
        location: location.to_owned(),
        outcome: Removal::Failed { because },
    };

    let mut rows = Vec::new();
    let daemon_location = format!(
        "{} · {}/…",
        mixengine_platform::KEYRING_SERVICE,
        found.home.as_deref().unwrap_or("this home")
    );

    match (&found.daemon, found.home.as_deref()) {
        (Err(mixengine_platform::Error::UnsupportedPlatform { .. }), _) => {}
        (Err(error), _) => rows.push(failed(
            ResidueId::Credentials,
            "this home's passwords",
            &daemon_location,
            format!("the credential store could not be listed: {error}"),
        )),
        (Ok(_), None) => rows.push(failed(
            ResidueId::Credentials,
            "this home's passwords",
            &daemon_location,
            "this home's id could not be read, so its passwords cannot be told from another \
             home's"
                .to_owned(),
        )),
        (Ok(keys), Some(home)) => {
            let count = ours(home, keys).len();
            if count > 0 {
                let noun = if count == 1 { "password" } else { "passwords" };
                rows.push(row(
                    ResidueId::Credentials,
                    format!("{count} {noun} this home's services and databases use"),
                    &daemon_location,
                ));
            }
        }
    }

    match &found.window {
        None | Some(Err(mixengine_platform::Error::UnsupportedPlatform { .. })) => {}
        Some(Err(error)) => rows.push(failed(
            ResidueId::WindowCredentials,
            "MixLab's saved passwords and sync sign-in",
            mixengine_core::window::KEYRING_SERVICE,
            format!("the credential store could not be listed: {error}"),
        )),
        Some(Ok(keys)) if keys.is_empty() => {}
        Some(Ok(_)) => rows.push(row(
            ResidueId::WindowCredentials,
            "MixLab's saved passwords and sync sign-in".to_owned(),
            mixengine_core::window::KEYRING_SERVICE,
        )),
    }

    rows
}
