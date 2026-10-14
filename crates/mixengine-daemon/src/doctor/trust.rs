//! The checks about who trusts this home's authority: the OS trust store, the browsers
//! that keep their own, the certificates its sites are served with, the bundle generated
//! for programs that read one, and every JDK this home installed.

use mixengine_proto::{Check, Outcome, ProblemId};

use super::Doctor;

impl Doctor {
    /// **3.** Whether this machine trusts the authority T48 made — roadmap task **T49a**.
    ///
    /// **A home with no authority is skipped, not a problem.** There is nothing to trust, and the
    /// daemon already warned about the generation that failed; reporting it twice would put a second
    /// condition on the screen for one cause. A machine with no store MixEngine knows how to write
    /// is a `Note` rather than a problem, for the reason the resolver check gives about `hosts_only`:
    /// it is a supported mode and calling it a fault would put a permanent problem on every machine
    /// that will never have one.
    ///
    /// **This answers "is it in the store", not "does a browser trust it".** Firefox and Chrome on
    /// Linux read NSS and not this store at all (T49b), and the honest end-to-end check is a live
    /// handshake, which is T53's.
    pub(super) fn trust_store(&self) -> Check {
        let name = "this machine's trust in MixEngine's authority".to_owned();

        let der = match mixengine_core::certs::ca::read(&self.certs, std::time::SystemTime::now()) {
            mixengine_proto::CaState::Present { ca } => {
                mixengine_core::certs::ca::der(&ca.certificate_pem)
            }
            mixengine_proto::CaState::Absent {} | mixengine_proto::CaState::Unusable { .. } => None,
        };

        let Some(der) = der else {
            return Check {
                name,
                outcome: Outcome::Skipped {
                    because: "this home has no usable certificate authority, so there is nothing \
                              for this machine to trust; `mix cert ca-status` says which"
                        .to_owned(),
                },
            };
        };

        match self.host.trust_store().probe(&der) {
            Ok(state) if state.installed => Check {
                name,
                outcome: Outcome::Ok {},
            },
            Ok(state) if state.method == mixengine_platform::TrustStoreMethod::None => Check {
                name,
                outcome: Outcome::Note {
                    because: state.missing.unwrap_or_else(|| {
                        "this machine has no system trust store MixEngine knows how to write"
                            .to_owned()
                    }),
                },
            },
            Ok(state) => Check {
                name,
                outcome: Outcome::Problem {
                    id: ProblemId::CaNotTrusted,
                    because: state.missing.unwrap_or_else(|| {
                        "this machine does not hold MixEngine's certificate authority".to_owned()
                    }),
                },
            },
            Err(error) => Check {
                name,
                outcome: Outcome::Skipped {
                    because: format!("this machine's trust store could not be read: {error}"),
                },
            },
        }
    }

    /// **3b.** What Firefox and Chrome hold, which is a different question from the one above:
    /// they read NSS databases and not the system store at all.
    ///
    /// **No tool is a `Note` and not a problem**, on the reasoning the resolver's `hosts_only` arm
    /// states — a machine that will never run a browser would otherwise carry a permanent fault. So
    /// is a system MixEngine does not search. A machine with databases that simply lack it *is* a
    /// problem, and the reason names them.
    pub(super) fn browsers(&self) -> Check {
        let name = "MixEngine's authority in this machine's browsers".to_owned();

        let der = match mixengine_core::certs::ca::read(&self.certs, std::time::SystemTime::now()) {
            mixengine_proto::CaState::Present { ca } => {
                mixengine_core::certs::ca::der(&ca.certificate_pem)
            }
            mixengine_proto::CaState::Absent {} | mixengine_proto::CaState::Unusable { .. } => None,
        };

        let Some(der) = der else {
            return Check {
                name,
                outcome: Outcome::Skipped {
                    because: "this home has no usable certificate authority, so there is nothing \
                              for a browser to trust; `mix cert ca-status` says which"
                        .to_owned(),
                },
            };
        };

        let survey = match self.host.browsers().survey(&der) {
            Ok(survey) => survey,
            Err(error) => {
                return Check {
                    name,
                    outcome: Outcome::Skipped {
                        because: format!("this machine's browsers could not be asked: {error}"),
                    },
                };
            }
        };

        let lacking = survey.lacking();

        if !lacking.is_empty() {
            let because = lacking
                .iter()
                .map(|one| one.path.as_str())
                .collect::<Vec<_>>()
                .join(", ");

            return Check {
                name,
                outcome: Outcome::Problem {
                    id: ProblemId::BrowsersNotTrusted,
                    because: format!(
                        "these browser databases do not hold MixEngine's authority: {because}"
                    ),
                },
            };
        }

        match survey {
            // Nothing lacks it because nothing could be asked, which is a true thing to say and not
            // an `Ok` — the distinction this whole check turns on.
            mixengine_platform::BrowserSurvey::NoTool { because }
            | mixengine_platform::BrowserSurvey::NotSearched { because } => Check {
                name,
                outcome: Outcome::Note { because },
            },
            mixengine_platform::BrowserSurvey::Reached { .. } => Check {
                name,
                outcome: Outcome::Ok {},
            },
        }
    }

