//! The leaf a managed server presents.
//!
//! One certificate per service that asks for one, signed by this home's authority, under
//! `certs/services/<service-id>.{key,crt}`. It exists because of one measurement: from 11.4 MariaDB
//! turns TLS on by default, and a server with no certificate configured *generates one at every
//! start* — a 4096-bit RSA key, four to thirteen seconds of silence on a CI runner, the whole of
//! the M3 warm start's spread. Turning TLS off is not an answer, because an 11.4 client with a
//! password on its command line insists on it and is refused by a server without it. A pair on disk
//! costs the server two file reads. The design is
//! `docs/specs/2026-09-07-t99-a-certificate-for-the-database-design.md`.
//!
//! **Shaped like [`super::leaf`] on purpose, and made of it.** The same four exports, the same
//! four questions before a pair is reused, the same order of writes — because the body *is*
//! [`super::leaf`]'s, behind a second pair of paths. A site's leaf is named by its primary domain
//! and covers the site's domains; a service's is named by its id and covers `localhost` and the
//! address it binds. Ninety days and a thirty-day renewal window are the site leaf's numbers,
//! deliberately: a second lifetime would be a second rule to keep in step, and a database restarted
//! less than once a quarter is not the laptop this product is for.
//!
//! **Renewed at a start, never reloaded.** MariaDB reads its certificate once; a pair reissued under
//! a running server is served at the next `mix service restart`, and nothing here pretends
//! otherwise.

use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use mixengine_proto::{CertState, ServiceId};

use super::leaf::{self, Issued};
use crate::Result;

/// Where the service leaves live, under the certificates directory — beside `sites/`.
const SERVICES: &str = "services";

/// The name every service leaf covers, and the one that becomes its common name.
const LOCALHOST: &str = "localhost";

/// Where this service's private key lives.
///
/// **Named after the service id**, which [`ServiceId::parse`] has already restricted to lowercase
/// letters, digits, `-`, `.` and one `@` — every one of them an ordinary character in a file name
/// on all three systems, and the same string the service's `etc/` and log directories are named
/// by.
#[must_use]
pub fn key_path(certs: &Path, service: &ServiceId) -> PathBuf {
    certs
        .join(SERVICES)
        .join(format!("{}.key", service.as_str()))
}

/// Where this service's certificate lives.
#[must_use]
pub fn certificate_path(certs: &Path, service: &ServiceId) -> PathBuf {
    certs
        .join(SERVICES)
        .join(format!("{}.crt", service.as_str()))
}

/// What is on disk for this service, without changing any of it.
#[must_use]
pub fn read(certs: &Path, service: &ServiceId, now: SystemTime) -> CertState {
    leaf::read_pair(
        &key_path(certs, service),
        &certificate_path(certs, service),
        now,
    )
}

/// Give this service a certificate covering exactly `names`, if what is there is not already one.
///
/// [`leaf::ensure`]'s four questions, asked of `certs/services/<id>`: both halves there and each
/// other's, the names equal, more than thirty days left, signed by the authority this home has
/// **now** — so a rotation reissues a service leaf for the reason it reissues a site's.
///
/// # Errors
///
/// [`Error::Certificate`](crate::Error::Certificate) when this home has no usable authority, when
/// `names` is empty, or when the machine will not produce a key pair;
/// [`Error::Io`](crate::Error::Io) when the pair cannot be written.
pub fn ensure(
    certs: &Path,
    service: &ServiceId,
    names: &[String],
    now: SystemTime,
) -> Result<(Issued, CertState)> {
    leaf::issue_at(
        certs,
        &key_path(certs, service),
        &certificate_path(certs, service),
        names,
        now,
    )
}

