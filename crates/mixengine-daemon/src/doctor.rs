//! `mix doctor`, which reports and does not repair — roadmap task **T47a**.
//!
//! **Nothing here writes.** No row, no file, nothing enqueued, and no elevation prompt can result
//! from a call — which is what makes it safe to run on a timer, inside `mix status`, and inside
//! T93's bundle. Repairing what it finds is `daemon.doctor_repair`, and that is T47b's.
//!
//! Every check is assembled from the reader its subsystem already owns — the hosts block from T41's
//! comparison, the resolver from T45's probe, the domains from T46's report — rather than from a
//! second opinion. Two implementations of one question are two answers to it.
//!
//! # Three outcomes that are not failures
//!
//! [`Outcome::Ok`] is the ordinary one. [`Outcome::Note`] is a fact worth stating that nobody can
//! act on — `hosts_only` is a supported mode and macOS genuinely makes no promise about a killed
//! daemon's descendants — and reporting either as a fault would put a permanent problem on a
//! correctly working machine. [`Outcome::Skipped`] is a check that could not run and says why, which
//! is the difference between "there is nothing wrong here" and "nobody looked".

use std::sync::Arc;

use mixengine_proto::{Check, DoctorReport, Outcome, ProblemId};

pub(crate) mod hosts;
mod installs;
mod trust;

/// The `daemon.doctor` half of the API.
#[derive(Debug)]
pub(crate) struct Doctor {
    /// The rows, for the hosts block this home's sites need.
    store: mixengine_core::Store,

    /// The server, and which TLDs this machine routes here — T44 and T45.
    dns: Arc<crate::dns::Dns>,

    /// This machine: its hosts file, its resolver, its permissions, its reserved ranges.
    host: Arc<dyn mixengine_platform::Host>,

    /// The queue, for what is waiting on a person.
    elevation: Arc<crate::elevation::Elevation>,

    /// The front end's program path, which is what the port-access probe is about.
    services: Arc<crate::services::Registry>,

    /// T46's report, rendered rather than recomputed.
    domains: Arc<crate::domains::Domains>,

    /// The home's own directory, for the permissions check.
    root: std::path::PathBuf,

    /// Where this home's authority lives, for the trust-store check — T49a.
    certs: std::path::PathBuf,

    /// Where the generated trust bundle lives, for the check that reads it — T132.
    etc: std::path::PathBuf,

    /// `<root>/bin`, for the check that asks what else on this PATH answers to its names — T131.
    bin: std::path::PathBuf,

    /// `<root>/runtimes`, for the check that asks whether `JAVA_HOME` names one of this home's JDKs
    /// — roadmap task **T27e**.
    runtimes: std::path::PathBuf,

    /// The whole layout, for the one check that walks both `runtimes/` and `packages/` — T182f.
    layout: mixengine_core::Paths,

    /// What these rows render to, for the drift check — the registry's own generator.
    generator: mixengine_core::generate::Generator,

    /// This home's crash reports, for the eighteenth check — roadmap task **T91**.
    crashes: crate::crash::Reports,
}

impl Doctor {
    /// The one of these the API holds.
    pub(crate) fn new(
        store: &mixengine_core::Store,
        dns: Arc<crate::dns::Dns>,
        elevation: Arc<crate::elevation::Elevation>,
        services: Arc<crate::services::Registry>,
        domains: Arc<crate::domains::Domains>,
        paths: &mixengine_core::Paths,
        crashes: crate::crash::Reports,
    ) -> Arc<Self> {
        // **Taken from the queue rather than passed beside it** — the same reasoning as the
        // generator below, one argument along. Every caller was already handing over
        // `elevation.host()`, so the machine and the queue could only ever have disagreed by
        // somebody's mistake; and a seventh argument here is the last one clippy allows, which T91
        // is what discovered.
        let host = elevation.host();

        // **The home's layout rather than its root and its generator separately.** Both are derived
        // from it, and passing them apart let a caller hand this a generator built from one home and
        // a root from another — a check comparing a rendering the registry would never have written.
        let generator = crate::services::generator(paths, store, host.as_ref(), services.welcome());

        Arc::new(Self {
            store: store.clone(),
            dns,
            host,
            elevation,
            services,
            domains,
            root: paths.root().to_path_buf(),
            certs: paths.certs().to_path_buf(),
            etc: paths.etc().to_path_buf(),
            bin: paths.bin().to_path_buf(),
            runtimes: paths.runtimes().to_path_buf(),
            layout: paths.clone(),
            generator,
            crashes,
        })
    }

