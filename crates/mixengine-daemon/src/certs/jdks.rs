//! Every installed JDK told about this home's authority, inside its own `cacerts` —
//! [ADR 0039](../../../../docs/decisions/0039-a-jdk-is-told-about-the-authority-inside-its-own-cacerts.md).
//!
//! **The browsers' shape, call for call**: a store the user owns, written without a prompt, at
//! start, on repair, on a rotation and on every removal — plus once after a JDK is installed, since
//! that is the one moment a store nobody has written to appears. Nothing here fails what called it;
//! what happened comes back in a [`JdkChange`], which the caller logs.
//!
//! A JDK reads neither the operating system's store nor the bundle `certs::bundle` writes, and no
//! environment variable *adds* an authority to the one it does read. That is the whole of why this
//! module exists rather than a row in the table `mixengine-shim`'s `trusting` keeps.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use mixengine_core::Store;
use mixengine_core::runtimes::{self, java};
use mixengine_proto::{CaState, RuntimeKind};

/// What asking every JDK did.
#[derive(Debug, Default)]
pub(crate) struct JdkChange {
    /// `java 21.0.12.1` for each JDK that was written to.
    pub(crate) written: Vec<String>,

    /// `java 21.0.12.1: …` for each that would not answer or would not write.
    pub(crate) refused: Vec<String>,
}

impl JdkChange {
    /// One line for what was written, one warning per refusal. `when` says which of the moments the
    /// module names this was, because a refusal at start and a refusal after an install are read
    /// differently.
    pub(crate) fn log(&self, when: &str) {
        if !self.written.is_empty() {
            tracing::info!(
                jdks = ?self.written,
                "{when}: this home's authority was written into these JDKs"
            );
        }

        for refused in &self.refused {
            tracing::warn!(%refused, "{when}: a JDK's certificate store was not updated");
        }
    }
}

/// **One writer at a time.**
///
/// A `static` rather than a field: `Certificates` is cloned and constructed in half a dozen places,
/// so a lock of its own would be half a dozen locks, and a start's reconcile racing a rotation would
/// be two JVMs rewriting one file.
static WRITES: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Put this home's authority into every JDK that does not already hold it.
///
/// Nothing to do for a home with no usable authority, or none at all: there is nothing a JDK could
/// be told, and writing a certificate a rotation has to replace would be worse than writing none.
pub(crate) async fn hold(store: &Store, certs: &Path) -> JdkChange {
    let mut change = JdkChange::default();

    let Some((key_id, der)) = authority(certs) else {
        return change;
    };
    let certificate = mixengine_core::certs::ca::certificate_path(certs);

    let _writing = WRITES.lock().await;
    for (name, keytool) in keytools(store, &mut change.refused).await {
        match java::holds(&keytool, &key_id, &der).await {
            Ok(true) => {}
            Ok(false) => match java::hold(&keytool, &key_id, &certificate).await {
                Ok(()) => change.written.push(name),
                Err(error) => change.refused.push(format!("{name}: {error}")),
            },
            Err(error) => change.refused.push(format!("{name}: {error}")),
        }
    }

    change
}

/// Let the authority `key_id` names go from every installed JDK.
///
/// **Named by key-id and never by certificate**, which is [`java::alias`]'s own guarantee: a
/// rotation's old authority and its new one are different aliases, so letting one go never touches
/// the other.
pub(crate) async fn release(store: &Store, key_id: &str) -> JdkChange {
    let mut change = JdkChange::default();

    let _writing = WRITES.lock().await;
    for (name, keytool) in keytools(store, &mut change.refused).await {
        match java::release(&keytool, key_id).await {
            Ok(()) => change.written.push(name),
            Err(error) => change.refused.push(format!("{name}: {error}")),
        }
    }

    change
}

/// Which JDKs do not hold this home's authority — `mix doctor`'s question, which writes nothing.
///
/// Empty for a home with no usable authority: there is nothing for a JDK to be missing. A JDK whose
/// `keytool` refused is listed with what it said, because "could not be asked" is not "holds it".
pub(crate) async fn lacking(store: &Store, certs: &Path) -> Vec<String> {
    let mut lacking = Vec::new();

    let Some((key_id, der)) = authority(certs) else {
        return lacking;
    };

    for (name, keytool) in keytools(store, &mut lacking).await {
        match java::holds(&keytool, &key_id, &der).await {
            Ok(true) => {}
            Ok(false) => lacking.push(name),
            Err(error) => lacking.push(format!("{name} ({error})")),
        }
    }

    lacking
}

/// This home's authority as the two things every call here needs, or [`None`] when it has none a
/// JDK could be told about.
fn authority(certs: &Path) -> Option<(String, Vec<u8>)> {
    let CaState::Present { ca } = mixengine_core::certs::ca::read(certs, SystemTime::now()) else {
        return None;
    };

    mixengine_core::certs::ca::der(&ca.certificate_pem).map(|der| (ca.key_id, der))
}

/// Each installed JDK's name and its own `keytool`, with anything unreadable pushed onto `refused`.
async fn keytools(store: &Store, refused: &mut Vec<String>) -> Vec<(String, PathBuf)> {
    let installed = match runtimes::records(store, Some(RuntimeKind::Java)).await {
        Ok(installed) => installed,
        Err(error) => {
            refused.push(format!(
                "this home's installed JDKs could not be read: {error}"
            ));
            return Vec::new();
        }
    };

    let mut found = Vec::with_capacity(installed.len());
    for runtime in installed {
        let name = format!("java {}", runtime.version.as_str());

        match runtimes::program(store, RuntimeKind::Java, &runtime.version, "keytool").await {
            Ok(keytool) => found.push((name, keytool)),
            Err(error) => refused.push(format!("{name}: {error}")),
        }
    }

    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A home with no JDK asks nothing and reports nothing**, whatever its authority — the
    /// ordinary machine, where this costs one query and no process.
    #[tokio::test]
    async fn a_home_with_no_jdk_has_nothing_to_hold_or_lack() {
        let home = tempfile::tempdir().expect("a temporary home");
        let store = Store::open(&home.path().join("mixengine.db"))
            .await
            .expect("a store");
        let certs = home.path().join("certs");
        std::fs::create_dir_all(&certs).expect("a certificates directory");
        mixengine_core::certs::ca::ensure(&certs, SystemTime::now()).expect("an authority");

        let change = hold(&store, &certs).await;

        assert!(change.written.is_empty(), "{change:?}");
        assert!(change.refused.is_empty(), "{change:?}");
        assert!(lacking(&store, &certs).await.is_empty());
    }

    /// **A home with no authority asks the JDKs for nothing**, which is the rule
    /// `install_in_browsers` follows for the same reason: writing something a rotation has to
    /// replace is worse than writing nothing.
    #[tokio::test]
    async fn a_home_with_no_authority_writes_into_no_jdk() {
        let home = tempfile::tempdir().expect("a temporary home");
        let store = Store::open(&home.path().join("mixengine.db"))
            .await
            .expect("a store");

        let change = hold(&store, &home.path().join("certs")).await;

        assert!(
            change.written.is_empty() && change.refused.is_empty(),
            "{change:?}"
        );
    }
}
