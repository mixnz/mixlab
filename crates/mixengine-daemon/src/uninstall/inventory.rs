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

use mixengine_proto::privileged::{FIREWALL_LABEL, PrivilegedOp};
use mixengine_proto::{Error, Removal, Residue, ResidueId};

use super::Uninstall;

mod credential_store;
mod machine;

pub(crate) use credential_store::{credential_rows, credentials, ours};
use machine::{certificate_rows, firewall_rules, hosts_block, port_access, resolver_wiring};

/// How every sentence starts that keeps a row for another home's sake — what the report counts.
pub(crate) const FOR_ANOTHER_HOME: &str = "kept for the other homes on this machine: ";

/// Why the helper and its log stay for another home, or [`None`] when this home is the last — the
/// same answer `mixengine-elevate` reaches from the same ledger (ADR 0066).
///
/// **No ledger, or one that cannot be read, keeps them**: keeping a file is recoverable, removing
/// another home's is not.
pub(crate) fn for_others(
    ledger: Result<Option<mixengine_proto::privileged::HomesLedger>, String>,
    root: &Path,
    alive: impl Fn(&Path) -> bool,
) -> Option<String> {
    let ledger = match ledger {
        Ok(Some(ledger)) => ledger,
        Ok(None) => {
            return Some(format!(
                "{FOR_ANOTHER_HOME}no record yet says which other homes use it"
            ));
        }
        Err(why) => {
            return Some(format!(
                "{FOR_ANOTHER_HOME}the record of which homes use it could not be read ({why})"
            ));
        }
    };

    let others = ledger.living_others(root, alive);

    (!others.is_empty()).then(|| {
        format!(
            "{FOR_ANOTHER_HOME}{} still {} it",
            others
                .iter()
                .map(|other| other.display().to_string())
                .collect::<Vec<_>>()
                .join(", "),
            if others.len() == 1 { "uses" } else { "use" }
        )
    })
}

/// `row`, kept with `sentence` when it would have gone — and as it was otherwise.
pub(crate) fn keep_for_others(row: Residue, sentence: Option<String>) -> Residue {
    match (&row.outcome, sentence) {
        (Removal::Planned { .. }, Some(because)) => Residue {
            outcome: Removal::Kept { because },
            ..row
        },
        _ => row,
    }
}

/// The helper's ledger of homes, from the audit directory.
fn ledger() -> Result<Option<mixengine_proto::privileged::HomesLedger>, String> {
    mixengine_platform::elevated::audit_directory()
        .and_then(|directory| mixengine_platform::ledger::read(&directory))
        .map_err(|error| mixengine_proto::flatten(&error))
}

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

    // ADR 0066: the helper and its log are the machine's, and stay while another home uses them —
    // read here from the helper's own ledger, which the helper reads again before it acts.
    let shared = for_others(
        ledger(),
        &mixengine_platform::paths::in_full(uninstall.paths.root()),
        mixengine_platform::ledger::alive,
    );
    rows.push(keep_for_others(privileged_helper().await, shared.clone()));
    rows.push(keep_for_others(audit_log(), shared));

    // T182a: the program itself, where an installer placed it. Read here, from the disk, like
    // every other row; whether it goes is the query's `package`.
    let present: Vec<std::path::PathBuf> = mixengine_platform::install::package_paths()
        .map(|paths| {
            paths
                .binaries
                .into_iter()
                .chain([paths.bundle])
                .filter(|path| there(path))
                .collect()
        })
        .unwrap_or_default();
    // Whether this copy has the window, asked the way `database.client` asks: a headless package
    // has none, and on macOS the two packages share one receipt.
    let window = matches!(
        uninstall.host.desktop_apps().locate_window(
            mixengine_core::window::EXECUTABLE,
            mixengine_core::window::BUNDLE
        ),
        Ok(mixengine_platform::Located::Installed(_))
    );
    rows.extend(package_row(&PackageReading {
        os: std::env::consts::OS,
        placement: uninstall.updates.placement(),
        asked: query.package,
        present,
        window,
    }));
    rows.push(autostart_entry(uninstall).await);
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

