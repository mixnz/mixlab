//! What MixEngine knows about a JDK that the index does not say — roadmap task **T27e**.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use mixengine_platform::process::Ran;

use crate::{Error, InstallError, Result};

/// The password every JDK's `cacerts` ships with — a published default, not a secret (the design's
/// D8).
const STOREPASS: &str = "changeit";

/// How long one `keytool` may take. A JVM start is a few hundred milliseconds; a minute is a hang.
const PATIENCE: Duration = Duration::from_secs(60);

/// The variables a JVM reads before its own arguments, removed from every JVM the daemon starts
/// for itself — the design's D3. A malformed `_JAVA_OPTIONS` in the daemon's own environment would
/// otherwise fail every JDK's install for a reason that has nothing to do with the JDK.
pub const UNSET: &[&str] = &[
    "JAVA_TOOL_OPTIONS",
    "_JAVA_OPTIONS",
    "JDK_JAVA_OPTIONS",
    "CLASSPATH",
    "JAVA_HOME",
];

/// `JAVA_HOME` for the JDK whose `java` is `java` — two directories up, which is `Contents/Home` on
/// macOS and the archive root elsewhere (the design's D5).
///
/// [`None`] for a path with no directory two levels above it: an empty path is not a home, and
/// naming one would point `JAVA_HOME` at whatever directory a program happened to start in.
#[must_use]
pub fn home(java: &Path) -> Option<PathBuf> {
    java.parent()?
        .parent()
        .filter(|home| !home.as_os_str().is_empty())
        .map(Path::to_path_buf)
}

/// The alias this home's authority is held under: its key-id, which is T49a's D5 applied to one
/// more store — a removal names an authority and never a certificate.
#[must_use]
pub fn alias(key_id: &str) -> String {
    format!("mixengine-{key_id}")
}

/// Whether the standard output of `keytool -exportcert -rfc` is exactly the certificate `der`.
///
/// The PEM block is found by its header rather than assumed to start the stream: a JVM prints
/// `Picked up …` lines of its own. `-list` is never parsed, because its words are localised.
#[must_use]
pub fn exported_is(stdout: &str, der: &[u8]) -> bool {
    stdout
        .find("-----BEGIN CERTIFICATE-----")
        .and_then(|start| crate::certs::ca::der(&stdout[start..]))
        .is_some_and(|found| found == der)
}

/// Whether this JDK's `cacerts` holds the authority `key_id` names, as exactly `der`.
///
/// # Errors
///
/// [`InstallError::KeytoolRefused`] when `keytool` cannot be started at all or does not finish. An
/// alias that is not there is `Ok(false)`, which is what `keytool` says by exiting non-zero.
pub async fn holds(keytool: &Path, key_id: &str, der: &[u8]) -> Result<bool> {
    let ran = run(keytool, &exporting(key_id)).await?;

    Ok(ran.succeeded() && exported_is(ran.output(), der))
}

/// Put the authority in `certificate` into this JDK's `cacerts`, under `key_id`'s alias.
///
/// # Errors
///
/// [`InstallError::KeytoolRefused`] naming `keytool`'s last line — a `cacerts` whose password is
/// not the published default is the one this build expects to meet.
pub async fn hold(keytool: &Path, key_id: &str, certificate: &Path) -> Result<()> {
    let args = [
        OsString::from("-importcert"),
        OsString::from("-noprompt"),
        OsString::from("-cacerts"),
        OsString::from("-storepass"),
        OsString::from(STOREPASS),
        OsString::from("-alias"),
        OsString::from(alias(key_id)),
        OsString::from("-file"),
        certificate.as_os_str().to_owned(),
    ];

    succeeded(keytool, &run(keytool, &args).await?)
}

/// Let the authority `key_id` names go from this JDK's `cacerts`.
///
/// An alias that is not there is already let go, which is what makes this idempotent for a home
/// whose authority was never written into a JDK installed since.
///
/// # Errors
///
/// [`InstallError::KeytoolRefused`] when the alias is there and `keytool` would not delete it.
pub async fn release(keytool: &Path, key_id: &str) -> Result<()> {
    if !run(keytool, &exporting(key_id)).await?.succeeded() {
        return Ok(());
    }

    let args = [
        OsString::from("-delete"),
        OsString::from("-cacerts"),
        OsString::from("-storepass"),
        OsString::from(STOREPASS),
        OsString::from("-alias"),
        OsString::from(alias(key_id)),
    ];

    succeeded(keytool, &run(keytool, &args).await?)
}

/// The one invocation that both asks and proves: export the alias, as PEM.
fn exporting(key_id: &str) -> [OsString; 7] {
    [
        OsString::from("-exportcert"),
        OsString::from("-rfc"),
        OsString::from("-cacerts"),
        OsString::from("-storepass"),
        OsString::from(STOREPASS),
        OsString::from("-alias"),
        OsString::from(alias(key_id)),
    ]
}

/// One `keytool`, through the platform's one-shot: no console window, a deadline, captured output,
/// and an environment built from a short allow-list — so [`UNSET`] cannot reach it in the first
/// place, whatever the daemon was started with.
async fn run(keytool: &Path, args: &[OsString]) -> Result<Ran> {
    let directory = keytool.parent().unwrap_or(keytool);

    mixengine_platform::process::run_once(keytool, args, directory, &BTreeMap::new(), PATIENCE)
        .await
        .map_err(|error| {
            Error::Install(InstallError::KeytoolRefused {
                program: keytool.to_path_buf(),
                detail: error.to_string(),
            })
        })
}

/// A run that did what it was asked, or the sentence saying it did not.
fn succeeded(keytool: &Path, ran: &Ran) -> Result<()> {
    if ran.succeeded() {
        return Ok(());
    }

    Err(Error::Install(InstallError::KeytoolRefused {
        program: keytool.to_path_buf(),
        detail: match ran.timed_out() {
            true => format!("it did not finish within {} seconds", PATIENCE.as_secs()),
            false => ran
                .complaint()
                .unwrap_or("it failed and said nothing")
                .to_owned(),
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_is_two_directories_above_java() {
        let root = Path::new("runtimes").join("java").join("21.0.12.1");

        assert_eq!(home(&root.join("bin").join("java")), Some(root.clone()));

        let bundle = root.join("Contents").join("Home");
        assert_eq!(home(&bundle.join("bin").join("java")), Some(bundle));
    }

    #[test]
    fn a_path_with_nowhere_above_it_has_no_home() {
        assert_eq!(home(Path::new("java")), None);
        assert_eq!(home(&Path::new("bin").join("java")), None);
    }

    #[test]
    fn the_alias_names_the_authority_by_key_id() {
        assert_eq!(alias("0a1b2c3d4e5f6071"), "mixengine-0a1b2c3d4e5f6071");
    }

    #[test]
    fn an_export_is_this_authority_only_when_its_bytes_are() {
        let der = [0x30, 0x03, 0x02, 0x01, 0x05];
        let pem = pem::encode(&pem::Pem::new("CERTIFICATE", der.to_vec()));

        assert!(exported_is(&pem, &der));
        assert!(
            exported_is(&format!("Picked up _JAVA_OPTIONS: -Xmx1g\n{pem}"), &der),
            "a JVM's own line before the block is not part of the certificate"
        );
        assert!(!exported_is(&pem, &[0x30, 0x00]));
        assert!(!exported_is(
            "keytool error: java.lang.Exception: Alias <mixengine-x> does not exist",
            &der
        ));
    }
}