    /// **3c.** Whether every site that declares HTTPS has a certificate that still covers its
    /// names — roadmap task **T50**.
    ///
    /// **A check on disk and not a handshake.** Whether a browser accepts what the front end
    /// actually serves is a stronger claim and it is `mix cert status`' (T53); this one answers
    /// whether the file exists and matches the row, which is the question that catches the most
    /// common report — a domain added and a certificate not reissued.
    pub(super) async fn site_certificates(&self) -> Check {
        let name = "a certificate for every site that declares HTTPS".to_owned();

        let Ok(sites) = mixengine_core::sites::records(&self.store, None).await else {
            return Check {
                name,
                outcome: Outcome::Skipped {
                    because: "this home's sites could not be read".to_owned(),
                },
            };
        };

        let wanted: Vec<_> = sites
            .into_iter()
            .filter(|site| site.https_enabled && !site.domains.is_empty())
            .collect();

        if wanted.is_empty() {
            return Check {
                name,
                outcome: Outcome::Ok {},
            };
        }

        if !matches!(
            mixengine_core::certs::ca::read(&self.certs, std::time::SystemTime::now()),
            mixengine_proto::CaState::Present { .. }
        ) {
            return Check {
                name,
                outcome: Outcome::Skipped {
                    because: "this home has no usable certificate authority to sign with; \
                              `mix cert ca-status` says which"
                        .to_owned(),
                },
            };
        }

        let now = std::time::SystemTime::now();

        // **Three of `leaf::ensure`'s four questions and deliberately not the fourth.** A leaf
        // signed by an authority this home has since replaced is caught by `ensure` when the repair
        // below runs; asserting it here as well would be two copies of one rule to keep in step,
        // and the copy that drifted would report a machine as faulty for a certificate the repair
        // then declined to replace.
        let lacking: Vec<String> = wanted
            .iter()
            .filter(|site| {
                let primary = &site.domains[0];

                !matches!(
                    mixengine_core::certs::leaf::read(&self.certs, primary, now),
                    mixengine_proto::CertState::Present { ref cert }
                        if cert.sans == site.domains
                            && cert.days_left > mixengine_core::certs::leaf::RENEW_WITHIN_DAYS
                )
            })
            .map(|site| site.domains[0].clone())
            .collect();

        if lacking.is_empty() {
            Check {
                name,
                outcome: Outcome::Ok {},
            }
        } else {
            Check {
                name,
                outcome: Outcome::Problem {
                    id: ProblemId::SiteCertificateMissing,
                    because: format!(
                        "these sites have no certificate covering their names: {}",
                        lacking.join(", ")
                    ),
                },
            }
        }
    }

