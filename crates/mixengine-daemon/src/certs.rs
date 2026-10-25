//! This home's certificate authority: made once, and reported on.
//!
//! **Made at start rather than when the first HTTPS site is created.**
//! `docs/architecture/security-model.md` promises one elevation prompt at first run, covering the
//! CA, the resolver wiring and the port grant together. An authority that first appeared with the
//! first site would put its trust-store install in a second batch and therefore behind a second
//! prompt — which is the finding the resolver had already made and written down in `main.rs` beside
//! the block this one sits under. Generating here costs one ECDSA key on disk in a home that never
//! serves HTTPS; the alternative costs a second prompt to everybody who does.
//!
//! Everything about what an authority *is* lives in `mixengine_core::certs::ca`. This module is the
//! two things that are the daemon's: when it happens, and which thread it happens on.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use mixengine_proto::{
    BrowserDatabase, Browsers, CaState, CaStatus, CertIssueReport, Error, ErrorCode, IssueOutcome,
    SiteCertOutcome, Trust,
};

use crate::error::ToWire as _;

pub(crate) mod authority;
pub(crate) mod bundle;
pub(crate) mod handshake;
pub(crate) mod jdks;
pub(crate) mod renewal;

/// Everything this needs, which is one directory.
///
/// **`Clone` because a job takes an owned one.** Every field clones cheaply: a path, an `Arc`, and
/// a `Store` whose own documentation says one pool sits behind it.
#[derive(Debug, Clone)]
pub(crate) struct Certificates {
    certs: PathBuf,

    /// Where the generated trust bundle goes.
    ///
    /// `Option` for `host`'s reason one field down: `in_directory` is handed a certificates
    /// directory and no `Paths`, and repairing a certificate needs no bundle to do it.
    etc: Option<PathBuf>,

    /// This machine, for the trust-store half of the answer.
    ///
    /// `Option` because `ensure` is called at start from a place that has `Paths` and not yet an
    /// `Api`, and a home whose authority is being made does not need the store read to make it.
    /// `status` always has one.
    host: Option<Arc<dyn mixengine_platform::Host>>,

    /// The rows, for the sites a certificate is issued *for*.
    ///
    /// `Option` for `host`'s reason: `ensure` is called at start from a place that has `Paths` and
    /// not yet a `Store`, and a home whose authority is being made does not need the site list to
    /// make it. Without one, `issue(None)` answers for no sites rather than failing.
    store: Option<mixengine_core::Store>,
}

impl Certificates {
    pub(crate) fn new(paths: &mixengine_core::Paths) -> Self {
        Self {
            certs: paths.certs().to_path_buf(),
            etc: Some(paths.etc().to_path_buf()),
            host: None,
            store: None,
        }
    }

    /// The one the API holds, which can also say whether this machine trusts what it finds.
    pub(crate) fn reading(
        paths: &mixengine_core::Paths,
        host: Arc<dyn mixengine_platform::Host>,
    ) -> Self {
        Self {
            etc: Some(paths.etc().to_path_buf()),
            ..Self::in_directory(paths.certs(), host)
        }
    }

    /// The same, for a caller that kept the directory rather than the whole `Paths`.
    ///
    /// `crate::repair` is the one: it stores `certs` alone, and rebuilding a `Paths` from a root to
    /// get back to it would be a second way of deciding where certificates live.
    pub(crate) fn in_directory(
        certs: &std::path::Path,
        host: Arc<dyn mixengine_platform::Host>,
    ) -> Self {
        Self {
            certs: certs.to_path_buf(),
            etc: None,
            host: Some(host),
            store: None,
        }
    }

    /// The one the API holds: it can read the machine's stores *and* walk this home's own sites.
    pub(crate) fn issuing(
        paths: &mixengine_core::Paths,
        host: Arc<dyn mixengine_platform::Host>,
        store: mixengine_core::Store,
    ) -> Self {
        Self {
            store: Some(store),
            ..Self::reading(paths, host)
        }
    }

