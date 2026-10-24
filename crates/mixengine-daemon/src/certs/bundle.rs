//! Writing `etc/ca/bundle.pem`, and when.
//!
//! [`mixengine_core::generate::ca`] composes the file; this is the two lines that fetch its inputs
//! and the one rule about failure. The machine's roots come from
//! [`TrustStore::roots`](mixengine_platform::TrustStore::roots) — the OS store on all three systems,
//! read unprivileged — and the authority comes off disk.
//!
//! **Nothing here can fail a daemon start.** A machine whose store cannot be enumerated is one
//! whose runtimes are told nothing and go on working exactly as they did before this task existed;
//! a daemon that refused to start over it would be strictly worse. `mix doctor` is where the gap is
//! reported, and its repair is this function.

use mixengine_core::{Paths, generate::ca};
use mixengine_platform::Host;

/// Bring the bundle up to date with this machine's trust store and this home's authority.
///
/// Called at every daemon start, and again by `cert.ca_rotate`: a rotation changes the fingerprint
/// in the header, so the file differs and is installed — the same mechanism a renewed leaf uses to
/// make a front end re-read its configuration.
/// Answers whether the file on disk moved, which is what tells a caller the generated ini sets
/// naming it have to be written again.
pub(crate) fn render(paths: &Paths, host: &dyn Host) -> bool {
    render_into(paths.etc(), paths.certs(), host)
}

/// The same, for a caller that kept the two directories rather than the whole [`Paths`].
pub(crate) fn render_into(etc: &std::path::Path, certs: &std::path::Path, host: &dyn Host) -> bool {
    let roots = match host.trust_store().roots() {
        Ok(roots) => roots,

        // Every caller of `TrustStore` treats a failure as "no answer" and carries on, which is the
        // rule `probe`'s own documentation states. Here it means no bundle, and therefore nothing
        // exported that would replace a runtime's own trust store with a shorter list.
        Err(error) => {
            tracing::warn!(
                %error,
                "could not read this machine's trusted roots, so no trust bundle was written"
            );
            Vec::new()
        }
    };

    let authority =
        std::fs::read_to_string(mixengine_core::certs::ca::certificate_path(certs)).ok();

    match ca::render(etc, &roots, authority.as_deref()) {
        Ok(true) if roots.len() >= ca::ROOT_FLOOR => {
            tracing::info!(
                roots = roots.len(),
                "wrote a trust bundle this home's runtimes can be pointed at"
            );
            true
        }
        Ok(true) => {
            tracing::warn!(
                roots = roots.len(),
                floor = ca::ROOT_FLOOR,
                "this machine's trust store answered too few roots to believe; the trust bundle \
                 was removed"
            );
            true
        }
        Ok(false) => false,
        Err(error) => {
            tracing::warn!(%error, "the trust bundle could not be written");
            false
        }
    }
}