    /// Examine everything, in a fixed order.
    ///
    /// **Every check appears, whatever it answered.** A check that found nothing wrong is the
    /// evidence that it ran, and a shorter list on one system would read as a clean bill of health
    /// rather than as a question nobody asked.
    pub(crate) async fn report(&self) -> DoctorReport {
        DoctorReport {
            checks: vec![
                self.hosts_block().await,
                self.orphaned_hosts_blocks(),
                self.resolver(),
                self.trust_store(),
                self.browsers(),
                self.site_certificates().await,
                self.trust_bundle(),
                self.go_toolchain().await,
                self.java_pin().await,
                self.java_trust().await,
                self.commands().await,
                self.unrecorded().await,
                self.missing_installs().await,
                self.dns_server(),
                self.port_access().await,
                self.pending_permissions().await,
                self.domains().await,
                self.home_permissions(),
                self.descendants(),
                self.resource_limits(),
                self.reserved_ports(),
                self.application_control(),
                self.foreign_firewall_rules(),
                self.generated_config().await,
                self.unsupervised().await,
                crate::crash::check(&self.crashes),
            ],
        }
    }

    /// **1.** Compared the way `Elevation::require_hosts` compares it — as operations rather than as
    /// lists, so the ordering and the deduplication are `hosts_apply`'s in both places and there is
    /// one definition of "the same block".
    async fn hosts_block(&self) -> Check {
        let name = "the managed hosts block".to_owned();

        let Ok(desired) = mixengine_core::hosts::desired(&self.store, &self.dns.wired()).await
        else {
            return Check {
                name,
                outcome: Outcome::Skipped {
                    because: "this home's sites could not be read".to_owned(),
                },
            };
        };

        // this home's own block, under the tag the helper derives from its path.
        let tag = mixengine_platform::hosts::tag_for(&self.root);
        let wanted = mixengine_proto::privileged::PrivilegedOp::hosts_apply(
            Some(tag.clone()),
            desired.clone(),
        );

        match self.host.hosts_file().managed(Some(&tag)) {
            // `present.clone()` and not `present`: a pattern guard may not move out of what it
            // matched, which is why `Elevation::require_hosts` writes it the same way one file over.
            Ok(present)
                if mixengine_proto::privileged::PrivilegedOp::hosts_apply(
                    Some(tag.clone()),
                    present.clone(),
                ) == wanted =>
            {
                Check {
                    name,
                    outcome: Outcome::Ok {},
                }
            }
            Ok(present) => Check {
                name,
                outcome: Outcome::Problem {
                    id: ProblemId::HostsBlockDiffers,
                    because: hosts::differs(&desired, &present),
                },
            },
            Err(error) => Check {
                name,
                outcome: Outcome::Skipped {
                    because: format!("the hosts file could not be read: {error}"),
                },
            },
        }
    }

    /// **2.** A port the operating system chose is a port nothing may be wired to — T45 — so that is
    /// a `Note` and not a fault. Every test home in this workspace is on one.
    fn resolver(&self) -> Check {
        let name = "the resolver on this machine".to_owned();

        let Some(port) = self.dns.wirable_port() else {
            return Check {
                name,
                outcome: Outcome::Note {
                    because: "this home's DNS port is chosen by the operating system, so no \
                              resolver may be pointed at it"
                        .to_owned(),
                },
            };
        };

        let want: Vec<&str> = mixengine_proto::domains::WIRED_TLDS.to_vec();

        match self.host.resolver().probe(&want, port) {
            Ok(state) if state.wired.len() == want.len() => Check {
                name,
                outcome: Outcome::Ok {},
            },
            Ok(state) => Check {
                name,
                outcome: Outcome::Problem {
                    id: ProblemId::ResolverNotWired,
                    because: state.missing.unwrap_or_else(|| {
                        "nothing on this machine sends a managed TLD to this daemon".to_owned()
                    }),
                },
            },
            Err(error) => Check {
                name,
                outcome: Outcome::Skipped {
                    because: format!("this machine's resolver could not be read: {error}"),
                },
            },
        }
    }

