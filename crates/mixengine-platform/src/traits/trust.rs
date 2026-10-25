//! Whether this machine trusts MixEngine's own certificate authority.

use mixengine_proto::privileged::{TrustPlan, TrustTarget};

use crate::Result;

/// How this machine can be made to trust a certificate authority of our own.
///
/// **Windows and macOS are constants; Linux is a question asked at runtime**, on
/// [`ResolverMethod`](crate::ResolverMethod)'s precedent and for its reason. A Debian-family
/// machine takes an anchor in one directory and a Red Hat one takes it in another, each refreshed
/// by its own command, and a machine with neither has no system store MixEngine knows how to write.
/// That is why [`TrustStore::method`] returns a `Result` where
/// [`PortAccessMethod`](crate::PortAccessMethod)'s equivalent is a constant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustStoreMethod {
    /// Windows: `Root`, under `LocalMachine`.
    SystemRoot,

    /// macOS: `/Library/Keychains/System.keychain`.
    SystemKeychain,

    /// Linux, Debian family: `/usr/local/share/ca-certificates`.
    CaCertificates,

    /// Linux, Red Hat family: `/etc/pki/ca-trust/source/anchors`.
    CaTrustAnchors,

    /// This machine has no system trust store MixEngine knows how to write.
    ///
    /// **A valid answer, not an error**, exactly as [`ResolverMethod::None`](crate::ResolverMethod)
    /// is. Sites keep working over HTTP, `cert.ca_status` says why in words, and nothing fails —
    /// which is what `docs/features/tls.md` asks for, with the correction that what is
    /// unsupported is this machine's configuration rather than the platform.
    None,
}

/// What this machine trusts today, and what it would take to trust ours.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustState {
    /// Which mechanism this machine has.
    pub method: TrustStoreMethod,

    /// Whether the certificate that was asked about is already there.
    ///
    /// **Compared as exact DER bytes.** A subject match would claim another home's authority as
    /// this one's, and a store's own SHA-1 property is a different value from the SHA-256
    /// `cert.ca_status` reports, so carrying two hashes for one identity is how they come apart.
    pub installed: bool,

    /// Why not, in words, for `mix doctor`'s `CaNotTrusted` and for the screen.
    pub missing: Option<String>,
}

impl TrustState {
    /// What would have to be applied for this machine to trust `der`, or [`None`] when nothing
    /// would.
    ///
    /// **Whole state, like every operation beside it**: a machine that already holds exactly this
    /// certificate plans nothing, so no prompt is spent on a row whose only possible outcome is
    /// `AlreadyDone` — the hosts file's rule that the disk is read before a prompt is spent.
    #[must_use]
    pub fn plan(&self, der: &[u8]) -> Option<TrustPlan> {
        if self.installed {
            return None;
        }

        match self.method {
            TrustStoreMethod::None => None,
            TrustStoreMethod::SystemRoot => Some(TrustPlan::SystemRoot { der: der.to_vec() }),
            TrustStoreMethod::SystemKeychain => {
                Some(TrustPlan::SystemKeychain { der: der.to_vec() })
            }
            TrustStoreMethod::CaCertificates => {
                Some(TrustPlan::CaCertificates { der: der.to_vec() })
            }
            TrustStoreMethod::CaTrustAnchors => {
                Some(TrustPlan::CaTrustAnchors { der: der.to_vec() })
            }
        }
    }

    /// What would have to be removed to undo [`plan`](Self::plan), or [`None`] on a machine with no
    /// store.
    ///
    /// **It names an authority and never a certificate**, because removal is the direction that can
    /// do damage. `key_id` is the eight hex characters `mixengine_core::certs::ca` puts in the
    /// common name; there is no fingerprint here, because a removal that could name one could name
    /// the root that validates this machine's own updates.
    ///
    /// The daemon builds one when the authority is rotated, and when MixEngine is uninstalled.
    #[must_use]
    pub fn target(&self, key_id: &str) -> Option<TrustTarget> {
        let key_id = key_id.to_owned();

        match self.method {
            TrustStoreMethod::None => None,
            TrustStoreMethod::SystemRoot => Some(TrustTarget::SystemRoot { key_id }),
            TrustStoreMethod::SystemKeychain => Some(TrustTarget::SystemKeychain { key_id }),
            TrustStoreMethod::CaCertificates => Some(TrustTarget::CaCertificates { key_id }),
            TrustStoreMethod::CaTrustAnchors => Some(TrustTarget::CaTrustAnchors { key_id }),
        }
    }
}

/// Whether this machine trusts MixEngine's own certificate authority.
///
/// **Reads only, and never prompts.** The write needs a token this process does not have; it is
/// `PrivilegedOp::TrustCaInstall`, applied by `mixengine-elevate` — carrying a
/// [`TrustPlan`] this state built. Reading needs no privilege on any of the three systems, which is what makes
/// it affordable to ask on every daemon start — and `tests/trust.rs` is what measures that rather
/// than this sentence.
pub trait TrustStore: std::fmt::Debug + Send + Sync {
    /// Which mechanism this machine has, or [`TrustStoreMethod::None`].
    ///
    /// # Errors
    ///
    /// [`Error::Io`](crate::Error::Io) when something this reads cannot be read. **Every caller
    /// treats an error as "no answer" and carries on**: this is asked at start-up, and a probe that
    /// failed must not become the thing that stops a daemon.
    fn method(&self) -> Result<TrustStoreMethod>;

