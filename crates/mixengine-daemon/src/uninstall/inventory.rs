//! What MixEngine has written on this machine — roadmap task **T87**.
//!
//! **One enumeration, and both methods call it.** `daemon.uninstall_plan` renders it;
//! `daemon.uninstall` renders it, acts, and calls it again to measure. Two enumerations — one for
//! the dry run and one for the real run — is the second inventory the roadmap sentence refuses, and
//! it is the one that would actually have been built.
//!
//! **Nothing here writes and nothing here enqueues.** Every reading is one `mix doctor` already
//! takes, through the same `mixengine-platform` capability, which is what *"reads the same
//! inventory"* means: what is shared is the **readers** and not [`DoctorReport`]. That document
//! answers *is this as it should be?* and this one asks *is any of ours there?* — and
//! [`Outcome::Ok`] does not mean the same thing across its own rows. `Ok` on the trust check means
//! the authority **is** installed; `Ok` on the hosts check means the block **matches** what the
//! sites need, which on a wired machine is an *empty* block. An uninstall driven off that would
//! remove the trust store and skip the hosts block on one machine and do the reverse on the next
//! (the T87 design, D1).
//!
//! **A reader that fails is [`Removal::Failed`] and never [`Removal::Absent`].** An uninstall that
//! reported "nothing there" because it could not look is the one failure mode this whole feature
//! exists to prevent.
//!
//! [`DoctorReport`]: mixengine_proto::DoctorReport
//! [`Outcome::Ok`]: mixengine_proto::Outcome::Ok

use std::path::Path;

use mixengine_proto::privileged::{FIREWALL_LABEL, FirewallPlan, PrivilegedOp};
use mixengine_proto::{Error, Removal, Residue, ResidueId};

use super::Uninstall;

/// The label every firewall rule MixEngine writes carries, composed the one way `sites::sharing`
/// composes it. Written here as the same expression rather than a second spelling, so a rename in
/// one place is a compile error and not a rule nobody removes.
pub(crate) fn firewall_label() -> String {
    format!("{FIREWALL_LABEL}shared sites")
}

/// Everything this machine holds of MixEngine's, in a fixed order.
///
/// The order is the order the act works in — the machine first, this user's own things next, the
/// home last — so a person reading the plan reads it in the order it will happen.
pub(crate) async fn take(
    uninstall: &Uninstall,
    query: &mixengine_proto::UninstallQuery,
) -> Result<Vec<Residue>, Error> {
    let mut rows = vec![
        hosts_block(uninstall),
        resolver_wiring(uninstall),
        port_access(uninstall).await,
        firewall_rules(uninstall).await,
    ];

    let (trust, browsers) = certificate_rows(uninstall).await;
    rows.push(trust);
    rows.push(browsers);

    rows.push(privileged_helper().await);
    rows.push(audit_log());
    rows.push(autostart_entry(uninstall));
    rows.push(path_entry(uninstall).await);

    // T185a: what this install added to itself, from a copy MixEngine updates itself and nowhere
    // else — a copy a package manager placed is its package's to remove (ADR 0048).
    let recorded: Vec<String> = mixengine_core::updates::records::get(
        &uninstall.store,
        mixengine_core::updates::records::COMPLETED,
    )
    .await
    .ok()
    .flatten()
    .unwrap_or_default();
    let directory = match uninstall.updates.placement() {
        mixengine_core::updates::Placement::SelfUpdatable { directory } => {
            Some(directory.as_path())
        }
        _ => None,
    };
    rows.extend(completed_row(directory, &recorded));
    rows.extend(window_rows(
        is_the_windows_home(uninstall),
        mixengine_platform::window_data::locate(mixengine_core::window::IDENTIFIER).as_ref(),
        query.keep_home,
    ));
    // T182d: what this home and the window keep in the credential store, following the home.
    rows.extend(credential_rows(
        &credentials(uninstall).await,
        query.keep_home,
    ));
    // Every directory `[paths]` has moved out of the root, in `directories()`' own order. On an
    // ordinary home there are none: `Paths::directories` answers the root's own subdirectories, and
    // only a relocation makes one of them lie somewhere else.
    let root = uninstall.paths.root().to_path_buf();
    let moved: Vec<std::path::PathBuf> = uninstall
        .paths
        .directories()
        .into_iter()
        .filter(|directory| *directory != root && !directory.starts_with(&root))
        .map(Path::to_path_buf)
        .collect();

    rows.extend(directory_rows(&root, &moved, query));
    let window_folders: Vec<std::path::PathBuf> = rows
        .iter()
        .filter(|row| matches!(row.id, ResidueId::WindowData | ResidueId::WindowCache))
        .filter(|row| matches!(row.outcome, Removal::Planned { .. }))
        .map(|row| std::path::PathBuf::from(&row.location))
        .collect();
    rows.extend(
        in_use(
            going(&root, &moved, query),
            window_folders,
            !query.skip_holders,
        )
        .await,
    );

    Ok(rows)
}

/// Is this the home the installed MixLab window drives — T182b?
///
/// **The window's folders are one per user and not one per home**, so only the home a release's
/// window uses may take them: an uninstall of a development home, or of one somebody pointed
/// `--home` at, must not delete the connections a person saved in the real one.
fn is_the_windows_home(uninstall: &Uninstall) -> bool {
    mixengine_platform::RELEASE
        && mixengine_core::paths::resolve_root(None, &*uninstall.host)
            .is_ok_and(|default| default == uninstall.paths.root())
}