    /// **3.** `hosts_only` is a **mode T46a closed as supported**, not a degradation — so it is a
    /// `Note`. Only a bind that failed is a problem, and that distinction is the whole of this
    /// check: calling the supported mode a fault would put a permanent problem on every machine
    /// that never wired a resolver.
    fn dns_server(&self) -> Check {
        let name = "the built-in DNS server".to_owned();
        let status = self.dns.status();

        match (status.listening, status.because) {
            (Some(_), _) => Check {
                name,
                outcome: Outcome::Ok {},
            },
            (None, Some(because)) => Check {
                name,
                outcome: Outcome::Problem {
                    id: ProblemId::DnsServerUnavailable,
                    because,
                },
            },
            (None, None) => Check {
                name,
                outcome: Outcome::Note {
                    because: "the DNS server is switched off in config.toml".to_owned(),
                },
            },
        }
    }

    /// **4.** A home with no front end has nothing that needs to answer on 80 or 443.
    async fn port_access(&self) -> Check {
        let name = "answering on 80 and 443".to_owned();

        let Some(binary) = self.services.front_end_program().await else {
            return Check {
                name,
                outcome: Outcome::Note {
                    because: "this home has no front end, so nothing needs those ports".to_owned(),
                },
            };
        };

        match self
            .host
            .port_access()
            .probe(&binary, &crate::elevation::Elevation::ANSWERING)
        {
            Ok(state) if state.granted => Check {
                name,
                outcome: Outcome::Ok {},
            },
            Ok(state) => Check {
                name,
                outcome: Outcome::Problem {
                    id: ProblemId::PortAccessMissing,
                    because: state
                        .missing
                        .unwrap_or_else(|| "this machine has not granted it".to_owned()),
                },
            },
            Err(error) => Check {
                name,
                outcome: Outcome::Skipped {
                    because: format!("this machine could not be asked: {error}"),
                },
            },
        }
    }

    /// **5.** Something waiting on a person is not a fault of the machine, but it *is* something
    /// this home asked for and has not got — and T47b's flush is exactly the repair for it.
    ///
    /// Read through `elevation.status`, which is what the client screen T64 built reads, so "what is
    /// waiting" has one definition rather than two that have to agree.
    async fn pending_permissions(&self) -> Check {
        let name = "operations waiting for permission".to_owned();

        let waiting = match self.elevation.status().await {
            Ok(status) => status.pending,
            Err(error) => {
                return Check {
                    name,
                    outcome: Outcome::Skipped {
                        because: format!("the queue could not be read: {error}"),
                    },
                };
            }
        };

        if waiting.is_empty() {
            return Check {
                name,
                outcome: Outcome::Ok {},
            };
        }

        Check {
            name,
            outcome: Outcome::Problem {
                id: ProblemId::PermissionPending,
                because: format!(
                    "{} operation(s) are waiting to be granted or dropped",
                    waiting.len()
                ),
            },
        }
    }

    /// **6.** T46's report, rendered — **never recomputed**. T46's own roadmap entry asks for that
    /// in as many words, and the reason is this whole module's: two implementations of one question
    /// are two answers to it.
    ///
    /// **One `Problem` naming every unreachable domain**, rather than one per domain. A home with
    /// twelve names that do not resolve has one fault with one cause, and twelve rows would bury the
    /// eight checks around them.
    async fn domains(&self) -> Check {
        let name = "every domain this home declares".to_owned();

        let Ok(report) = self
            .domains
            .status(&mixengine_proto::DomainStatusQuery { domain: None })
            .await
        else {
            return Check {
                name,
                outcome: Outcome::Skipped {
                    because: "this home's domains could not be read".to_owned(),
                },
            };
        };

        let unreachable: Vec<&str> = report
            .domains
            .iter()
            .filter(|row| row.resolves_to.is_empty())
            .map(|row| row.domain.as_str())
            .collect();

        if unreachable.is_empty() {
            return Check {
                name,
                outcome: Outcome::Ok {},
            };
        }

        Check {
            name,
            outcome: Outcome::Problem {
                id: ProblemId::DomainUnreachable,
                because: format!("this machine does not resolve {}", unreachable.join(", ")),
            },
        }
    }

