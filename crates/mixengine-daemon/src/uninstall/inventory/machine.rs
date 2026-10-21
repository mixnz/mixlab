//! The rows for what this home changed on the machine: the hosts file, the resolver, port access,
//! the firewall, certificates and browsers.

use mixengine_proto::privileged::{FirewallPlan, PrivilegedOp};
use mixengine_proto::{Removal, Residue, ResidueId};

use super::super::Uninstall;

use super::{firewall_label, resolver_place, trust_place};

/// **1.** The block in this machine's hosts file, read the way T41 reads it.
pub(super) fn hosts_block(uninstall: &Uninstall) -> Residue {
    let file = uninstall.host.hosts_file();
    let location = file.path().display().to_string();

    let outcome = match file.managed(None) {
        // An empty block is a machine that has never had one, which is not a failure — the trait's
        // own words.
        Ok(entries) if entries.is_empty() => Removal::Absent {},
        Ok(entries) => Removal::Planned {
            how: PrivilegedOp::hosts_apply(None, Vec::new()).describe()
                + &format!(", which today holds {} name(s)", entries.len()),
        },
        Err(error) => Removal::Failed {
            because: format!(
                "this machine's hosts file could not be read, so what is in it is unknown: {}",
                mixengine_proto::flatten(&error)
            ),
        },
    };

    Residue {
        id: ResidueId::HostsBlock,
        what: "the managed hosts block".to_owned(),
        location,
        outcome,
    }
}

/// **2.** Whatever sends a managed TLD to this daemon's own DNS server — T45's probe.
///
/// **Asked about the port the server is on, or about nothing.** A home whose DNS server never bound
/// has no wiring of its own to find, and a probe against a port nothing listens on would be asking
/// about somebody else's.
pub(super) fn resolver_wiring(uninstall: &Uninstall) -> Residue {
    let what = "the DNS routing for this home's managed names".to_owned();

    let Some(port) = uninstall.dns.wirable_port() else {
        return Residue {
            id: ResidueId::ResolverWiring,
            what,
            location: "no DNS server of this home's is listening, so nothing can be routed to one"
                .to_owned(),
            outcome: Removal::Absent {},
        };
    };

    let want: Vec<&str> = mixengine_proto::domains::WIRED_TLDS.to_vec();

    let state = match uninstall.host.resolver().probe(&want, port) {
        Ok(state) => state,
        Err(error) => {
            return Residue {
                id: ResidueId::ResolverWiring,
                what,
                location: "this machine's resolver configuration".to_owned(),
                outcome: Removal::Failed {
                    because: format!(
                        "this machine's resolver could not be read, so what it routes is unknown: \
                         {}",
                        mixengine_proto::flatten(&error)
                    ),
                },
            };
        }
    };

    let location = resolver_place(state.method).to_owned();

    // **`wired` and not `method`.** A machine with a mechanism and nothing routed through it has
    // nothing of ours to remove, and asking to revoke there would spend a prompt on an operation
    // whose only outcome is `AlreadyDone` — T41's D11, one capability along.
    let outcome = match (state.wired.is_empty(), state.target()) {
        (false, Some(target)) => Removal::Planned {
            how: PrivilegedOp::ResolverRevoke { target }.describe(),
        },
        _ => Removal::Absent {},
    };

    Residue {
        id: ResidueId::ResolverWiring,
        what,
        location,
        outcome,
    }
}

/// **3.** The capability, or the packet-filter redirect with its anchor and its boot-time job.
pub(super) async fn port_access(uninstall: &Uninstall) -> Residue {
    let what = "permission for this home's front end to answer on 80 and 443".to_owned();

    let Some(binary) = uninstall.services.front_end_program().await else {
        return Residue {
            id: ResidueId::PortAccess,
            what,
            location: "this home has no front end, so nothing was granted for one".to_owned(),
            outcome: Removal::Absent {},
        };
    };

    let state = match uninstall
        .host
        .port_access()
        .probe(&binary, &crate::elevation::Elevation::ANSWERING)
    {
        Ok(state) => state,
        Err(error) => {
            return Residue {
                id: ResidueId::PortAccess,
                what,
                location: binary.display().to_string(),
                outcome: Removal::Failed {
                    because: format!(
                        "this machine could not be asked what it has granted: {}",
                        mixengine_proto::flatten(&error)
                    ),
                },
            };
        }
    };

    let location = match state.method {
        mixengine_platform::PortAccessMethod::Capability => binary.display().to_string(),
        mixengine_platform::PortAccessMethod::Redirect => {
            "this machine's packet filter: its anchor, its block in /etc/pf.conf and the boot-time \
             job that enables it"
                .to_owned()
        }
        mixengine_platform::PortAccessMethod::Direct => {
            "this system reserves no port below 1024, so nothing was ever granted".to_owned()
        }
    };

    // **`granted` and not `method`**, on the resolver row's reasoning: a system that grants nothing
    // and a system that has not granted it both have nothing of ours to take away.
    let outcome = match (state.granted, state.target(&binary)) {
        (true, Some(target)) => Removal::Planned {
            how: PrivilegedOp::PortAccessRevoke { target }.describe(),
        },
        _ => Removal::Absent {},
    };

    Residue {
        id: ResidueId::PortAccess,
        what,
        location,
        outcome,
    }
}