/// What [`package_row`] decides from, read by [`take`] and written by hand in tests.
pub(crate) struct PackageReading<'a> {
    /// `std::env::consts::OS`.
    pub(crate) os: &'a str,
    /// Who placed this copy, from the daemon's start (T88f's reading of the receipt).
    pub(crate) placement: &'a mixengine_core::updates::Placement,
    /// The query's `package`.
    pub(crate) asked: bool,
    /// Which of the `.pkg`'s paths are on disk now.
    pub(crate) present: Vec<std::path::PathBuf>,
    /// Whether this install has MixLab, the window.
    pub(crate) window: bool,
}

/// **7a.** The program itself, where an installer placed it — roadmap task **T182a**, spec D3.
///
/// On a copy the macOS `.pkg` placed, `Planned` when asked for and `Kept` naming the flag when not;
/// on a copy a Linux package or the Windows installer placed, `Kept` naming what removes it there.
/// A copy no installer placed has no row: nothing here would ever remove it, and nothing should.
pub(crate) fn package_row(reading: &PackageReading<'_>) -> Option<Residue> {
    use mixengine_core::updates::Placement;

    let what = match reading.window {
        true => "the program itself: its commands, the MixLab application and the package receipt",
        false => "the program itself: its commands and the package receipt",
    }
    .to_owned();

    let (location, outcome) = match reading.placement {
        Placement::Installer { receipt, .. } if reading.os == "macos" && receipt == PKG_RECEIPT => {
            let outcome = match (reading.asked, reading.present.is_empty()) {
                (true, true) => Removal::Absent {},
                (true, false) => Removal::Planned {
                    how: PrivilegedOp::PackageRemove {}.describe(),
                },
                (false, _) => Removal::Kept {
                    because: "it stays unless asked for: `mix uninstall --package` removes it as \
                              well"
                        .to_owned(),
                },
            };
            (receipt.clone(), outcome)
        }
        Placement::Installer { receipt, .. } => (
            receipt.clone(),
            Removal::Kept {
                because: package_manager_sentence(receipt),
            },
        ),
        Placement::SelfUpdatable { directory } if reading.os == "windows" => (
            directory.display().to_string(),
            Removal::Kept {
                because: "the Windows uninstaller removes the program".to_owned(),
            },
        ),
        _ => return None,
    };

    Some(Residue {
        id: ResidueId::Package,
        what,
        location,
        outcome,
    })
}

/// The receipt `mixengine_platform` reads off the macOS package database for a `.pkg` copy.
const PKG_RECEIPT: &str = "dev.mixengine.cli";

/// How the program goes where a package manager owns it — `dpkg:<name>` and `rpm:<name>` are how
/// `mixengine-platform` spells those receipts.
fn package_manager_sentence(receipt: &str) -> String {
    match receipt.split_once(':') {
        Some(("dpkg", package)) => {
            format!("it came with the {package} package: `sudo apt remove {package}` removes it")
        }
        Some(("rpm", package)) => {
            format!("it came with the {package} package: `sudo dnf remove {package}` removes it")
        }
        _ => format!("it came with the {receipt} package, and removing that package removes it"),
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
///
/// **And the linger this home turned on, with or without the entry**, because a unit deleted by
/// hand leaves it behind, and the account would go on lingering for a home that is gone.
async fn autostart_entry(uninstall: &Uninstall) -> Residue {
    let what = "the entry that starts this home's daemon at login".to_owned();

    let asked = async {
        Ok::<_, mixengine_proto::Error>((
            uninstall.autostart.status().await?,
            uninstall.autostart.holds_linger().await?,
        ))
    }
    .await;

    match asked {
        Ok((report, holds_linger)) => Residue {
            id: ResidueId::AutostartEntry,
            what,
            location: report.location.clone(),
            outcome: autostart_outcome(&report, holds_linger),
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

/// What row 9 plans, from the entry's report and whether this home holds the account's linger.
pub(super) fn autostart_outcome(
    report: &mixengine_proto::AutostartReport,
    holds_linger: bool,
) -> Removal {
    let entry = report.enabled && report.for_this_home;

    let how = match (entry, holds_linger) {
        (true, false) => {
            "remove the entry that starts this home's daemon at login, which needs no administrator"
        }
        (true, true) => {
            "remove the entry that starts this home's daemon at login, and turn off the systemd \
             linger MixEngine turned on for it, neither of which needs an administrator"
        }
        (false, true) => {
            "turn off the systemd linger MixEngine turned on for this home, which needs no \
             administrator"
        }
        (false, false) => return Removal::Absent {},
    };

    Removal::Planned {
        how: how.to_owned(),
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
mod tests;