    /// **7.** `is_restricted_to_owner`'s first caller — and what settles the `icacls` question T3a
    /// left open, since the whole of what this needs is "inheritance is intact, yes or no".
    fn home_permissions(&self) -> Check {
        let name = "the home is readable only by its owner".to_owned();

        match self
            .host
            .directory_access()
            .is_restricted_to_owner(&self.root)
        {
            Ok(true) => Check {
                name,
                outcome: Outcome::Ok {},
            },
            Ok(false) => Check {
                name,
                outcome: Outcome::Problem {
                    id: ProblemId::HomePermissionsLost,
                    because: "this home is no longer restricted to its owner, which a move onto \
                              another volume or a restore from a backup does silently"
                        .to_owned(),
                },
            },
            Err(error) => Check {
                name,
                outcome: Outcome::Skipped {
                    because: format!("this home's permissions could not be read: {error}"),
                },
            },
        }
    }

    /// **8.** Always a `Note`, on every system — the whole of the design's D4, and
    /// [ADR 0007](../../../docs/decisions/0007-supervised-child-owns-a-process-group.md)'s own
    /// table read out loud.
    fn descendants(&self) -> Check {
        Check {
            name: "what this system promises about a service's descendants".to_owned(),
            outcome: Outcome::Note {
                because: mixengine_platform::orphan_guarantee().because().to_owned(),
            },
        }
    }

    /// **A `Note`, never a `Problem`** — roadmap task **T68**, and this is what T47a's distinction
    /// was built for.
    ///
    /// A machine whose session was started without a delegated `cpu` controller is not a broken
    /// machine, and reporting it as a fault would report the way somebody logs in as broken. But
    /// saying nothing is worse: a person who set a memory ceiling and watched a service sail past it
    /// has no other way to find out why.
    ///
    /// **[`Unsupported`](mixengine_platform::Enforcement::Unsupported) says nothing at all**, which is the other half of the same
    /// judgement. macOS having no hard memory cap is a permanent fact about an operating system, not
    /// news about this machine — and a report that repeated it on every run would teach people to
    /// skip the report.
    ///
    /// It gains T47b no repair arm, and could not: there is nothing here MixEngine may fix without
    /// asking a person to change how their session is started. That is why it is not a
    /// [`ProblemId`], whose `match` in `daemon.doctor_repair` has no
    /// wildcard on purpose.
    fn resource_limits(&self) -> Check {
        Check {
            name: "what this machine will enforce of a service's limits".to_owned(),
            outcome: limit_outcome(&self.host.resource_control().support()),
        }
    }

    /// **16.** A `Note` for an inbound firewall rule this machine holds for MixEngine's own daemon
    /// that MixEngine did not write — roadmap task **T76**, the design's D8.
    ///
    /// **The rule is real, it is wide, and it is not ours.** Binding UDP 5353 to advertise a shared
    /// site makes Windows raise its own dialog, and Allow writes an every-port TCP-and-UDP rule for
    /// `mixengined.exe` on the Private *and* Public profiles — far wider than the web ports a shared
    /// site needs, created outside `mixengine-elevate`, and not removed by `site.unshare`, because
    /// MixEngine never made it and does not delete what it did not make. What this build does about
    /// it is say that it is there.
    ///
    /// MixEngine's own rules are scoped by port and carry no program, so nothing here can count one
    /// of ours by mistake.
    fn foreign_firewall_rules(&self) -> Check {
        let name = "firewall rules this machine holds for MixEngine's own daemon".to_owned();

        let program = match std::env::current_exe() {
            Ok(program) => program,
            Err(error) => {
                return Check {
                    name,
                    outcome: Outcome::Skipped {
                        because: format!("this daemon's own path could not be read: {error}"),
                    },
                };
            }
        };

        Check {
            name,
            outcome: foreign_rule_outcome(self.host.firewall_rules().naming(&program), &program),
        }
    }

