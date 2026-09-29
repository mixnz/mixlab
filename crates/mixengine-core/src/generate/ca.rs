//! One file every runtime can be pointed at — roadmap task **T132**.
//!
//! MixEngine installs its authority into the **operating system's** trust store, which is what
//! makes a browser show a padlock on `https://blog.test`
//! ([tls.md](../../../../docs/features/tls.md)). No language runtime reads that store:
//!
//! - **Node** ships a compiled-in copy of the Mozilla set and consults nothing else unless told to.
//! - **Ruby**'s OpenSSL resolves its default against the loaded `libcrypto` — a `ssl/cert.pem`
//!   inside the moved tree ([runtime-packaging.md](../../../../docs/operations/runtime-packaging.md)).
//! - **Python** uses OpenSSL's default paths, and `certifi`'s vendored bundle above that.
//! - **PHP** uses `openssl.cafile` and `curl.cainfo`, and the Windows artifact ships no CA file at
//!   all — so PHP there cannot verify *any* HTTPS through the openssl streams, and a local site is
//!   the case that makes somebody notice rather than the only one.
//!
//! So a Node process reaching `https://ezweb.test` and reporting `unable to verify the first
//! certificate` is doing exactly what it should, and MixEngine has never told it anything.
//!
//! # Why a merged bundle rather than our authority alone
//!
//! Because of what each mechanism *does*. `NODE_EXTRA_CA_CERTS` **adds** to what Node already
//! trusts, so Node is handed `certs/ca/root.crt` and keeps its own set. Every other mechanism here
//! — `SSL_CERT_FILE`, `REQUESTS_CA_BUNDLE`, `openssl.cafile`, `curl.cainfo` — **replaces** one. A
//! runtime pointed at a file holding one certificate would trust this home's sites and nothing else
//! on the internet, and `pip install` would be the first thing to stop working.
//!
//! So the file holds **every root this machine already trusts, and then ours**: the same answer the
//! browser on the same machine gives, which is the property that makes it defensible.
//!
//! # What it costs, stated plainly
//!
//! Python and Ruby stop using their own curated root sets and start using the machine's. On a
//! corporate laptop with an inspection proxy's certificate in the OS store, a Python script will
//! now trust that proxy — exactly as the browser beside it already does. That is the intended
//! behaviour and it is why this has a decision record of its own
//! ([ADR 0034](../../../../docs/decisions/0034-mixengines-authority-reaches-a-runtime-through-a-generated-bundle.md)).
//!
//! # The floor
//!
//! **A store that answers fewer than [`ROOT_FLOOR`] roots was read wrong, and nothing is written.**
//! A real store holds tens of them — 35 on the Windows this was measured on, some hundred and
//! forty in a Linux `ca-certificates`; a handful means the enumeration
//! failed in a way the call did not report. Writing a bundle from it would replace a working trust
//! store with a broken one on the next command somebody typed — so the file is not written, a stale
//! one is removed, and `mix doctor` says so. Node's additive variable is unaffected, which is why
//! it is the one that names the authority directly.

use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// The fewest roots a real machine's trust store holds.
///
/// **Twenty, and the number was measured rather than assumed.** A stock Windows 11 answers **35**:
/// that store is *seeded* with a small set and fetches the rest on demand, so the "hundred and
/// forty" a Linux `ca-certificates` holds is the high end of the range and not the middle of it.
/// Twenty is below every real answer this project has seen and far above a failed enumeration,
/// which is the only distinction this constant has to make — it is a floor, not an estimate, and
/// raising it towards a distribution's figure would refuse a working Windows.
pub const ROOT_FLOOR: usize = 20;

/// Where the bundle goes.
///
/// Under `etc/` and not under `certs/`, because `CLAUDE.md` puts generated configuration
/// there and says it is disposable: this file is rebuilt from the OS store and the authority on
/// every start, and nothing ever reads it back into state. `certs/` holds what cannot be rebuilt.
#[must_use]
pub fn path(etc: &Path) -> PathBuf {
    etc.join("ca").join("bundle.pem")
}