    /// Rewrite `etc/ca/bundle.pem` around whatever authority is on disk now.
    ///
    /// Called after a rotation commits. The file's header carries the authority's fingerprint, so a
    /// replaced authority makes it a *changed* file and every runtime started afterwards is handed
    /// the new one — the same mechanism a reissued leaf uses to make a front end re-read.
    ///
    /// A [`Certificates`] built without a `Paths` (`in_directory`, which `crate::repair` holds) or
    /// without a host does nothing: neither has a bundle to write or a store to read it from.
    pub(crate) fn rebuild_trust_bundle(&self) {
        let (Some(etc), Some(host)) = (&self.etc, &self.host) else {
            return;
        };

        bundle::render_into(etc, &self.certs, host.as_ref());
    }

    /// Whether this site is actually served over TLS right now.
    ///
    /// **The same two questions `generate::served` asks**, and deliberately the same two: a site
    /// declaring HTTPS with no usable pair on disk renders no TLS listener at all, so a firewall
    /// plan that opened the TLS port for it would be opening a port nothing answers on. Asking the
    /// row alone would do exactly that.
    pub(crate) fn serves_tls(&self, site: &mixengine_core::sites::SiteRecord) -> bool {
        if !site.https_enabled {
            return false;
        }

        let Some(primary) = site.domains.first() else {
            return false;
        };

        matches!(
            mixengine_core::certs::leaf::read(&self.certs, primary, std::time::SystemTime::now()),
            mixengine_proto::CertState::Present { .. }
        )
    }

    /// Make the authority if this home has none, and answer with what is there either way.
    ///
    /// Idempotent, and never destructive: an authority that is present and broken is left alone and
    /// reported, because replacing it would invalidate every leaf and every trust store that holds
    /// it. See `mixengine_core::certs::ca`.
    ///
    /// # Errors
    ///
    /// Whatever `certs/ca/` could not be made or written, and the case where this machine will not
    /// produce a key pair at all. Callers at start log it and carry on.
    pub(crate) async fn ensure(&self) -> Result<CaStatus, Error> {
        let certs = self.certs.clone();

        // Key generation and two file writes. `docs/standards/rust.md`'s rule for anything that
        // touches a disk from a runtime worker — and on Windows the private key's ACL is written by
        // running `icacls`, which is a process rather than a syscall.
        //
        // **The conversion to a wire error happens inside the closure**, not after the `await`.
        // `mixengine_core::Error` is over 128 bytes, so carrying it out through the task's own
        // `Result` puts a large error in two frames and `clippy::result_large_err` says so — the
        // same boundary every other module in this crate converts at.
        let state = blocking("making", move || {
            mixengine_core::certs::ca::ensure(&certs, SystemTime::now())
                .map_err(|error| error.to_wire())
        })
        .await??;

        Ok(CaStatus {
            trust: self.trust(&state),
            browsers: self.browsers(&state),
            state,
        })
    }

    /// Give one site, or every HTTPS site, the certificate its names need.
    ///
    /// **Takes records and never a [`SiteRef`](mixengine_proto::SiteRef).** Resolving a reference
    /// to a row is `crate::sites::Sites::expect`, and that struct holds a `Certificates` of its own
    /// so a site gains its certificate before `site.create` answers — a `Certificates` that
    /// resolved references would close that loop. The two callers that have a reference already
    /// have the row it names: the RPC dispatch resolves it, and `sites` is holding the record it
    /// just wrote.
    ///
    /// **One site's refusal never takes the others with it.** A home with no authority answers
    /// `Refused` for each site by name rather than failing the call, which is the shape
    /// `BrowserChange` already has and the only shape a report over N sites can honestly take.
    ///
    /// # Errors
    ///
    /// Only when the site rows cannot be read at all, which is the `None` form's first step. A home
    /// with no store answers for no sites — see the test.
    pub(crate) async fn issue(
        &self,
        site: Option<mixengine_core::sites::SiteRecord>,
    ) -> Result<CertIssueReport, Error> {
        let records = match site {
            Some(one) => vec![one],
            None => match self.store.as_ref() {
                Some(store) => mixengine_core::sites::records(store, None)
                    .await
                    .map_err(|error| error.to_wire())?,
                None => Vec::new(),
            },
        };

        let certs = self.certs.clone();
        let now = SystemTime::now();

        // Key generation and two file writes per site — the rule every disk-touching call in this
        // module follows.
        let sites = blocking("issuing certificates for", move || {
            records
                .into_iter()
                .map(|record| issued(&certs, &record, now))
                .collect()
        })
        .await?;

        Ok(CertIssueReport { sites })
    }