    /// **9.** A `Problem` **only where a reserved range holds a port this home actually needs** —
    /// the ranges are the operating system's business until they collide with ours.
    ///
    /// This is the one check that saves a person from a wrong search rather than telling them
    /// something they could have found: a bind into a reserved range fails with an access error, so
    /// it reads as a permission problem, and elevation, UAC and the firewall are all the wrong place
    /// to look.
    fn reserved_ports(&self) -> Check {
        let name = "ports this system has reserved".to_owned();

        let ranges = match self.host.reserved_ports().reserved() {
            Ok(ranges) => ranges,
            Err(error) => {
                return Check {
                    name,
                    outcome: Outcome::Skipped {
                        because: error.to_string(),
                    },
                };
            }
        };

        let taken: Vec<u16> = [80, 443]
            .into_iter()
            .chain(self.dns.port())
            .filter(|port| ranges.iter().any(|range| range.holds(*port)))
            .collect();

        if taken.is_empty() {
            return Check {
                name,
                outcome: if ranges.is_empty() {
                    Outcome::Ok {}
                } else {
                    Outcome::Note {
                        because: format!(
                            "{} port range(s) are reserved on this system, and none holds a port \
                             this home needs",
                            ranges.len()
                        ),
                    }
                },
            };
        }

        Check {
            name,
            outcome: Outcome::Problem {
                id: ProblemId::PortRangeReserved,
                because: format!(
                    "this system has reserved {}, so binding it fails with an error that reads \
                     like a permission problem and is not one",
                    taken
                        .iter()
                        .map(u16::to_string)
                        .collect::<Vec<_>>()
                        .join(" and ")
                ),
            },
        }
    }

    /// **17.** What this machine will refuse to load, which is not a fault of anything MixEngine
    /// installed — roadmap task **T94**.
    ///
    /// **A check about the machine MixEngine *can* run on, and it says nothing about the one it
    /// cannot.** On a machine that refused `mixengined.exe` itself there is nothing here to ask,
    /// and the only record is Windows' own `Microsoft-Windows-CodeIntegrity/Operational` log. What
    /// this covers is the reachable middle, and that middle is real because the judgement is per
    /// *file*: a daemon that loaded and a runtime downloaded five minutes ago are two separate
    /// verdicts.
    ///
    /// **It names Smart App Control because that is what it read.** An enterprise WDAC policy
    /// refuses image loads through the same subsystem while this value reads `Off`, which is why
    /// the sentence attached to a refused *load* — `APP_CONTROL_REFUSAL` — says "an application
    /// control policy" and never names the feature (T94 design, D3).
    fn application_control(&self) -> Check {
        Check {
            name: "this machine's application control policy".to_owned(),
            outcome: app_control_outcome(self.host.app_control().state()),
        }
    }

    /// **10.** Is what is installed what these rows render to?
    ///
    /// **Rendered again and compared, never parsed back** — the workspace rule, and the reason this
    /// check could not be built in T47a: answering the question *is* the first half of the repair.
    /// [`Generator::drift`](mixengine_core::generate::Generator::drift) builds the rendering in
    /// memory and installs nothing, so this stays a read.
    ///
    /// **One `Problem` naming every service that drifted**, on the domains check's reasoning: a home
    /// whose rows all moved has one fault with one cause, and a row per service would bury the ten
    /// checks around them.
    ///
    /// What this is a fault *about* is worth being exact on, because generated configuration is
    /// disposable and the next write corrects it anyway: the fault is that a service is running on a
    /// rendering nothing in this home asked for, right now.
    async fn generated_config(&self) -> Check {
        let name = "the generated configuration".to_owned();

        let drifts = match self.generator.drift().await {
            Ok(drifts) => drifts,
            Err(error) => {
                return Check {
                    name,
                    outcome: Outcome::Skipped {
                        because: format!(
                            "what these rows render to could not be worked out: {error}"
                        ),
                    },
                };
            }
        };

        let stale: Vec<&str> = drifts
            .iter()
            .filter(|one| !one.drift.is_empty())
            .map(|one| one.service.as_str())
            .collect();

        if stale.is_empty() {
            return Check {
                name,
                outcome: Outcome::Ok {},
            };
        }

        Check {
            name,
            outcome: Outcome::Problem {
                id: ProblemId::GeneratedConfigStale,
                because: format!(
                    "what is installed for {} is not what its row renders to, so what is being \
                     served is not what this home says",
                    stale.join(", ")
                ),
            },
        }
    }