    /// Whether this machine already holds exactly `der`, and why not.
    ///
    /// **This answers "is the certificate in the store", not "does a browser trust it"**, and the
    /// difference is the one [`ResolverConfig::probe`](crate::ResolverConfig::probe) draws for its
    /// own capability: Firefox and Chrome on Linux read NSS and not this store at all, and a
    /// browser already running may not re-read a store it has cached. The honest end-to-end check
    /// is a live TLS handshake, which is `mix cert status`' job.
    ///
    /// # Errors
    ///
    /// As [`method`](Self::method).
    fn probe(&self, der: &[u8]) -> Result<TrustState>;

    /// Every root this machine trusts, as DER.
    ///
    /// **The other direction through the same store.** [`probe`](Self::probe) asks whether one
    /// certificate is in it; this reads the lot, so that MixEngine can write a bundle holding them
    /// *and* its own authority. That bundle is what a runtime whose trust mechanism **replaces**
    /// rather than adds — Python's and Ruby's `SSL_CERT_FILE`, PHP's `openssl.cafile` — can be
    /// pointed at without losing the public internet.
    ///
    /// **Defaulted here rather than written three times**, because the per-OS work is
    /// `rustls-native-certs`': the Windows `ROOT` store, macOS's trust settings, Linux's
    /// `ca-certificates` file. What `docs/architecture/platform-abstraction.md` asks of this
    /// crate is that the `#[cfg]` live here and nowhere above it, and a dependency that carries it
    /// satisfies that as squarely as a `match` would.
    ///
    /// Unprivileged on all three systems, for [`probe`](Self::probe)'s reason.
    ///
    /// # Errors
    ///
    /// [`Error::Io`](crate::Error::Io) when the store cannot be enumerated. **A caller must treat a
    /// short answer as a failure too**: `crate::certs` will not write a bundle from fewer roots
    /// than a real machine holds, because a `SSL_CERT_FILE` naming a handful of certificates breaks
    /// every public handshake on the machine at once.
    fn roots(&self) -> Result<Vec<Vec<u8>>> {
        let loaded = rustls_native_certs::load_native_certs();

        // Partial answers are the ordinary case rather than an error: a Linux machine whose
        // `ca-certificates` holds one unparseable file still trusts the other hundred and forty.
        // What makes the difference is whether anything came back at all.
        for error in &loaded.errors {
            tracing::debug!(%error, "one of this machine's trust store entries could not be read");
        }

        if loaded.certs.is_empty() {
            let reason = match loaded.errors.first() {
                Some(error) => error.to_string(),
                None => "this machine's trust store holds nothing this build could read".to_owned(),
            };

            return Err(crate::Error::Io {
                action: "read this machine's trusted roots",
                path: std::path::PathBuf::from("(the system trust store)"),
                source: std::io::Error::other(reason),
            });
        }

        Ok(loaded
            .certs
            .into_iter()
            .map(|certificate| certificate.as_ref().to_vec())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(method: TrustStoreMethod, installed: bool) -> TrustState {
        TrustState {
            method,
            installed,
            missing: None,
        }
    }

    /// A machine that already trusts it asks for nothing, so no prompt is spent on a row whose only
    /// possible outcome is `AlreadyDone`.
    #[test]
    fn a_machine_that_already_trusts_it_needs_no_plan() {
        assert_eq!(
            state(TrustStoreMethod::SystemRoot, true).plan(&[1, 2, 3]),
            None
        );
    }

    #[test]
    fn a_machine_that_does_not_is_asked_for_this_certificate() {
        assert_eq!(
            state(TrustStoreMethod::SystemRoot, false).plan(&[1, 2, 3]),
            Some(TrustPlan::SystemRoot { der: vec![1, 2, 3] })
        );
    }

    /// Each mechanism asks for its own, and never for another system's.
    #[test]
    fn every_mechanism_plans_its_own_store() {
        for (method, expected) in [
            (
                TrustStoreMethod::SystemKeychain,
                TrustPlan::SystemKeychain { der: vec![7] },
            ),
            (
                TrustStoreMethod::CaCertificates,
                TrustPlan::CaCertificates { der: vec![7] },
            ),
            (
                TrustStoreMethod::CaTrustAnchors,
                TrustPlan::CaTrustAnchors { der: vec![7] },
            ),
        ] {
            assert_eq!(state(method, false).plan(&[7]), Some(expected));
        }
    }

    /// A machine with no store is a supported machine, not a failure.
    #[test]
    fn a_machine_with_no_store_asks_for_nothing_in_either_direction() {
        let state = state(TrustStoreMethod::None, false);

        assert_eq!(state.plan(&[1]), None);
        assert_eq!(state.target("deadbeef"), None);
    }

    /// At the layer that builds the value, an authority is named and a certificate is not.
    #[test]
    fn a_target_names_the_authority_and_the_mechanism() {
        assert_eq!(
            state(TrustStoreMethod::CaCertificates, true).target("deadbeef"),
            Some(TrustTarget::CaCertificates {
                key_id: "deadbeef".to_owned()
            })
        );
    }

    /// A machine that already trusts it can still be asked to stop, which is what makes the two
    /// directions independent rather than a toggle.
    #[test]
    fn a_target_does_not_depend_on_whether_it_is_installed() {
        assert_eq!(
            state(TrustStoreMethod::SystemRoot, true).target("deadbeef"),
            state(TrustStoreMethod::SystemRoot, false).target("deadbeef"),
        );
    }
}