    /// What this home's authority is, and nothing about the machine holding it.
    ///
    /// [`Self::status`] also asks this machine's trust stores and its browser databases, and on
    /// Linux the second of those spawns `certutil` once per profile. That is a fair price on a
    /// start and an unfair one every hour, which is why the renewal loop reads this instead — and
    /// `status` is built on top of it so that the two cannot come to disagree about what reading an
    /// authority means.
    ///
    /// # Errors
    ///
    /// Only when the task reading it does not finish. A home with no authority, or one whose
    /// authority is damaged, is an answer rather than a failure — see [`CaState`].
    pub(crate) async fn authority(&self) -> Result<CaState, Error> {
        let certs = self.certs.clone();

        blocking("reading", move || {
            mixengine_core::certs::ca::read(&certs, SystemTime::now())
        })
        .await
    }

    /// What every site's certificate is doing, on disk and on the wire.
    ///
    /// **The one answer in this module that comes from a socket.** Everything else here reads a
    /// file and believes it; this opens a connection to the front end running on this machine and
    /// reports the certificate that server actually presents, which is the only thing a browser
    /// ever sees.
    ///
    /// `tls_port` is [`None`] for a home whose front end is not installed or cannot be read. Every
    /// site then answers `NotServed`, which is what is true.
    ///
    /// **Reads only.** A site with a problem is reported and never repaired: a diagnostic that
    /// fixed what it found would be unable to report the state it had just fixed, and
    /// `ServedCertificateDiffers` in particular would disappear the moment it was looked at.
    ///
    /// # Errors
    ///
    /// Whatever reading this home's rows or its authority failed with. A site nothing answers for
    /// is an answer rather than a failure — see [`mixengine_proto::Handshake`].
    pub(crate) async fn site_status(
        &self,
        site: Option<mixengine_core::sites::SiteRecord>,
        tls_port: Option<u16>,
    ) -> Result<mixengine_proto::CertStatusReport, Error> {
        let records = match site {
            Some(one) => vec![one],
            None => match self.store.as_ref() {
                Some(store) => mixengine_core::sites::records(store, None)
                    .await
                    .map_err(|error| error.to_wire())?,
                None => Vec::new(),
            },
        };

        // Read once for the whole walk rather than per site: it is the same authority for all of
        // them, and it is the trust root every handshake below is judged against.
        let authority = match self.authority().await? {
            CaState::Present { ca } => mixengine_core::certs::ca::der(&ca.certificate_pem),
            CaState::Absent {} | CaState::Unusable { .. } => None,
        };

        let now = SystemTime::now();
        let mut sites = Vec::with_capacity(records.len());

        for record in records {
            let Some(domain) = record.domains.first().cloned() else {
                continue;
            };

            let certs = self.certs.clone();
            let primary = domain.clone();
            let disk = blocking("reading a certificate for", move || {
                mixengine_core::certs::leaf::read(&certs, &primary, now)
            })
            .await?;

            let handshake = match (record.https_enabled, tls_port, authority.as_deref()) {
                (false, _, _) => mixengine_proto::Handshake::NotAsked {},

                (true, Some(port), Some(authority)) => {
                    crate::certs::handshake::against(&domain, port, authority, now).await
                }

                (true, None, _) => mixengine_proto::Handshake::NotServed {
                    because: "this home has no front end that could serve it".to_owned(),
                },

                (true, Some(_), None) => mixengine_proto::Handshake::NotServed {
                    because: "this home has no usable authority to judge a certificate against"
                        .to_owned(),
                },
            };

            // **What the issuer covered, not what the row lists.** LAN sharing first shipped with
            // the LAN address in the certificate and this comparison reading the bare domain list,
            // so every shared site answered `NamesDiffer` for as long as it was shared.
            let covered =
                mixengine_core::certs::leaf::covered(&record.domains, record.sharing.as_ref());

            sites.push(mixengine_proto::SiteCertStatus {
                problem: problem(record.https_enabled, &covered, &disk, &handshake),
                domain,
                domains: record.domains,
                disk,
                handshake,
            });
        }

        Ok(mixengine_proto::CertStatusReport { sites })
    }