    /// **11.** A row that claims a supervisor this daemon does not have.
    ///
    /// **Narrow on purpose, and [`Registry::recover`](crate::services::Registry::recover) is why.**
    /// That function answers the same question at every boot by walking *every* row, which is right
    /// when nothing is supervised yet and wrong on a running daemon: it would stop services that are
    /// working. The rows a live daemon may reconcile are the ones it holds no runner for, and that is
    /// exactly this set.
    async fn unsupervised(&self) -> Check {
        let name = "every service this daemon is supervising".to_owned();

        let records = match mixengine_core::services::records(&self.store).await {
            Ok(records) => records,
            Err(error) => {
                return Check {
                    name,
                    outcome: Outcome::Skipped {
                        because: format!("the services could not be read: {error}"),
                    },
                };
            }
        };

        // `records` is keyed by the id as a `String` and `supervised` answers `ServiceId`s. Compared
        // as strings rather than by parsing, because a row whose id no longer parses is still a row
        // this daemon is not supervising.
        let supervised = self.services.supervised();
        let held: std::collections::BTreeSet<&str> = supervised
            .iter()
            .map(mixengine_proto::ServiceId::as_str)
            .collect();

        let stranded = stranded(
            records.iter().map(|(stored, record)| {
                (
                    stored.as_str(),
                    record.state.is_supervised() || record.pid.is_some(),
                )
            }),
            &held,
        );

        if stranded.is_empty() {
            return Check {
                name,
                outcome: Outcome::Ok {},
            };
        }

        Check {
            name,
            outcome: Outcome::Problem {
                id: ProblemId::ServiceUnsupervised,
                because: format!(
                    "{} claim(s) a supervisor this daemon does not have, so a port and a data \
                     directory are held by something nothing is watching",
                    stranded.len()
                ),
            },
        }
    }
}

fn stranded<'a>(
    rows: impl Iterator<Item = (&'a str, bool)>,
    held: &std::collections::BTreeSet<&str>,
) -> Vec<String> {
    rows.filter(|(id, claims)| *claims && !held.contains(id))
        .map(|(id, _)| id.to_owned())
        .collect()
}

/// What [`Doctor::resource_limits`] decides, as a function of the answer alone.
///
/// Out here rather than inside the method so that a test can drive every arm with a `LimitSupport`
/// it wrote, instead of assembling a whole `Doctor` to ask one question. The reasoning is on the
/// method.
fn limit_outcome(support: &mixengine_platform::LimitSupport) -> Outcome {
    use mixengine_platform::Enforcement;

    let unavailable: Vec<String> = [("cpu", &support.cpu), ("memory", &support.memory)]
        .into_iter()
        .filter_map(|(field, enforcement)| match enforcement {
            Enforcement::Unavailable { why } => Some(format!("{field}: {why}")),

            // **A watchdog with a `why` is still a machine somebody could fix** — roadmap task
            // T71a. The field does something here, so this is not a problem; it is the same Note
            // `Unavailable` earns, because the sentence names the same fixable thing. A `why` of
            // `None` is an operating system with nothing to fix, and gets `Unsupported`'s silence.
            Enforcement::Advisory { why: Some(why) } => Some(format!("{field}: {why}")),

            // `Hard` needs no comment, and `Unsupported` deliberately gets none — see the method.
            Enforcement::Hard { .. }
            | Enforcement::Unsupported
            | Enforcement::Advisory { why: None } => None,

            // A variant added after this daemon was written cannot be described, and a doctor that
            // invented a sentence for it would be reporting its own ignorance as the machine's.
            _ => None,
        })
        .collect();

    if unavailable.is_empty() {
        return Outcome::Ok {};
    }

    Outcome::Note {
        because: unavailable.join("; "),
    }
}