/// **10b.** The MixLab window's own folders — T182b.
///
/// What a person made there follows the home, kept when it is kept; the webview's cache and the
/// logs go whatever is kept. A folder that is not there has no row: a headless install has none of
/// them, and six rows saying so would only lengthen the plan.
pub(crate) fn window_rows(
    ours: bool,
    found: Option<&mixengine_platform::window_data::WindowData>,
    keep_home: bool,
) -> Vec<Residue> {
    let Some(found) = found.filter(|_| ours) else {
        return Vec::new();
    };

    let data = found
        .data
        .iter()
        .filter(|path| there(path))
        .map(|path| Residue {
            id: ResidueId::WindowData,
            what: "MixLab's saved connections, histories and sync database".to_owned(),
            location: path.display().to_string(),
            outcome: match keep_home {
                true => Removal::Kept {
                    because: "you asked for this home's data to be left where it is".to_owned(),
                },
                false => Removal::Planned {
                    how: "remove this directory and everything under it".to_owned(),
                },
            },
        });

    let cache = found
        .cache
        .iter()
        .filter(|path| there(path))
        .map(|path| Residue {
            id: ResidueId::WindowCache,
            what: "MixLab's cache and logs".to_owned(),
            location: path.display().to_string(),
            outcome: Removal::Planned {
                how: "remove this directory and everything under it".to_owned(),
            },
        });

    data.chain(cache).collect()
}

/// What the credential store holds for this uninstall — T182d. Read once, off the runtime.
pub(crate) struct Found {
    /// This home's id, or `None` when it could not be read.
    pub(crate) home: Option<String>,
    /// Every key under `mixengine`.
    pub(crate) daemon: mixengine_platform::Result<Vec<String>>,
    /// Every key under `MixLab`, when this is the home the release window drives.
    pub(crate) window: Option<mixengine_platform::Result<Vec<String>>>,
}

/// Read the store for [`credential_rows`].
pub(crate) async fn credentials(uninstall: &Uninstall) -> Found {
    let home = mixengine_core::home::id(&uninstall.store)
        .await
        .ok()
        .map(|id| id.as_str().to_owned());
    let window = is_the_windows_home(uninstall);
    let host = std::sync::Arc::clone(&uninstall.host);

    let (daemon, window) = tokio::task::spawn_blocking(move || {
        let keyring = host.keyring();
        (
            keyring.keys(mixengine_platform::KEYRING_SERVICE),
            window.then(|| keyring.keys(mixengine_core::window::KEYRING_SERVICE)),
        )
    })
    .await
    .unwrap_or_else(|error| {
        let failed = || {
            Err(mixengine_platform::Error::Secret {
                action: "list",
                service: mixengine_platform::KEYRING_SERVICE.to_owned(),
                key: "*".to_owned(),
                source: error.to_string().into(),
            })
        };
        (failed(), None)
    });

    Found {
        home,
        daemon,
        window,
    }
}

/// `keys` that belong to `home`.
pub(crate) fn ours(home: &str, keys: &[String]) -> Vec<String> {
    let prefix = format!("{home}/");
    keys.iter()
        .filter(|key| key.starts_with(&prefix))
        .cloned()
        .collect()
}

/// **10d.** This home's passwords and the window's — T182d.
///
/// They follow the home, as the window's data does. A store with nothing of ours has no row. A
/// machine with no store at all is the same answer, because it has nothing stored in one, and a
/// row saying "failed" there would fail every uninstall on a headless Linux. A store that is there
/// and refused is `Failed`, never absent.
pub(crate) fn credential_rows(found: &Found, keep_home: bool) -> Vec<Residue> {
    let row = |id, what: String, location: &str| Residue {
        id,
        what,
        location: location.to_owned(),
        outcome: match keep_home {
            true => Removal::Kept {
                because: "you asked for this home's data to be left where it is, and the \
                          passwords it needs stay with it"
                    .to_owned(),
            },
            false => Removal::Planned {
                how: "remove them from this user's credential store".to_owned(),
            },
        },
    };
    let failed = |id, what: &str, location: &str, because: String| Residue {
        id,
        what: what.to_owned(),
        location: location.to_owned(),
        outcome: Removal::Failed { because },
    };

    let mut rows = Vec::new();
    let daemon_location = format!(
        "{} · {}/…",
        mixengine_platform::KEYRING_SERVICE,
        found.home.as_deref().unwrap_or("this home")
    );

    match (&found.daemon, found.home.as_deref()) {
        (Err(mixengine_platform::Error::UnsupportedPlatform { .. }), _) => {}
        (Err(error), _) => rows.push(failed(
            ResidueId::Credentials,
            "this home's passwords",
            &daemon_location,
            format!("the credential store could not be listed: {error}"),
        )),
        (Ok(_), None) => rows.push(failed(
            ResidueId::Credentials,
            "this home's passwords",
            &daemon_location,
            "this home's id could not be read, so its passwords cannot be told from another \
             home's"
                .to_owned(),
        )),
        (Ok(keys), Some(home)) => {
            let count = ours(home, keys).len();
            if count > 0 {
                let noun = if count == 1 { "password" } else { "passwords" };
                rows.push(row(
                    ResidueId::Credentials,
                    format!("{count} {noun} this home's services and databases use"),
                    &daemon_location,
                ));
            }
        }
    }

    match &found.window {
        None | Some(Err(mixengine_platform::Error::UnsupportedPlatform { .. })) => {}
        Some(Err(error)) => rows.push(failed(
            ResidueId::WindowCredentials,
            "MixLab's saved passwords and sync sign-in",
            mixengine_core::window::KEYRING_SERVICE,
            format!("the credential store could not be listed: {error}"),
        )),
        Some(Ok(keys)) if keys.is_empty() => {}
        Some(Ok(_)) => rows.push(row(
            ResidueId::WindowCredentials,
            "MixLab's saved passwords and sync sign-in".to_owned(),
            mixengine_core::window::KEYRING_SERVICE,
        )),
    }

    rows
}