    /// What is on disk, without changing any of it.
    ///
    /// # Errors
    ///
    /// Only when the task reading it does not finish. A home with no authority, or one whose
    /// authority is damaged, is an answer rather than a failure — see
    /// [`CaState`].
    pub(crate) async fn status(&self) -> Result<CaStatus, Error> {
        let state = self.authority().await?;

        Ok(CaStatus {
            trust: self.trust(&state),
            browsers: self.browsers(&state),
            state,
        })
    }

    /// Whether this machine holds the authority that was just read.
    ///
    /// **Every branch that cannot ask says why rather than guessing.** A client renders this
    /// sentence, and "not installed" printed because nothing was asked would be the daemon inventing
    /// an answer — which is the rule `CLAUDE.md` states as a client rendering only what the
    /// daemon returns, one layer up.
    fn trust(&self, state: &CaState) -> Trust {
        let CaState::Present { ca } = state else {
            return Trust::Unknown {
                because: "this home has no usable certificate authority, so its trust store was not asked"
                    .to_owned(),
            };
        };

        let (Some(host), Some(der)) = (
            self.host.as_ref(),
            mixengine_core::certs::ca::der(&ca.certificate_pem),
        ) else {
            return Trust::Unknown {
                because: "this machine's trust store was not read".to_owned(),
            };
        };

        match host.trust_store().probe(&der) {
            Ok(state) if state.installed => Trust::Installed {
                store: store(state.method),
            },
            Ok(state) if state.method == mixengine_platform::TrustStoreMethod::None => {
                Trust::NoStore {
                    because: state.missing.unwrap_or_else(|| {
                        "this machine has no system trust store MixEngine knows how to write"
                            .to_owned()
                    }),
                }
            }
            Ok(state) => Trust::NotInstalled {
                because: state
                    .missing
                    .unwrap_or_else(|| format!("{} does not hold it", store(state.method))),
            },
            Err(error) => Trust::Unknown {
                because: format!("this machine's trust store could not be read: {error}"),
            },
        }
    }

    /// Ask this machine's browsers to hold the authority this home has, if it has a usable one.
    ///
    /// **No prompt, and no elevation queue.** These databases belong to the user, which is the line
    /// between them and the system stores: the system stores need root and ride in the first-run
    /// batch, and this is a subprocess in the user's own home.
    ///
    /// **Never fails a start, and never fails a repair.** A machine with no `certutil`, no browser
    /// profile, or a locked one is a machine that keeps working; what happened comes back in the
    /// return value, which is what the caller logs and what `mix doctor --repair` reports.
    ///
    /// **Takes the state rather than reading it.** Both callers already have one — the start made
    /// it a few lines above and the repair read it to decide there was anything to repair — and
    /// asking for it again here would run the *system* trust-store probe a second time on every
    /// daemon start, which on Linux means parsing the whole `ca-certificates.crt` bundle twice to
    /// answer a question about browsers.
    ///
    /// The state and not the DER, so that the one decision worth making — a damaged authority is
    /// not written anywhere — lives here and is tested, rather than being repeated at each call.
    pub(crate) async fn install_in_browsers(
        &self,
        state: &CaState,
    ) -> mixengine_platform::BrowserChange {
        let Some(host) = self.host.clone() else {
            return mixengine_platform::BrowserChange::default();
        };

        // Only a present authority has bytes to install, exactly as the trust-store producer
        // decides: asking a browser to hold a damaged certificate would be writing something a
        // rotation has to replace anyway.
        let CaState::Present { ca } = state else {
            return mixengine_platform::BrowserChange::default();
        };

        let Some(der) = mixengine_core::certs::ca::der(&ca.certificate_pem) else {
            return mixengine_platform::BrowserChange::default();
        };

        // Process spawns and file writes, so off the runtime — `docs/standards/rust.md`'s rule
        // for anything that touches a disk from a worker.
        tokio::task::spawn_blocking(move || host.browsers().install(&der))
            .await
            .unwrap_or_else(|_| {
                Ok(mixengine_platform::BrowserChange {
                    written: Vec::new(),
                    refused: vec![
                        "the task asking this machine's browsers did not finish".to_owned(),
                    ],
                })
            })
            .unwrap_or_else(|error| mixengine_platform::BrowserChange {
                written: Vec::new(),
                refused: vec![mixengine_proto::flatten(&error)],
            })
    }