/// What a rule count means, and what to tell somebody about it.
///
/// **Never a `Problem`, and the variant is the decision.** A `Problem` carries a `ProblemId`, and a
/// `ProblemId` is what `daemon.doctor_repair` matches on — so giving this condition one would be
/// offering to delete a firewall rule that MixEngine did not create and that somebody personally
/// clicked Allow on. The condition is deliberately given no identity a repair could key off, and
/// what it carries instead is the sentence and the command to run by hand.
///
/// A free function so the mapping can be tested against an answer built by hand, with no host, no
/// daemon and no firewall — the shape [`limit_outcome`] established.
fn foreign_rule_outcome(
    naming: mixengine_platform::Result<Option<usize>>,
    program: &std::path::Path,
) -> Outcome {
    let program = program.display();

    match naming {
        // Not `Ok {}`: zero would be the claim *this machine holds no such rule*, and on a system
        // with no such table that is a statement about something that does not exist.
        Ok(None) => Outcome::Skipped {
            because: "this system has no per-program inbound firewall rule to enumerate".to_owned(),
        },

        Ok(Some(0)) => Outcome::Ok {},

        Ok(Some(count)) => Outcome::Note {
            because: format!(
                "this machine holds {count} inbound firewall rule(s) for {program}, and MixEngine \
                 did not create any of them; most likely the one Windows offered when this daemon \
                 began answering mDNS for a shared site. MixEngine opens the web ports a shared \
                 site needs and nothing else, so a rule for the program as a whole is wider than \
                 sharing ever asks for. To see them: `netsh advfirewall firewall show rule \
                 name=all dir=in verbose`. To remove them, from an administrator prompt: `netsh \
                 advfirewall firewall delete rule name=all program=\"{program}\"`"
            ),
        },

        // A tool that would not run is a question nobody answered, not a machine with no rules.
        Err(error) => Outcome::Skipped {
            because: mixengine_proto::flatten(&error),
        },
    }
}

/// What this machine's application control policy means for a product nothing signs.
///
/// A free function so every arm is reachable from a value written by hand, with no host and no
/// registry — the shape [`limit_outcome`] and [`foreign_rule_outcome`] established. The reasoning
/// is on [`Doctor::application_control`].
fn app_control_outcome(
    state: mixengine_platform::Result<mixengine_platform::AppControlState>,
) -> Outcome {
    use mixengine_platform::AppControlState;

    match state {
        Ok(AppControlState::Off) => Outcome::Ok {},

        Ok(AppControlState::Evaluation) => Outcome::Note {
            because: "Smart App Control is evaluating this machine and has not decided yet; if it \
                      begins enforcing, the unsigned programs MixEngine starts will be refused at \
                      load"
                .to_owned(),
        },

        Ok(AppControlState::Enforced) => Outcome::Problem {
            id: ProblemId::ApplicationControlEnforced,
            because: "Smart App Control is enforcing on this machine, and every program MixEngine \
                      starts is unsigned; a runtime it has just downloaded is exactly the \
                      first-seen file such a policy refuses to load"
                .to_owned(),
        },

        // Not one of the three: a build that guessed which named state a number resembled would be
        // reporting its own ignorance as this machine's verdict.
        Ok(AppControlState::Unknown { value }) => Outcome::Skipped {
            because: format!(
                "this machine's Smart App Control policy state read as {value}, which this build \
                 has no name for"
            ),
        },

        // macOS and Linux, and a Windows that would not answer. A check that ran and says why it
        // had nothing to examine.
        Err(error) => Outcome::Skipped {
            because: mixengine_proto::flatten(&error),
        },
    }
}

#[cfg(test)]
mod tests;