/// **10c.** The program files this install added to itself — T185a, ADR 0054.
///
/// `directory` is the install's when its placement is `SelfUpdatable`, and `None` otherwise. One row
/// for every recorded file still there, because the uninstall keys what it did by id; its location
/// is the install directory, which `mix` must not measure as something going.
pub(crate) fn completed_row(directory: Option<&Path>, recorded: &[String]) -> Option<Residue> {
    let directory = directory?;
    let present: Vec<&str> = recorded
        .iter()
        .map(String::as_str)
        .filter(|name| there(&directory.join(format!("{name}{}", std::env::consts::EXE_SUFFIX))))
        .collect();

    if present.is_empty() {
        return None;
    }

    Some(Residue {
        id: ResidueId::CompletedBinary,
        what: format!(
            "program files this install added to itself: {}",
            present.join(", ")
        ),
        location: directory.display().to_string(),
        outcome: Removal::Planned {
            how: "remove these files from the program's directory".to_owned(),
        },
    })
}

/// **11.** The home, each relocated directory, and every tombstone an earlier run left beside them
/// — T182, D2 and D6.
///
/// The home answers `keep_home` and the relocated directories answer `keep_relocated`: two choices,
/// because a person may want the home gone and the databases on another disk kept, or the reverse.
/// A tombstone is garbage whatever is kept, so it is always `Planned`.
pub(crate) fn directory_rows(
    root: &Path,
    moved: &[std::path::PathBuf],
    query: &mixengine_proto::UninstallQuery,
) -> Vec<Residue> {
    let mut rows = vec![home(root, query.keep_home)];

    for directory in moved {
        rows.push(relocated(directory, query.keep_relocated));
    }

    rows.extend(
        mixengine_platform::tombstone::tombstones_beside(root)
            .iter()
            .map(|path| tombstone(ResidueId::Home, path)),
    );

    for directory in moved {
        rows.extend(
            mixengine_platform::tombstone::tombstones_beside(directory)
                .iter()
                .map(|path| tombstone(ResidueId::RelocatedDirectory, path)),
        );
    }

    rows
}

/// The directories this uninstall would remove: the ones nobody asked to keep.
pub(crate) fn going(
    root: &Path,
    moved: &[std::path::PathBuf],
    query: &mixengine_proto::UninstallQuery,
) -> Vec<std::path::PathBuf> {
    let mut going = Vec::new();

    if !query.keep_home {
        going.push(root.to_path_buf());
    }

    if !query.keep_relocated {
        going.extend(moved.iter().cloned());
    }

    going
}

/// **11c.** A tombstone an earlier uninstall renamed and could not delete — T182, D6.
fn tombstone(id: ResidueId, path: &Path) -> Residue {
    Residue {
        id,
        what: "a directory an earlier uninstall set aside and could not finish removing".to_owned(),
        location: path.display().to_string(),
        outcome: Removal::Planned {
            how: "remove this directory and everything under it".to_owned(),
        },
    }
}

/// **12.** Every process in the way of a directory this uninstall would remove — T182, D4, and
/// T182e, D4.
///
/// Two kinds, one row per process: a process *running from* one of those directories (any system),
/// and a process holding something inside one of them that **cannot be moved** — a working
/// directory, a file held without sharing delete (Windows, where that refuses a rename). What can be
/// moved is not mentioned: the rename moves it out on its own (`main`'s
/// `remove_what_the_uninstall_armed`). The window's folders are looked in for holders too; nothing
/// runs from them.
///
/// **This daemon and everything it started are spared**: its own shutdown stops them, in
/// dependency order, before anything is removed. A table that cannot be read spares everybody — the
/// rename in `mixengine_platform::tombstone` is the second line of defence, and it refuses rather
/// than half-deletes.
async fn in_use(
    directories: Vec<std::path::PathBuf>,
    window_folders: Vec<std::path::PathBuf>,
    look_for_holders: bool,
) -> Vec<Residue> {
    if directories.is_empty() && window_folders.is_empty() {
        return Vec::new();
    }

    let spare = Some(std::process::id());
    let (occupants, held) = crate::api::on_a_blocking_thread(move || {
        let occupants = mixengine_platform::occupants::processes_under(&directories, spare);
        let held = match look_for_holders {
            true => {
                let mut everywhere = directories;
                everywhere.extend(window_folders);
                mixengine_platform::occupants::held_under(&everywhere, spare)
            }
            false => Vec::new(),
        };
        Ok((occupants, held))
    })
    .await
    .unwrap_or_else(|error| {
        tracing::warn!(%error, "the processes in the way of an uninstall could not be read");
        (Vec::new(), Vec::new())
    });

    let running: Vec<Residue> = occupants
        .into_iter()
        .map(|occupant| Residue {
            id: ResidueId::InUse,
            what: format!("{} (pid {})", occupant.name, occupant.pid),
            location: occupant.executable.display().to_string(),
            outcome: Removal::Blocked {
                by: format!(
                    "{} is running from a directory this uninstall removes; close it and run the \
                     uninstall again",
                    occupant.name
                ),
            },
        })
        .collect();

    let stuck = stuck_rows(&held, &running);
    running.into_iter().chain(stuck).collect()
}

