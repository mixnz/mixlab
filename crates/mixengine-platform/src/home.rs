//! The home a development build suggests for itself, and whether it may take it.
//!
//! Roadmap task T166, ADR 0040. `.cargo/config.toml` sets [`DEV_HOME_VAR`] to the checkout's own
//! `.mixengine-home` for everything cargo runs — the window's daemon, a terminal's `cargo run`, the
//! window itself — so that they are one daemon. It used to set `MIXENGINE_HOME`, which is an
//! override: a checkout on an external disk then put `run/` where macOS will not let the elevated
//! helper read its own request, and every Allow on the prompt ended in *no report*.
//!
//! So the checkout **suggests**, and the three places that resolve a home — `mix`, `mixengined`
//! and the window — weigh the suggestion here, in one function, after `--home` and
//! `MIXENGINE_HOME` and before [`HomeDirs::default_home`](crate::HomeDirs::default_home). One
//! function is what keeps them agreeing, which is the property the suggestion exists for.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::Host;

/// The variable a development checkout names its suggested home in.
pub const DEV_HOME_VAR: &str = "MIXENGINE_DEV_HOME";

/// Where this user's home goes when nothing says otherwise — the same answer as
/// [`HomeDirs::default_home`](crate::HomeDirs::default_home), with no [`Host`] built to ask it.
///
/// **For the shim, which runs in front of every `php` a person types.** A `Host` is a trait object
/// holding every capability, and its vtable keeps every implementation and every DLL they import:
/// on Windows a dozen, loaded and unloaded around each run, for a program that only needs this one
/// path. The trait's implementations call this, so there is one answer however it is asked for.
///
/// # Errors
///
/// [`crate::Error::NoHomeDirectory`] when the OS cannot say where user data belongs.
pub fn default_home() -> crate::Result<PathBuf> {
    crate::sys::home::default_home()
}

/// The checkout's suggested home, if this build may take it — read from the environment.
///
/// `None` sends the caller on to the platform default, which for a development build is ADR 0024's
/// `MixEngine-dev`.
#[must_use]
pub fn development_home(host: &dyn Host) -> Option<PathBuf> {
    development_home_from(host, std::env::var_os(DEV_HOME_VAR).as_deref())
}

/// [`development_home`] over a value the caller read, so that a test does not depend on the
/// environment cargo gives it — which, in this repository, always carries the variable.
///
/// `None` in a release build, whatever the value: a shipped binary has no checkout. `None` for an
/// absent or empty value, and for a directory an elevated process could not read — that one with a
/// `warn`, because a developer whose home just moved should be able to find out why.
#[must_use]
pub fn development_home_from(host: &dyn Host, value: Option<&OsStr>) -> Option<PathBuf> {
    decide(host, value, crate::RELEASE)
}

fn decide(host: &dyn Host, value: Option<&OsStr>, release: bool) -> Option<PathBuf> {
    if release {
        return None;
    }

    let value = value.filter(|value| !value.is_empty())?;
    let path = std::path::absolute(value).ok()?;

    if !host.home_dirs().elevated_can_read(&path) {
        if first_time(&PASSED_OVER_SAID) {
            tracing::warn!(
                passed_over = %path.display(),
                "this checkout's home is on a volume the elevation helper cannot read; using the \
                 default home instead"
            );
        }
        return None;
    }

    Some(path)
}

/// Whether this process has already said it passed the suggestion over.
///
/// **Once a process, because the window asks on every connection** — each RPC call, and each
/// events, logs and metrics stream, resolves the home afresh, and a dev window opening one screen
/// printed the same sentence nineteen times in a second. The answer cannot change in between: it is
/// a function of an environment variable and a path, and neither moves under a running process.
static PASSED_OVER_SAID: AtomicBool = AtomicBool::new(false);

/// `true` the first time it is asked about `flag`, and never again.
fn first_time(flag: &AtomicBool) -> bool {
    !flag.swap(true, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::mock;

    fn suggested() -> &'static OsStr {
        OsStr::new("/checkout/.mixengine-home")
    }

    #[test]
    fn a_readable_suggestion_is_taken() {
        let host = mock::Host::with_home("/default");

        assert_eq!(
            decide(&host, Some(suggested()), false),
            // Made absolute the way `decide` does, which on Windows puts a drive in front.
            Some(std::path::absolute(suggested()).unwrap())
        );
    }

    #[test]
    fn no_suggestion_or_an_empty_one_is_none() {
        let host = mock::Host::with_home("/default");

        assert_eq!(decide(&host, None, false), None);
        assert_eq!(decide(&host, Some(OsStr::new("")), false), None);
    }

    /// The incident T166 comes from: a checkout on a volume TCC gates.
    #[test]
    fn a_suggestion_the_helper_could_not_read_is_passed_over() {
        let host = mock::Host::with_home("/default")
            .elevated_cannot_read(std::path::absolute("/checkout").unwrap());

        assert_eq!(decide(&host, Some(suggested()), false), None);
    }

    /// Asked twice, the second answer is the first — only the warning is not repeated.
    #[test]
    fn a_passed_over_suggestion_stays_passed_over() {
        let host = mock::Host::with_home("/default")
            .elevated_cannot_read(std::path::absolute("/checkout").unwrap());

        assert_eq!(decide(&host, Some(suggested()), false), None);
        assert_eq!(decide(&host, Some(suggested()), false), None);
    }

    #[test]
    fn the_warning_is_said_once() {
        let flag = AtomicBool::new(false);

        assert!(first_time(&flag));
        assert!(!first_time(&flag));
        assert!(!first_time(&flag));
    }

    #[test]
    fn a_release_build_has_no_checkout() {
        let host = mock::Host::with_home("/default");

        assert_eq!(decide(&host, Some(suggested()), true), None);
    }
}