    /// Ask this machine's browsers to let the authority `key_id` names go.
    ///
    /// The mirror of [`install_in_browsers`](Self::install_in_browsers), and unprivileged for its
    /// reason: these databases belong to the user, not to the system.
    ///
    /// **Names an authority and never a certificate.** What sits under the nickname is read back
    /// and checked before anything is deleted, which is
    /// [`BrowserTrust::remove`](mixengine_platform::BrowserTrust::remove)'s own guarantee and not
    /// this method's to repeat.
    pub(crate) async fn remove_from_browsers(
        &self,
        key_id: &str,
    ) -> mixengine_platform::BrowserChange {
        let Some(host) = self.host.clone() else {
            return mixengine_platform::BrowserChange::default();
        };

        let key_id = key_id.to_owned();

        // A process spawn per profile, so off the runtime — `docs/standards/rust.md`.
        tokio::task::spawn_blocking(move || host.browsers().remove(&key_id))
            .await
            .unwrap_or_else(|_| {
                Ok(mixengine_platform::BrowserChange {
                    written: Vec::new(),
                    refused: vec![
                        "the task asking this machine's browsers did not finish".to_owned(),
                    ],
                })
            })
            .unwrap_or_else(|error| mixengine_platform::BrowserChange {
                written: Vec::new(),
                refused: vec![mixengine_proto::flatten(&error)],
            })
    }

    /// Put this home's authority into every installed JDK that lacks it.
    ///
    /// **Nothing without a store**, on [`rebuild_trust_bundle`](Self::rebuild_trust_bundle)'s rule:
    /// which JDKs exist is a question about rows, and a `Certificates` built without them is one
    /// repairing a certificate rather than a home.
    pub(crate) async fn hold_in_jdks(&self) -> jdks::JdkChange {
        match &self.store {
            Some(store) => jdks::hold(store, &self.certs).await,
            None => jdks::JdkChange::default(),
        }
    }

    /// Let the authority `key_id` names go from every installed JDK.
    pub(crate) async fn release_from_jdks(&self, key_id: &str) -> jdks::JdkChange {
        match &self.store {
            Some(store) => jdks::release(store, key_id).await,
            None => jdks::JdkChange::default(),
        }
    }