/// A row per process holding something stuck, that is not already a row for running — T182e, D4.
///
/// Matched on the `(pid N)` the running row's `what` ends with, which is how both kinds spell it.
fn stuck_rows(
    held: &[mixengine_platform::occupants::HeldItem],
    running: &[Residue],
) -> Vec<Residue> {
    let mut by_process: std::collections::BTreeMap<u32, (String, Vec<String>)> =
        std::collections::BTreeMap::new();

    for item in held.iter().filter(|item| !item.movable) {
        for holder in &item.holders {
            by_process
                .entry(holder.pid)
                .or_insert_with(|| (holder.name.clone(), Vec::new()))
                .1
                .push(item.path.display().to_string());
        }
    }

    by_process
        .into_iter()
        .filter(|(pid, _)| {
            let tag = format!("(pid {pid})");
            !running.iter().any(|row| row.what.ends_with(&tag))
        })
        .map(|(pid, (name, mut paths))| {
            paths.sort();
            let more = match paths.len() - 1 {
                0 => String::new(),
                more => format!(" (and {more} more)"),
            };
            let first = paths.swap_remove(0);
            Residue {
                id: ResidueId::InUse,
                what: format!("{name} (pid {pid})"),
                location: first.clone(),
                outcome: Removal::Blocked {
                    by: format!(
                        "{name} has {first} open, so it cannot be moved or deleted{more}; close it \
                         and run the uninstall again"
                    ),
                },
            }
        })
        .collect()
}