    /// **The bundle every runtime is pointed at** — roadmap task **T132**.
    ///
    /// Three answers, and the middle one is the reason this is not a single `is_file`:
    ///
    /// - a machine whose store **cannot be read** is a `Note`. There is nothing to repair, nothing
    ///   MixEngine did wrong, and nothing is exported — so the runtimes are exactly as they were
    ///   before this task existed, which is a working machine and not a faulty one.
    /// - a store that reads and a bundle that is missing is a **problem**, repaired by writing it.
    /// - a `SSL_CERT_FILE` the *daemon's own environment* already carries is a `Note`: a shim
    ///   inherits that variable and leaves it alone, so this home's authority never reaches the
    ///   runtime and the person has to be told which variable is winning.
    pub(super) fn trust_bundle(&self) -> Check {
        let name = "the trust bundle this home's runtimes read".to_owned();
        let bundle = mixengine_core::generate::ca::path(&self.etc);

        let shadowing: Vec<&str> = ["SSL_CERT_FILE", "REQUESTS_CA_BUNDLE", "NODE_EXTRA_CA_CERTS"]
            .into_iter()
            .filter(|variable| std::env::var_os(variable).is_some())
            .collect();

        if !shadowing.is_empty() {
            return Check {
                name,
                outcome: Outcome::Note {
                    because: format!(
                        "this daemon's own environment sets {}, and a command started through \
                         bin/ inherits it rather than MixEngine's own; unset it, or add this \
                         home's authority to the file it names",
                        shadowing.join(" and ")
                    ),
                },
            };
        }

        let roots = match self.host.trust_store().roots() {
            Ok(roots) => roots,
            Err(error) => {
                return Check {
                    name,
                    outcome: Outcome::Note {
                        because: format!(
                            "this machine's trusted roots could not be read, so no bundle was \
                             written and no runtime was told anything: {error}"
                        ),
                    },
                };
            }
        };

        if roots.len() < mixengine_core::generate::ca::ROOT_FLOOR {
            return Check {
                name,
                outcome: Outcome::Note {
                    because: format!(
                        "this machine's trust store answered {} roots, which is too few to \
                         believe, so no bundle was written",
                        roots.len()
                    ),
                },
            };
        }

        match bundle.is_file() {
            true => Check {
                name,
                outcome: Outcome::Ok {},
            },
            false => Check {
                name,
                outcome: Outcome::Problem {
                    id: ProblemId::TrustBundleMissing,
                    because: format!(
                        "{} is not there, so a Python, Ruby or PHP program started through bin/ \
                         cannot verify this home's own HTTPS sites",
                        bundle.display()
                    ),
                },
            },
        }
    }

    /// **Whether every JDK can verify this home's own sites** — roadmap task **T27e**, D11.
    ///
    /// One `keytool` per installed JDK, which reads and writes nothing; a home with no Java, or none
    /// with a usable authority, asks no process anything.
    pub(super) async fn java_trust(&self) -> Check {
        Check {
            name: "every JDK trusts this home's authority".to_owned(),
            outcome: java_trust_outcome(
                &crate::certs::jdks::lacking(&self.store, &self.certs).await,
            ),
        }
    }
}

/// What [`Doctor::java_trust`] decides from the JDKs that do not hold this home's authority.
fn java_trust_outcome(lacking: &[String]) -> Outcome {
    match lacking.is_empty() {
        true => Outcome::Ok {},
        false => Outcome::Problem {
            id: ProblemId::JavaTrustMissing,
            because: format!(
                "{} cannot verify this home's own HTTPS sites; `mix doctor --repair` writes the \
                 authority into each one, and a `cacerts` whose password was changed is the one \
                 case keytool refuses",
                lacking.join(", ")
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nothing lacking is the ordinary machine, and the one a home with no Java always is.
    #[test]
    fn every_jdk_holding_the_authority_is_ok() {
        let outcome = super::java_trust_outcome(&[]);

        assert!(matches!(outcome, Outcome::Ok {}), "{outcome:?}");
    }

    /// **A JDK that cannot verify this home's sites is a problem, and is named** — T27e, D11.
    #[test]
    fn a_jdk_lacking_the_authority_is_a_problem_naming_it() {
        let outcome = super::java_trust_outcome(&["java 21.0.12.1".to_owned()]);

        let Outcome::Problem { id, because } = outcome else {
            panic!("a JDK that cannot verify a local site is a problem: {outcome:?}");
        };

        assert_eq!(id, ProblemId::JavaTrustMissing);
        assert!(because.contains("java 21.0.12.1"), "{because}");
    }
}