/// Write the bundle, or take away one that should no longer be there.
///
/// `authority_pem` is `certs/ca/root.crt`'s contents, and [`None`] is a home whose authority has
/// not been made yet — the bundle is still written, because the machine's own roots are worth
/// having in it on their own, and the runtimes are pointed at it the moment it exists.
///
/// Answers whether the file on disk changed, which is what a caller logs.
///
/// # Errors
///
/// [`Error::Io`] naming the file that could not be written or removed. **Not** an error for a store
/// that answers too few roots: that is the case this refuses to act on, and it is reported by
/// removing the file rather than by failing a daemon start.
pub fn render(etc: &Path, roots: &[Vec<u8>], authority_pem: Option<&str>) -> Result<bool> {
    let bundle = path(etc);

    if roots.len() < ROOT_FLOOR {
        return discard(&bundle);
    }

    let contents = compose(roots, authority_pem);

    // Compared before it is written, for `document::install`'s reason one file along: this runs at
    // every daemon start, and rewriting a two-hundred-kilobyte file each time would be a write for
    // nothing — and, worse, a modification time that moves under whatever is reading it.
    if std::fs::read_to_string(&bundle).is_ok_and(|held| held == contents) {
        return Ok(false);
    }

    let io = |action: &'static str, path: &Path| {
        let path = path.to_path_buf();
        move |source| Error::Io {
            action,
            path,
            source,
        }
    };

    let directory = bundle.parent().unwrap_or(etc);
    std::fs::create_dir_all(directory).map_err(io("create the directory at", directory))?;

    // **Written aside and renamed**, because a half-written bundle is every TLS connection on this
    // machine failing at once: a runtime that reads it mid-write gets a truncated PEM and trusts
    // nothing. A rename is atomic on every filesystem this project supports.
    //
    // **And written private**, through the platform's own writer. The certificates in it are
    // public; a bundle *another account may write* is that account choosing what every one of this
    // user's runtimes trusts.
    let staged = bundle.with_extension("pem.staged");
    mixengine_platform::write_private(&staged, contents.as_bytes())?;

    std::fs::rename(&staged, &bundle).map_err(|source| {
        let _ = std::fs::remove_file(&staged);
        io("install the trust bundle at", &bundle)(source)
    })?;

    Ok(true)
}

/// Take away a bundle this machine can no longer stand behind.
fn discard(bundle: &Path) -> Result<bool> {
    match std::fs::remove_file(bundle) {
        Ok(()) => Ok(true),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(Error::Io {
            action: "remove the stale trust bundle at",
            path: bundle.to_path_buf(),
            source,
        }),
    }
}

/// The file's text: a header a person and `mix doctor` can both read, then the certificates.
fn compose(roots: &[Vec<u8>], authority_pem: Option<&str>) -> String {
    let mut contents = String::with_capacity(roots.len() * 1_500);

    contents.push_str(
        "# Generated by MixEngine. Edits are overwritten, and nothing reads this file back into\n\
         # state. It holds every certificate authority this machine trusts, and MixEngine's own.\n",
    );

    // **The authority's fingerprint, in the header.** It is what makes a rotation a *changed* file:
    // `cert.ca_rotate` replaces the authority, this line changes, the comparison above finds a
    // difference and the bundle is installed — the same mechanism a renewed leaf uses to make a
    // front end re-read its configuration.
    match authority_pem.and_then(crate::certs::ca::der) {
        Some(der) => contents.push_str(&format!("# MixEngine authority: {}\n", fingerprint(&der))),
        None => contents.push_str("# MixEngine authority: none; this home has not made one yet\n"),
    }

    contents.push_str(&format!("# Machine roots: {}\n", roots.len()));

    for root in roots {
        contents.push_str(&encode(root));
    }

    if let Some(authority) = authority_pem {
        contents.push_str(authority);

        if !authority.ends_with('\n') {
            contents.push('\n');
        }
    }

    contents
}

/// One DER certificate, as the PEM envelope every consumer of this file reads.
fn encode(der: &[u8]) -> String {
    pem::encode(&pem::Pem::new("CERTIFICATE", der.to_vec()))
}