/// **1.** The block in this machine's hosts file, read the way T41 reads it.
fn hosts_block(uninstall: &Uninstall) -> Residue {
    let file = uninstall.host.hosts_file();
    let location = file.path().display().to_string();

    let outcome = match file.managed() {
        // An empty block is a machine that has never had one, which is not a failure — the trait's
        // own words.
        Ok(entries) if entries.is_empty() => Removal::Absent {},
        Ok(entries) => Removal::Planned {
            how: PrivilegedOp::hosts_apply(Vec::new()).describe()
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
fn resolver_wiring(uninstall: &Uninstall) -> Residue {
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
async fn port_access(uninstall: &Uninstall) -> Residue {
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
async fn firewall_rules(uninstall: &Uninstall) -> Residue {
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
async fn certificate_rows(uninstall: &Uninstall) -> (Residue, Residue) {
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

/// **7.** `mixengine-elevate`, in the one directory an ordinary account cannot write.
///
/// **`symlink_metadata` and not `exists`**, which answers `false` for a dangling link somebody
/// planted — the rule `mixengine-elevate`'s own validation runs on, applied to the reading side.
///
/// **Its owner is asked only when it is there** — roadmap task **T88e**. A package database is a
/// process or two, and a file that is absent has nobody to name. A database that cannot answer is
/// "no package", which is the row as it was before that task: a helper this uninstall removes.
async fn privileged_helper() -> Residue {
    let Ok(path) = mixengine_platform::install::helper_path() else {
        return helper_row(None, false, None);
    };

    let present = there(&path);

    let packaged = match present {
        true => {
            let asked = path.clone();
            tokio::task::spawn_blocking(move || mixengine_platform::install::packaged_by(&asked))
                .await
                .ok()
                .flatten()
        }
        false => None,
    };

    helper_row(Some(&path), present, packaged)
}

/// The helper's row from what was read: where it is, whether it is there, and which package, if
/// any, placed it — the T88e design, D3.
///
/// **A helper a package placed is kept, and says so.** `mix`, `mixengined` and the shim leave the
/// way they came (the T87 design, Scope); a helper the same package wrote is part of the same
/// delivery, and removing it leaves the package database describing a file that is gone.
fn helper_row(path: Option<&Path>, present: bool, packaged: Option<String>) -> Residue {
    let what = "MixEngine's privileged helper".to_owned();

    let Some(path) = path else {
        return Residue {
            id: ResidueId::PrivilegedHelper,
            what,
            location: "this machine will not name a directory for a privileged helper".to_owned(),
            outcome: Removal::Absent {},
        };
    };

    let outcome = match (present, packaged) {
        (false, _) => Removal::Absent {},
        (true, Some(package)) => Removal::Kept {
            because: format!(
                "it came with the {package} package, and removing that package removes it"
            ),
        },
        (true, None) => Removal::Planned {
            how: PrivilegedOp::HelperRemove {}.describe(),
        },
    };

    Residue {
        id: ResidueId::PrivilegedHelper,
        what,
        location: path.display().to_string(),
        outcome,
    }
}

/// **8.** The root-owned record of what ran as root, outside `MIXENGINE_HOME`.
fn audit_log() -> Residue {
    let what = "the log of everything MixEngine has done as an administrator".to_owned();

    let Ok(directory) = mixengine_platform::elevated::audit_directory() else {
        return Residue {
            id: ResidueId::AuditLog,
            what,
            location: "this machine will not name a directory for the log".to_owned(),
            outcome: Removal::Absent {},
        };
    };

    let path = directory.join("elevate.log");
    let outcome = match there(&path) {
        true => Removal::Planned {
            how: PrivilegedOp::AuditLogRemove {}.describe(),
        },
        false => Removal::Absent {},
    };

    Residue {
        id: ResidueId::AuditLog,
        what,
        location: path.display().to_string(),
        outcome,
    }
}

/// **9.** The entry that starts this home's daemon at login — T85b's, and unprivileged.
///
/// **Only when it is this home's.** One entry per user means enabling from a second home replaces
/// it, so an entry naming another home is that home's to remove and not this one's — which is
/// exactly what `AutostartReport::for_this_home` exists to say.
fn autostart_entry(uninstall: &Uninstall) -> Residue {
    let what = "the entry that starts this home's daemon at login".to_owned();

    match uninstall.autostart.status() {
        Ok(report) => Residue {
            id: ResidueId::AutostartEntry,
            what,
            location: report.location.clone(),
            outcome: match report.enabled && report.for_this_home {
                true => Removal::Planned {
                    how:
                        "remove the entry that starts this home's daemon at login, which needs no \
                          administrator"
                            .to_owned(),
                },
                false => Removal::Absent {},
            },
        },
        Err(error) => Residue {
            id: ResidueId::AutostartEntry,
            what,
            location: "this machine's login configuration".to_owned(),
            outcome: Removal::Failed {
                because: format!(
                    "this machine could not be asked what it starts at login: {}",
                    mixengine_proto::flatten(&error)
                ),
            },
        },
    }
}

/// **10.** `<root>/bin` on this user's `PATH` — T26's, and unprivileged.
async fn path_entry(uninstall: &Uninstall) -> Residue {
    let what = "this home's commands on your PATH".to_owned();

    match uninstall.shims.status().await {
        Ok(report) => {
            let carrying: Vec<&mixengine_proto::PathPlace> =
                report.places.iter().filter(|place| place.present).collect();

            Residue {
                id: ResidueId::PathEntry,
                what,
                location: match carrying.is_empty() {
                    true => report.directory.clone(),
                    false => carrying
                        .iter()
                        .map(|place| place.name.clone())
                        .collect::<Vec<String>>()
                        .join(", "),
                },
                outcome: match carrying.is_empty() {
                    true => Removal::Absent {},
                    false => Removal::Planned {
                        how: format!(
                            "take {} off your PATH in {} place(s), which needs no administrator",
                            report.directory,
                            carrying.len()
                        ),
                    },
                },
            }
        }
        Err(error) => Residue {
            id: ResidueId::PathEntry,
            what,
            location: "this user's PATH".to_owned(),
            outcome: Removal::Failed {
                because: format!(
                    "this user's PATH could not be read, so what is on it is unknown: {}",
                    mixengine_proto::flatten(&error)
                ),
            },
        },
    }
}

/// **11.** `MIXENGINE_HOME` itself, and the one row that is never `Absent`.
///
/// **`Kept` says so rather than the row disappearing.** A person reading the plan has to see that
/// the one irreversible thing on the list was considered and deliberately left.
pub(crate) fn home(root: &Path, keep: bool) -> Residue {
    Residue {
        id: ResidueId::Home,
        what: "this home's own directory, and everything in it".to_owned(),
        location: root.display().to_string(),
        outcome: match keep {
            true => Removal::Kept {
                because: "you asked for this home to be left where it is".to_owned(),
            },
            false => Removal::Planned {
                how: "remove this directory and everything under it, including the databases in \
                      data/ and every certificate this home has issued"
                    .to_owned(),
            },
        },
    }
}

/// **11b.** A directory `[paths]` has moved out of the root.
fn relocated(directory: &Path, keep: bool) -> Residue {
    Residue {
        id: ResidueId::RelocatedDirectory,
        what: "a directory this home was configured to keep somewhere else".to_owned(),
        location: directory.display().to_string(),
        outcome: match keep {
            true => Removal::Kept {
                because: "you asked for the directories moved out of this home to be left where \
                          they are"
                    .to_owned(),
            },
            false => Removal::Planned {
                how: "remove this directory and everything under it".to_owned(),
            },
        },
    }
}

/// Is something there, without following a symlink at the end of the path?
fn there(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// Where this machine keeps the resolver configuration, in words.
///
/// **In words and not as a path**, because two of the three mechanisms are not files: NRPT is a
/// registry key and `systemd-networkd` is a link that has to be reloaded. A row that named a path
/// for one and a mechanism for the others would be inviting a person to go and look at something
/// that is not there.
fn resolver_place(method: mixengine_platform::ResolverMethod) -> &'static str {
    match method {
        mixengine_platform::ResolverMethod::ResolverDirectory => {
            "this machine's per-TLD resolver files"
        }
        mixengine_platform::ResolverMethod::SystemdLink => {
            "this machine's systemd-networkd link configuration"
        }
        mixengine_platform::ResolverMethod::Nrpt => "this machine's Name Resolution Policy Table",
        mixengine_platform::ResolverMethod::None => {
            "this machine has no mechanism for routing one TLD to one server"
        }
    }
}

/// Which store this machine keeps its trusted authorities in, in words — [`resolver_place`]'s rule.
fn trust_place(method: mixengine_platform::TrustStoreMethod) -> &'static str {
    match method {
        mixengine_platform::TrustStoreMethod::SystemRoot => {
            "this machine's Root store, under Local Machine"
        }
        mixengine_platform::TrustStoreMethod::SystemKeychain => "this machine's System keychain",
        mixengine_platform::TrustStoreMethod::CaCertificates
        | mixengine_platform::TrustStoreMethod::CaTrustAnchors => {
            "this machine's certificate anchors directory"
        }
        mixengine_platform::TrustStoreMethod::None => {
            "this machine has no system trust store MixEngine knows how to write"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixengine_platform::occupants::{HeldItem, Holder};

    fn held(path: &str, movable: bool, holders: &[(u32, &str)]) -> HeldItem {
        HeldItem {
            path: std::path::PathBuf::from(path),
            movable,
            holders: holders
                .iter()
                .map(|(pid, name)| Holder {
                    pid: *pid,
                    name: (*name).to_owned(),
                })
                .collect(),
        }
    }

    /// T182e, D4: a movable item is not a row at all — the rename moves it.
    #[test]
    fn a_movable_item_is_not_a_row() {
        let rows = stuck_rows(&[held(r"C:\home\bin", true, &[(1200, "Code.exe")])], &[]);
        assert!(rows.is_empty(), "{rows:?}");
    }

    /// T182e, D4: a stuck item is a `Blocked` row naming the program, the path and what to do.
    #[test]
    fn a_stuck_item_is_a_blocked_row() {
        let rows = stuck_rows(&[held(r"C:\home\data", false, &[(7, "pwsh.exe")])], &[]);

        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].id, ResidueId::InUse);
        assert_eq!(rows[0].what, "pwsh.exe (pid 7)");
        assert_eq!(rows[0].location, r"C:\home\data");
        let Removal::Blocked { by } = &rows[0].outcome else {
            panic!("{:?}", rows[0].outcome)
        };
        assert!(
            by.contains(r"has C:\home\data open, so it cannot be moved or deleted"),
            "{by}"
        );
        assert!(by.ends_with("close it and run the uninstall again"), "{by}");
    }

    /// One program holding many stuck things is one row with a count.
    #[test]
    fn many_stuck_items_of_one_process_are_one_row() {
        let rows = stuck_rows(
            &[
                held(r"C:\home\a", false, &[(9, "tool.exe")]),
                held(r"C:\home\b", false, &[(9, "tool.exe")]),
                held(r"C:\home\c", false, &[(9, "tool.exe")]),
            ],
            &[],
        );

        assert_eq!(rows.len(), 1, "{rows:?}");
        let Removal::Blocked { by } = &rows[0].outcome else {
            panic!("{:?}", rows[0].outcome)
        };
        assert!(by.contains("(and 2 more)"), "{by}");
        assert_eq!(rows[0].location, r"C:\home\a");
    }

    /// A process already a row for running from the home is not a second row.
    #[test]
    fn a_process_running_and_stuck_is_one_row() {
        let running = Residue {
            id: ResidueId::InUse,
            what: "php.exe (pid 42)".to_owned(),
            location: r"C:\home\runtimes\php.exe".to_owned(),
            outcome: Removal::Blocked {
                by: "running".to_owned(),
            },
        };

        let rows = stuck_rows(
            &[held(r"C:\home\data", false, &[(42, "php.exe")])],
            &[running],
        );

        assert!(rows.is_empty(), "{rows:?}");
    }

    fn found(daemon: &[&str], window: Option<&[&str]>) -> Found {
        Found {
            home: Some("0123456789ab".to_owned()),
            daemon: Ok(daemon.iter().map(|key| (*key).to_owned()).collect()),
            window: window.map(|keys| Ok(keys.iter().map(|key| (*key).to_owned()).collect())),
        }
    }

    /// Review focus 4: another home's keys are neither counted nor offered.
    #[test]
    fn only_this_homes_keys_make_the_row() {
        let rows = credential_rows(
            &found(
                &[
                    "0123456789ab/mariadb@main/root",
                    "ffffffffffff/mariadb@main/root",
                    "mariadb@main/root",
                ],
                None,
            ),
            false,
        );

        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].id, ResidueId::Credentials);
        assert!(rows[0].what.starts_with("1 password"), "{rows:?}");
        assert!(matches!(rows[0].outcome, Removal::Planned { .. }));
    }

    #[test]
    fn nothing_stored_is_no_row() {
        assert!(credential_rows(&found(&["ffffffffffff/x/y"], Some(&[][..])), false).is_empty());
    }

    #[test]
    fn a_kept_home_keeps_both_rows() {
        let rows = credential_rows(&found(&["0123456789ab/a/b"], Some(&["vault"][..])), true);

        assert_eq!(
            rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            [ResidueId::Credentials, ResidueId::WindowCredentials]
        );
        assert!(
            rows.iter()
                .all(|row| matches!(row.outcome, Removal::Kept { .. })),
            "{rows:?}"
        );
    }

    /// Review focus 1: a machine with no credential store has nothing in one.
    #[test]
    fn no_store_at_all_is_no_row() {
        let absent = || {
            Err(mixengine_platform::Error::UnsupportedPlatform {
                capability: "Keyring",
                reason: "none".to_owned(),
            })
        };
        let rows = credential_rows(
            &Found {
                home: Some("0123456789ab".to_owned()),
                daemon: absent(),
                window: Some(absent()),
            },
            false,
        );

        assert!(rows.is_empty(), "{rows:?}");
    }

    /// A store that is there and refused is a failure, never "nothing there".
    #[test]
    fn a_store_that_refused_is_a_failed_row() {
        let refused = Err(mixengine_platform::Error::Secret {
            action: "list",
            service: "mixengine".to_owned(),
            key: "*".to_owned(),
            source: "denied".into(),
        });
        let rows = credential_rows(
            &Found {
                home: Some("0123456789ab".to_owned()),
                daemon: refused,
                window: None,
            },
            false,
        );

        assert!(
            matches!(
                rows[..],
                [Residue {
                    id: ResidueId::Credentials,
                    outcome: Removal::Failed { .. },
                    ..
                }]
            ),
            "{rows:?}"
        );
    }

    /// T185a: what the install added to itself is one planned row, listing the recorded names.
    #[test]
    fn what_the_install_added_to_itself_is_one_planned_row() {
        let directory = tempfile::tempdir().expect("a directory");
        let exe = std::env::consts::EXE_SUFFIX;
        std::fs::write(
            directory.path().join(format!("mixengine-trampoline{exe}")),
            b"x",
        )
        .expect("a file");

        let row = completed_row(Some(directory.path()), &["mixengine-trampoline".to_owned()])
            .expect("a row");

        assert_eq!(row.id, ResidueId::CompletedBinary);
        assert!(row.what.contains("mixengine-trampoline"), "{row:?}");
        assert!(matches!(row.outcome, Removal::Planned { .. }), "{row:?}");
    }

    /// No row for a copy a package manager placed (ADR 0048), and none for a file already gone.
    #[test]
    fn no_row_without_a_writable_install_or_without_a_file() {
        let directory = tempfile::tempdir().expect("a directory");
        let recorded = ["mixengine-trampoline".to_owned()];

        assert!(completed_row(None, &recorded).is_none());
        assert!(completed_row(Some(directory.path()), &recorded).is_none());
    }

    fn query(keep_home: bool, keep_relocated: bool) -> mixengine_proto::UninstallQuery {
        mixengine_proto::UninstallQuery {
            keep_home,
            keep_relocated,
            grant: false,
            skip_holders: false,
        }
    }

    /// T182, D2. The home answers `keep_home` and a relocated directory answers `keep_relocated`,
    /// in all four combinations.
    #[test]
    fn the_home_and_the_relocated_directories_are_kept_separately() {
        let place = tempfile::tempdir().expect("tempdir");
        let root = place.path().join("MixEngine");
        let moved = vec![place.path().join("logs")];

        for (keep_home, keep_relocated) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            let rows = directory_rows(&root, &moved, &query(keep_home, keep_relocated));
            let kept = |id| {
                rows.iter()
                    .find(|row| row.id == id)
                    .map(|row| matches!(row.outcome, Removal::Kept { .. }))
                    .expect("a row")
            };

            assert_eq!(kept(ResidueId::Home), keep_home, "{rows:?}");
            assert_eq!(
                kept(ResidueId::RelocatedDirectory),
                keep_relocated,
                "{rows:?}"
            );

            let going = going(&root, &moved, &query(keep_home, keep_relocated));
            assert_eq!(going.contains(&root), !keep_home);
            assert_eq!(going.contains(&moved[0]), !keep_relocated);
        }
    }

    /// T182, D6. A tombstone an earlier run could not delete is planned for removal, whatever is
    /// kept, under the id of the directory it was.
    #[test]
    fn an_old_tombstone_is_planned_for_removal_whatever_is_kept() {
        let place = tempfile::tempdir().expect("tempdir");
        let root = place.path().join("MixEngine");
        let old = place.path().join("MixEngine.removing-1");
        std::fs::create_dir_all(&old).expect("a tombstone");

        let rows = directory_rows(&root, &[], &query(true, true));

        let row = rows
            .iter()
            .find(|row| row.location == old.display().to_string())
            .expect("the tombstone is a row");
        assert_eq!(row.id, ResidueId::Home);
        assert!(matches!(row.outcome, Removal::Planned { .. }), "{row:?}");
    }

    /// `keep_home` is a row's answer and not a missing row: a person reading the plan has to see
    /// that the home was considered and deliberately left.
    #[test]
    fn keeping_the_home_says_so_on_the_home_row() {
        let kept = home(Path::new("/tmp/home"), true);

        assert_eq!(kept.id, ResidueId::Home);
        assert!(matches!(kept.outcome, Removal::Kept { .. }), "{kept:?}");
        assert_eq!(kept.location, "/tmp/home");
    }

    /// And removing it names what goes with it. The databases in `data/` are the thing a person
    /// most needs to have been told about before they answer the question.
    #[test]
    fn removing_the_home_says_what_goes_with_it() {
        let planned = home(Path::new("/tmp/home"), false);

        let Removal::Planned { how } = &planned.outcome else {
            panic!("{planned:?}");
        };

        assert!(how.contains("data/"), "{how}");
    }

    /// Every mechanism has somewhere to point a person, including the two that mean "this machine
    /// has none of that": a row whose location is empty is a row nobody can act on.
    #[test]
    fn every_mechanism_names_a_place_to_look() {
        for method in [
            mixengine_platform::ResolverMethod::ResolverDirectory,
            mixengine_platform::ResolverMethod::SystemdLink,
            mixengine_platform::ResolverMethod::Nrpt,
            mixengine_platform::ResolverMethod::None,
        ] {
            assert!(!resolver_place(method).is_empty());
        }

        for method in [
            mixengine_platform::TrustStoreMethod::SystemRoot,
            mixengine_platform::TrustStoreMethod::SystemKeychain,
            mixengine_platform::TrustStoreMethod::CaCertificates,
            mixengine_platform::TrustStoreMethod::CaTrustAnchors,
            mixengine_platform::TrustStoreMethod::None,
        ] {
            assert!(!trust_place(method).is_empty());
        }
    }

    /// The label is composed once. A rule written under one name and looked for under another is a
    /// rule nobody ever removes.
    #[test]
    fn the_firewall_label_is_the_one_sharing_writes() {
        assert_eq!(firewall_label(), format!("{FIREWALL_LABEL}shared sites"));
    }

    /// A dangling symlink is something, not nothing: `exists` answers `false` for one, and a plan
    /// that skipped it would leave a link named after MixEngine's helper on the machine.
    #[cfg(unix)]
    #[test]
    fn a_dangling_link_is_still_something_that_is_there() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let link = directory.path().join("helper");
        std::os::unix::fs::symlink(directory.path().join("nowhere"), &link).expect("the link");

        assert!(!link.exists());
        assert!(there(&link));
    }

    /// **A helper a package placed is kept, and the row names the package** — the T88e design, D3.
    #[test]
    fn a_helper_a_package_placed_is_kept_and_names_the_package() {
        let row = helper_row(
            Some(Path::new("/usr/local/libexec/mixengine/mixengine-elevate")),
            true,
            Some("mixengine".to_owned()),
        );

        assert_eq!(row.id, ResidueId::PrivilegedHelper);
        assert_eq!(
            row.outcome,
            Removal::Kept {
                because: "it came with the mixengine package, and removing that package removes it"
                    .to_owned(),
            }
        );
    }

    /// A helper no package claims is still removed, and one that is not there is not, whoever
    /// would have owned it.
    #[test]
    fn a_helper_no_package_claims_is_planned_and_an_absent_one_is_absent() {
        let path = Path::new("/usr/local/libexec/mixengine/mixengine-elevate");

        assert!(matches!(
            helper_row(Some(path), true, None).outcome,
            Removal::Planned { .. }
        ));
        assert_eq!(
            helper_row(Some(path), false, Some("mixengine".to_owned())).outcome,
            Removal::Absent {}
        );
        assert_eq!(helper_row(None, false, None).outcome, Removal::Absent {});
    }

    /// T182b. The window's folders are this home's only when it is the one the installed window
    /// uses, what a person made follows the home, and the cache goes whatever is kept.
    #[test]
    fn the_windows_folders_follow_the_home_and_the_cache_always_goes() {
        let root = tempfile::tempdir().expect("a temporary directory");
        let data = root.path().join("Roaming").join("io.github.mixnz.mixlab");
        let cache = root.path().join("Local").join("io.github.mixnz.mixlab");
        let missing = root.path().join("Caches").join("io.github.mixnz.mixlab");
        std::fs::create_dir_all(&data).expect("the data folder");
        std::fs::create_dir_all(&cache).expect("the cache folder");

        let found = mixengine_platform::window_data::WindowData {
            data: vec![data.clone()],
            cache: vec![cache.clone(), missing],
        };

        let removed = window_rows(true, Some(&found), false);
        assert_eq!(
            removed
                .iter()
                .map(|row| (
                    row.id,
                    row.location.clone(),
                    matches!(row.outcome, Removal::Planned { .. })
                ))
                .collect::<Vec<_>>(),
            vec![
                (ResidueId::WindowData, data.display().to_string(), true),
                (ResidueId::WindowCache, cache.display().to_string(), true),
            ],
            "{removed:?}"
        );

        let kept = window_rows(true, Some(&found), true);
        assert!(matches!(kept[0].outcome, Removal::Kept { .. }), "{kept:?}");
        assert!(
            matches!(kept[1].outcome, Removal::Planned { .. }),
            "{kept:?}"
        );

        assert!(
            window_rows(false, Some(&found), false).is_empty(),
            "a home the installed window does not use took the window's folders"
        );
    }
}