/// **4.** The inbound rules a shared site needed — T74's, and the one row read from the rows.
///
/// **Read from this home's own sites rather than from the machine.** `FirewallRules` on the `Host`
/// answers what rules exist under *any* name, which is what `mix doctor` uses to report the
/// every-port rule Windows writes for `mixengined.exe` — a rule MixEngine did not make and must not
/// remove. What this home wrote is what this home shared, and the plan is whole-state either way:
/// the empty plan *is* the revoke.
pub(super) async fn firewall_rules(uninstall: &Uninstall) -> Residue {
    let what = "the firewall rules a shared site needed".to_owned();
    let location = firewall_label();

    let outcome = match mixengine_core::sites::records(&uninstall.store, None).await {
        Ok(records) if records.iter().any(|record| record.sharing.is_some()) => Removal::Planned {
            how: PrivilegedOp::FirewallApply {
                plan: FirewallPlan {
                    ports: Vec::new(),
                    label: firewall_label(),
                },
            }
            .describe(),
        },
        Ok(_) => Removal::Absent {},
        Err(error) => Removal::Failed {
            because: format!(
                "this home's sites could not be read, so whether anything is shared is unknown: {}",
                mixengine_proto::flatten(&error)
            ),
        },
    };

    Residue {
        id: ResidueId::FirewallRules,
        what,
        location,
        outcome,
    }
}

/// **5 and 6.** The authority, in this machine's own store and in its browsers.
///
/// One function for two rows because they share one reading — the authority on disk — and taking it
/// twice would be two answers to *what is this home's certificate a moment apart*. They are separate
/// rows because they are separate stores, repaired and removed by different mechanisms: one needs a
/// token and the other does not, which is the line T49 was split on.
pub(super) async fn certificate_rows(uninstall: &Uninstall) -> (Residue, Residue) {
    let trust_what = "MixEngine's certificate authority in this machine's trust store".to_owned();
    let browser_what = "the same authority in this machine's browsers".to_owned();

    let state =
        mixengine_core::certs::ca::read(uninstall.paths.certs(), std::time::SystemTime::now());

    let der = match &state {
        mixengine_proto::CaState::Present { ca } => {
            mixengine_core::certs::ca::der(&ca.certificate_pem).map(|der| (der, ca.key_id.clone()))
        }
        mixengine_proto::CaState::Absent {} | mixengine_proto::CaState::Unusable { .. } => None,
    };

    let Some((der, key_id)) = der else {
        let because =
            "this home has no usable certificate authority, so nothing that could be named is in \
             any store"
                .to_owned();

        return (
            Residue {
                id: ResidueId::TrustStore,
                what: trust_what,
                location: uninstall.paths.certs().display().to_string(),
                outcome: Removal::Absent {},
            },
            Residue {
                id: ResidueId::BrowserTrust,
                what: browser_what,
                location: because,
                outcome: Removal::Absent {},
            },
        );
    };

    let trust = match uninstall.host.trust_store().probe(&der) {
        Ok(probed) => Residue {
            id: ResidueId::TrustStore,
            what: trust_what,
            location: trust_place(probed.method).to_owned(),
            outcome: match (probed.installed, probed.target(&key_id)) {
                (true, Some(target)) => Removal::Planned {
                    how: PrivilegedOp::TrustCaRemove { target }.describe(),
                },
                _ => Removal::Absent {},
            },
        },
        Err(error) => Residue {
            id: ResidueId::TrustStore,
            what: trust_what,
            location: "this machine's trust store".to_owned(),
            outcome: Removal::Failed {
                because: format!(
                    "this machine's trust store could not be read, so whether it holds this \
                     authority is unknown: {}",
                    mixengine_proto::flatten(&error)
                ),
            },
        },
    };

    (trust, browser_row(uninstall, browser_what, der).await)
}

/// **6.** What Firefox and Chrome hold, which is a different question from the store above.
///
/// A process spawn per profile, so off the runtime — `docs/standards/rust.md`, and the same
/// arrangement `Certificates::remove_from_browsers` uses for the write.
async fn browser_row(uninstall: &Uninstall, what: String, der: Vec<u8>) -> Residue {
    let host = uninstall.host.clone();

    let surveyed = tokio::task::spawn_blocking(move || host.browsers().survey(&der)).await;

    let (location, outcome) = match surveyed {
        Ok(Ok(mixengine_platform::BrowserSurvey::Reached { databases })) => {
            let holding: Vec<&mixengine_platform::DatabaseState> =
                databases.iter().filter(|one| one.installed).collect();

            match holding.is_empty() {
                true => (
                    match databases.is_empty() {
                        true => "no browser database was found on this machine".to_owned(),
                        false => "no browser database on this machine holds it".to_owned(),
                    },
                    Removal::Absent {},
                ),
                false => (
                    holding
                        .iter()
                        .map(|one| format!("{} ({})", one.path, one.owner))
                        .collect::<Vec<String>>()
                        .join(", "),
                    Removal::Planned {
                        how: format!(
                            "take MixEngine's authority out of {} browser database(s), which needs \
                             no administrator",
                            holding.len()
                        ),
                    },
                ),
            }
        }

        // Neither is a failure: one is a machine without `libnss3-tools` and the other is a system
        // MixEngine does not search at all. Both carry their own sentence, which is why this reads
        // it out rather than writing a second one.
        Ok(Ok(
            mixengine_platform::BrowserSurvey::NoTool { because }
            | mixengine_platform::BrowserSurvey::NotSearched { because },
        )) => (because, Removal::Absent {}),

        Ok(Err(error)) => (
            "this machine's browsers".to_owned(),
            Removal::Failed {
                because: format!(
                    "this machine's browsers could not be asked, so what they hold is unknown: {}",
                    mixengine_proto::flatten(&error)
                ),
            },
        ),

        Err(join) => (
            "this machine's browsers".to_owned(),
            Removal::Failed {
                because: format!("the task asking this machine's browsers did not finish: {join}"),
            },
        ),
    };

    Residue {
        id: ResidueId::BrowserTrust,
        what,
        location,
        outcome,
    }
}
