//! Where this machine's MixEngine home is, and where its daemon listens.
//!
//! Both answers are `mixengine-platform`'s, the same way they are `mix`'s
//! (`crates/mixengine-cli/src/home.rs`): `MIXENGINE_HOME` wins, then a development checkout's
//! suggested home when `mixengine_platform::home` lets it stand (T166), then the platform, and
//! the result is made absolute and spelled in full rather than canonicalised, and the endpoint is
//! computed from `<home>/run`. Nothing here reads `config.toml`: `[daemon] ipc_path` was parsed by
//! the daemon's own config and used by nothing, so honouring it — as this file did until phase 11,
//! T102 — was dialling somewhere no daemon listens. The key has since been retired from the
//! template; a file that still carries it is read and ignored.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use mixengine_platform::home::{development_home_from, DEV_HOME_VAR};
use mixengine_platform::ipc::Endpoint;
use mixengine_platform::Host;

use crate::error::AppError;

/// The daemon's runtime directory under the home — the one directory `[paths]` cannot move.
const RUN: &str = "run";

/// The MixEngine home this window talks about.
pub fn home() -> Result<PathBuf, AppError> {
    home_from(
        std::env::var_os("MIXENGINE_HOME").as_deref(),
        std::env::var_os(DEV_HOME_VAR).as_deref(),
        mixengine_platform::host().as_ref(),
    )
}

/// [`home`] over values the caller read, so a test needs neither the environment nor this machine.
fn home_from(
    chosen: Option<&OsStr>,
    suggested: Option<&OsStr>,
    host: &dyn Host,
) -> Result<PathBuf, AppError> {
    let root = match chosen.filter(|value| !value.is_empty()) {
        Some(value) => PathBuf::from(value),
        None => match development_home_from(host, suggested) {
            Some(path) => path,
            None => host
                .home_dirs()
                .default_home()
                .map_err(|error| err!("error.mixengineNoHome", message = error))?,
        },
    };
    let absolute = std::path::absolute(&root)
        .map_err(|error| err!("error.mixengineNoHome", message = error))?;
    Ok(mixengine_platform::paths::in_full(&absolute))
}

/// `<home>/run` — where the endpoint lives, and what a Windows pipe name is fingerprinted on.
pub fn run_dir(home: &Path) -> PathBuf {
    home.join(RUN)
}

/// The endpoint the daemon for this home listens on, whether or not one is running.
pub fn endpoint() -> Result<Endpoint, AppError> {
    let home = home()?;
    let run = run_dir(&home);
    Endpoint::in_run_dir(&run).map_err(|error| {
        err!(
            "error.mixengineUnreachable",
            endpoint = run.display().to_string(),
            message = error
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_dir_hangs_off_the_home() {
        assert_eq!(run_dir(Path::new("/h")), PathBuf::from("/h").join("run"));
    }

    /// T166: `MIXENGINE_HOME` beats a checkout's suggestion, which beats the platform default —
    /// the same order `mix` and `mixengined` resolve in.
    #[test]
    fn a_checkout_suggestion_sits_between_the_override_and_the_default() {
        let host = mixengine_platform::mock::Host::with_home(std::env::temp_dir().join("default"));
        let suggested = std::env::temp_dir().join("checkout-home");
        let chosen = std::env::temp_dir().join("chosen");

        let home = home_from(None, Some(suggested.as_os_str()), &host).expect("a home");
        assert!(home.ends_with("checkout-home"), "{home:?}");

        let home = home_from(Some(chosen.as_os_str()), Some(suggested.as_os_str()), &host)
            .expect("a home");
        assert!(home.ends_with("chosen"), "{home:?}");

        let home = home_from(None, None, &host).expect("a home");
        assert!(home.ends_with("default"), "{home:?}");
    }

    /// `MIXENGINE_HOME` wins over the default — the whole reason a sandbox daemon is reachable.
    /// Set and restored inside one test: `std::env` is process-global and cargo runs tests on
    /// threads, so this is the only test in the crate that writes it.
    #[test]
    fn the_environment_overrides_the_default_home() {
        let before = std::env::var_os("MIXENGINE_HOME");
        let sandbox = std::env::temp_dir().join("mixengine-endpoint-test");
        std::env::set_var("MIXENGINE_HOME", &sandbox);
        let home = home();
        match before {
            Some(value) => std::env::set_var("MIXENGINE_HOME", value),
            None => std::env::remove_var("MIXENGINE_HOME"),
        }
        let home = home.expect("a home");
        assert!(home.ends_with("mixengine-endpoint-test"), "{home:?}");
        assert!(home.is_absolute(), "{home:?}");
    }
}