/// Every name a service listening on `bind` needs its certificate to cover.
///
/// `localhost` first, so it is the common name, then `bind` when it is an IPv4 literal that is not
/// already there. **IPv4 only**, for the reason [`leaf`] gives: the reuse check compares the names
/// read back off the disk against the names asked for, and a name that does not round-trip — an
/// IPv6 address, which `leaf::names` does not render — is a certificate reissued at every start. A
/// client connecting to `::1` with verification on takes the password-hash fallback it takes today.
///
/// One function rather than a list written in the recipe and again in a test, so the certificate
/// and the assertion about it are computed over one answer.
#[must_use]
pub fn names(bind: &str) -> Vec<String> {
    let mut names = vec![LOCALHOST.to_owned()];

    if bind.parse::<Ipv4Addr>().is_ok() && bind != LOCALHOST {
        names.push(bind.to_owned());
    }

    names
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use mixengine_proto::Unusable;

    use super::super::ca;
    use super::*;

    /// A home with an authority and nothing else.
    fn a_home() -> tempfile::TempDir {
        let home = tempfile::tempdir().expect("a temp home");
        ca::ensure(home.path(), SystemTime::now()).expect("an authority is made");
        home
    }

    fn mariadb() -> ServiceId {
        ServiceId::parse("mariadb@main").expect("an id")
    }

    #[test]
    fn a_service_with_no_certificate_is_absent() {
        let home = a_home();

        assert_eq!(
            read(home.path(), &mariadb(), SystemTime::now()),
            CertState::Absent {}
        );
    }

    /// The pair lands under `services/`, named by the id, covering exactly what was asked, signed
    /// by this home's authority, and says so when read back.
    #[test]
    fn ensure_writes_a_pair_under_services_and_reads_it_back() {
        let home = a_home();
        let now = SystemTime::now();
        let names = names("127.0.0.1");

        let (issued, state) = ensure(home.path(), &mariadb(), &names, now).expect("it issues");

        assert_eq!(issued, Issued::Written);
        assert!(
            home.path()
                .join("services")
                .join("mariadb@main.key")
                .is_file()
        );
        assert!(
            home.path()
                .join("services")
                .join("mariadb@main.crt")
                .is_file()
        );

        let CertState::Present { cert } = state else {
            panic!("not present after issuing: {state:?}");
        };
        assert_eq!(cert.sans, names);
        assert!(cert.subject.contains("localhost"), "{}", cert.subject);

        let mixengine_proto::CaState::Present { ca } = ca::read(home.path(), now) else {
            panic!("the authority went missing");
        };
        assert_eq!(cert.issuer, ca.subject);
        assert_eq!(
            read(home.path(), &mariadb(), now),
            CertState::Present { cert }
        );
    }

    #[test]
    fn a_second_ensure_reuses() {
        let home = a_home();
        let now = SystemTime::now();
        let names = names("127.0.0.1");

        let (_, before) = ensure(home.path(), &mariadb(), &names, now).expect("it issues");
        let (issued, after) = ensure(home.path(), &mariadb(), &names, now).expect("it answers");

        assert_eq!(issued, Issued::Reused);
        assert_eq!(before, after);
    }

    /// Question two: a service moved to another address gets a certificate that covers it.
    #[test]
    fn a_changed_bind_reissues() {
        let home = a_home();
        let now = SystemTime::now();

        let (_, before) =
            ensure(home.path(), &mariadb(), &names("127.0.0.1"), now).expect("it issues");
        let (issued, after) =
            ensure(home.path(), &mariadb(), &names("192.168.1.10"), now).expect("it issues");

        assert_eq!(issued, Issued::Written);
        assert_ne!(before, after);
    }

    /// Question three, with the site leaf's numbers.
    #[test]
    fn a_pair_near_expiry_reissues() {
        let home = a_home();
        let now = SystemTime::now();
        let names = names("127.0.0.1");
        let day = Duration::from_secs(24 * 60 * 60);

        ensure(home.path(), &mariadb(), &names, now).expect("it issues");

        let (issued, _) =
            ensure(home.path(), &mariadb(), &names, now + day * 50).expect("it answers");
        assert_eq!(issued, Issued::Reused);

        let (issued, _) =
            ensure(home.path(), &mariadb(), &names, now + day * 70).expect("it issues");
        assert_eq!(issued, Issued::Written);
    }

    /// Question four: a rotated authority reissues a service leaf for the reason it reissues a
    /// site's.
    #[test]
    fn a_rotated_authority_reissues() {
        let home = a_home();
        let now = SystemTime::now();
        let names = names("127.0.0.1");

        let (_, before) = ensure(home.path(), &mariadb(), &names, now).expect("it issues");

        let elsewhere = tempfile::tempdir().expect("a second temp home");
        ca::ensure(elsewhere.path(), now).expect("a second authority");
        std::fs::copy(ca::key_path(elsewhere.path()), ca::key_path(home.path()))
            .expect("the key is replaced");
        std::fs::copy(
            ca::certificate_path(elsewhere.path()),
            ca::certificate_path(home.path()),
        )
        .expect("the certificate is replaced");

        let (issued, after) = ensure(home.path(), &mariadb(), &names, now).expect("it issues");

        assert_eq!(issued, Issued::Written);
        assert_ne!(before, after);
    }

    #[test]
    fn half_a_pair_is_unusable() {
        let home = a_home();
        let now = SystemTime::now();

        ensure(home.path(), &mariadb(), &names("127.0.0.1"), now).expect("it issues");
        std::fs::remove_file(key_path(home.path(), &mariadb())).expect("the key is removed");

        assert_eq!(
            read(home.path(), &mariadb(), now),
            CertState::Unusable {
                because: Unusable::KeyMissing
            }
        );
    }

    #[test]
    fn names_keeps_localhost_first_and_takes_only_an_ipv4_literal() {
        assert_eq!(names("127.0.0.1"), vec!["localhost", "127.0.0.1"]);
        assert_eq!(names("10.0.0.5"), vec!["localhost", "10.0.0.5"]);
        assert_eq!(names("localhost"), vec!["localhost"]);
        assert_eq!(names("::1"), vec!["localhost"]);
        assert_eq!(names("db.internal"), vec!["localhost"]);
    }

    #[test]
    fn a_home_without_an_authority_refuses() {
        let home = tempfile::tempdir().expect("a temp home");

        let refused = ensure(
            home.path(),
            &mariadb(),
            &names("127.0.0.1"),
            SystemTime::now(),
        );

        assert!(refused.is_err());
        assert!(!home.path().join("services").exists());
    }
}