/// Lowercase hex of the SHA-256, which is how `cert.ca_status` spells a fingerprint.
fn fingerprint(der: &[u8]) -> String {
    use sha2::Digest as _;

    sha2::Sha256::digest(der)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// As many distinct made-up certificates as asked for. Never valid DER, because nothing here
    /// parses one — the PEM envelope carries whatever it is given.
    fn roots(how_many: usize) -> Vec<Vec<u8>> {
        (0..how_many)
            .map(|index| format!("a made-up root, number {index}").into_bytes())
            .collect()
    }

    /// A certificate in the envelope `certs::ca` writes, so the header's fingerprint is computed
    /// over something `der` can actually take apart.
    fn authority() -> String {
        pem::encode(&pem::Pem::new("CERTIFICATE", b"MixEngine's own".to_vec()))
    }

    /// **The whole of what the file is for**: this machine's roots, and then ours. A runtime pointed
    /// at it verifies `https://blog.test` *and* `https://crates.io`.
    #[test]
    fn the_bundle_holds_the_machines_roots_and_ours() {
        let home = tempfile::tempdir().expect("a temporary directory");

        assert!(render(home.path(), &roots(30), Some(&authority())).expect("a bundle"));

        let written = std::fs::read_to_string(path(home.path())).expect("the bundle");

        assert_eq!(
            written.matches("BEGIN CERTIFICATE").count(),
            31,
            "thirty of this machine's and one of ours"
        );
        assert!(written.contains("Machine roots: 30"), "{written}");
    }

    /// The header carries the authority's fingerprint, which is what makes a rotation a changed
    /// file — the same mechanism a renewed leaf uses to reload a front end.
    #[test]
    fn the_header_names_the_authority() {
        let home = tempfile::tempdir().expect("a temporary directory");

        render(home.path(), &roots(30), Some(&authority())).expect("a bundle");
        let first = std::fs::read_to_string(path(home.path())).expect("the bundle");

        let rotated = pem::encode(&pem::Pem::new("CERTIFICATE", b"a newer authority".to_vec()));
        assert!(
            render(home.path(), &roots(30), Some(&rotated)).expect("a second bundle"),
            "a rotation has to change the file"
        );

        let second = std::fs::read_to_string(path(home.path())).expect("the bundle");
        assert_ne!(first, second);
    }

    /// A start that changed nothing writes nothing: this runs every time the daemon starts, and a
    /// two-hundred-kilobyte rewrite per start would move the file under whatever is reading it.
    #[test]
    fn a_second_pass_over_an_unchanged_machine_writes_nothing() {
        let home = tempfile::tempdir().expect("a temporary directory");

        assert!(render(home.path(), &roots(30), Some(&authority())).expect("a bundle"));
        assert!(!render(home.path(), &roots(30), Some(&authority())).expect("a second pass"));
    }

    /// **Too few roots is a store read wrong**, and the bundle is not written: pointing a runtime
    /// at a file holding three certificates would break every public handshake on the machine.
    #[test]
    fn too_few_roots_writes_no_bundle() {
        let home = tempfile::tempdir().expect("a temporary directory");

        assert!(!render(home.path(), &roots(3), Some(&authority())).expect("no bundle"));
        assert!(!path(home.path()).exists());
    }

    /// And one that was there from a machine that *could* be read is taken away rather than left to
    /// go stale — a stale bundle is one a runtime keeps trusting.
    #[test]
    fn too_few_roots_removes_a_bundle_that_was_there() {
        let home = tempfile::tempdir().expect("a temporary directory");

        render(home.path(), &roots(30), Some(&authority())).expect("a bundle");
        assert!(path(home.path()).is_file());

        assert!(render(home.path(), &roots(0), Some(&authority())).expect("a removal"));
        assert!(!path(home.path()).exists());
    }

    /// A home whose authority has not been made yet still gets the machine's own roots: the file is
    /// worth having on its own, and the runtimes are pointed at it the moment it exists.
    #[test]
    fn a_home_with_no_authority_still_gets_the_machines_roots() {
        let home = tempfile::tempdir().expect("a temporary directory");

        assert!(render(home.path(), &roots(30), None).expect("a bundle"));

        let written = std::fs::read_to_string(path(home.path())).expect("the bundle");
        assert_eq!(written.matches("BEGIN CERTIFICATE").count(), 30);
        assert!(written.contains("has not made one yet"), "{written}");
    }

    /// Nothing is left beside the bundle: a staged file that survived would be read by the next
    /// pass's comparison and be a second answer to what this machine trusts.
    #[test]
    fn nothing_is_left_beside_the_bundle() {
        let home = tempfile::tempdir().expect("a temporary directory");

        render(home.path(), &roots(30), Some(&authority())).expect("a bundle");

        let directory = path(home.path())
            .parent()
            .expect("the bundle has a directory")
            .to_path_buf();

        let names: Vec<String> = std::fs::read_dir(&directory)
            .expect("a directory")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();

        assert_eq!(names, vec!["bundle.pem".to_owned()]);
    }
}