    /// Enqueue taking `ca` out of this machine's trust store, and say whether anything was.
    ///
    /// `Ok(false)` is a machine with no store MixEngine can write, one that is not holding this
    /// authority, or one whose store could not be read — **the hosts file's rule that the disk is
    /// read before a prompt is spent, one capability along**: a prompt spent on a row whose only
    /// possible outcome is `AlreadyDone` is a prompt spent for nothing, and a probe that failed has
    /// said nothing about what to ask for.
    ///
    /// # Errors
    ///
    /// The wire error of a row that could not be written.
    pub(crate) async fn require_untrust(
        &self,
        elevation: &crate::elevation::Elevation,
        ca: &mixengine_proto::Ca,
    ) -> Result<bool, Error> {
        let (Some(host), Some(der)) = (
            self.host.as_ref(),
            mixengine_core::certs::ca::der(&ca.certificate_pem),
        ) else {
            return Ok(false);
        };

        let state = match host.trust_store().probe(&der) {
            Ok(state) if state.installed => state,
            Ok(_) => return Ok(false),
            Err(error) => {
                tracing::warn!(
                    %error,
                    "this machine's trust store cannot be read; asking to remove nothing"
                );
                return Ok(false);
            }
        };

        // **By key-id and never by fingerprint.** A removal that could name a
        // fingerprint could name the root that validates this machine's own updates.
        match state.target(&ca.key_id) {
            Some(target) => {
                elevation
                    .enqueue(&mixengine_proto::privileged::PrivilegedOp::TrustCaRemove { target })
                    .await?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// What this machine's browsers hold.
    ///
    /// **Every branch that cannot ask says why**, exactly as [`Self::trust`] does: a client renders
    /// this sentence, and "not installed" printed because nothing was asked would be the daemon
    /// inventing an answer.
    ///
    /// A different question from [`Self::trust`] and not a refinement of it — Firefox and Chrome on
    /// Linux read these databases and not the system store at all.
    fn browsers(&self, state: &CaState) -> Browsers {
        let CaState::Present { ca } = state else {
            return Browsers::Unknown {
                because: "this home has no usable certificate authority, so no browser was asked"
                    .to_owned(),
            };
        };

        let (Some(host), Some(der)) = (
            self.host.as_ref(),
            mixengine_core::certs::ca::der(&ca.certificate_pem),
        ) else {
            return Browsers::Unknown {
                because: "this machine's browsers were not asked".to_owned(),
            };
        };

        match host.browsers().survey(&der) {
            Ok(mixengine_platform::BrowserSurvey::Reached { databases }) => Browsers::Reached {
                databases: databases.into_iter().map(database).collect(),
            },
            Ok(mixengine_platform::BrowserSurvey::NoTool { because }) => {
                Browsers::NoTool { because }
            }
            Ok(mixengine_platform::BrowserSurvey::NotSearched { because }) => {
                Browsers::NotSearched { because }
            }
            Err(error) => Browsers::Unknown {
                because: format!("this machine's browsers could not be asked: {error}"),
            },
        }
    }
}

/// One database, as a client sees it.
///
/// The platform's own type and the wire's are deliberately separate — the split `TrustState` and
/// `Trust` already make, one capability along — so this is where they meet.
/// One site's outcome.
///
/// A free function beside [`database`] and for its reason: it takes what it needs and holds nothing,
/// so the closure `issue` hands to a blocking thread can call it without carrying a `&self` across.
fn issued(
    certs: &std::path::Path,
    site: &mixengine_core::sites::SiteRecord,
    now: SystemTime,
) -> SiteCertOutcome {
    let domain = site.domains.first().cloned().unwrap_or_default();

    let refused = |because: String| SiteCertOutcome {
        domain: domain.clone(),
        outcome: IssueOutcome::Refused { because },
        state: mixengine_proto::CertState::Absent {},
    };

    if site.domains.is_empty() {
        return refused("this site has no domains".to_owned());
    }

    // **Not a refusal.** This site asked for no certificate, and the renewal loop announces every
    // failure it finds: under one name with `Refused` it would announce one per plaintext site,
    // once an hour, for as long as the daemon runs.
    if !site.https_enabled {
        return SiteCertOutcome {
            domain,
            outcome: IssueOutcome::NotWanted {
                because: "this site does not declare HTTPS".to_owned(),
            },
            state: mixengine_proto::CertState::Absent {},
        };
    }

    // The sharing row itself, when this site is shared. It travels with the row rather than as an
    // argument, so every path that reissues a certificate covers what the site currently answers
    // on: a share, an unshare, and the renewal timer that knows about neither. What that comes to
    // is `leaf::covered`'s to say, not this function's.
    match mixengine_core::certs::leaf::ensure(certs, &site.domains, site.sharing.as_ref(), now) {
        Ok((mixengine_core::certs::leaf::Issued::Written, state)) => SiteCertOutcome {
            domain,
            outcome: IssueOutcome::Issued {},
            state,
        },
        Ok((mixengine_core::certs::leaf::Issued::Reused, state)) => SiteCertOutcome {
            domain,
            outcome: IssueOutcome::Reused {},
            state,
        },
        Err(error) => refused(mixengine_proto::flatten(&error)),
    }
}

/// The one condition worth acting on, in the order a person would act.
///
/// **First match wins, and the order is the whole of it.** A site can be wrong in several ways at
/// once, and naming all of them leaves whoever reads the answer to decide which to fix first —
/// which is the work this function exists to do. `NoCertificate` comes before `NamesDiffer` because
/// there is nothing to compare names against; the wire comes before the clock because a padlock
/// that is red now outranks one that will be.
fn problem(
    https: bool,
    domains: &[String],
    disk: &mixengine_proto::CertState,
    handshake: &mixengine_proto::Handshake,
) -> Option<mixengine_proto::CertProblem> {
    use mixengine_proto::{CertProblem, CertState, Handshake, Verdict};

    // **A site that asked for no certificate has no problem**, which is the distinction
    // `IssueOutcome::NotWanted` exists to draw. Without this line every plaintext site in the home
    // would be reported as missing a certificate it never wanted.
    if !https {
        return None;
    }

    let CertState::Present { cert } = disk else {
        return Some(CertProblem::NoCertificate);
    };

    if cert.sans != domains {
        return Some(CertProblem::NamesDiffer);
    }

    match handshake {
        // Unreachable while `https` is true, and answered rather than asserted: a panic here would
        // turn the command that diagnoses this home into the thing that needs diagnosing.
        Handshake::NotAsked {} => return None,

        Handshake::NotServed { .. } | Handshake::Failed { .. } => {
            return Some(CertProblem::NotServed);
        }

        Handshake::Presented {
            cert: served,
            trust,
        } => {
            // **By fingerprint and not by names.** A hash differs whenever anything differs — a
            // renewal, a rotation, a reissue covering the same names — where comparing names would
            // call a server holding last month's certificate correct.
            if served.fingerprint != cert.fingerprint {
                return Some(CertProblem::ServedCertificateDiffers);
            }

            if matches!(trust, Verdict::Rejected { .. }) {
                return Some(CertProblem::NotTrusted);
            }
        }
    }

    (cert.days_left <= mixengine_core::certs::leaf::RENEW_WITHIN_DAYS)
        .then_some(CertProblem::Expiring)
}

fn database(state: mixengine_platform::DatabaseState) -> BrowserDatabase {
    BrowserDatabase {
        path: state.path,
        owner: state.owner,
        installed: state.installed,
        because: state.because,
    }
}

/// A store, in words a person can go and look in.
fn store(method: mixengine_platform::TrustStoreMethod) -> String {
    use mixengine_platform::TrustStoreMethod as Method;

    match method {
        Method::SystemRoot => "this machine's Trusted Root Certification Authorities",
        Method::SystemKeychain => "/Library/Keychains/System.keychain",
        Method::CaCertificates => "/usr/local/share/ca-certificates",
        Method::CaTrustAnchors => "/etc/pki/ca-trust/source/anchors",
        Method::None => "this machine has no system trust store MixEngine knows how to write",
    }
    .to_owned()
}

/// Run `work` off the runtime, and turn a task that did not finish into a sentence.
async fn blocking<T: Send + 'static>(
    what: &'static str,
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, Error> {
    tokio::task::spawn_blocking(work).await.map_err(|_| {
        Error::new(
            ErrorCode::Internal,
            format!("the task {what} this home's certificate authority did not finish"),
        )
    })
}

#[cfg(test)]
mod tests;
