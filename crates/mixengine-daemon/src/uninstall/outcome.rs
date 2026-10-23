//! What an uninstall leaves behind and how its answer is settled from the rows it set out to
//! remove.

use mixengine_proto::privileged::PrivilegedOp;
use mixengine_proto::{Error, Removal, Residue, ResidueId};

/// Forget any waiting helper installation before an uninstall asks for the removal — roadmap task
/// **T88d**.
///
/// `Elevation::require_helper` puts `HelperInstall {}` in the queue at every daemon start, and
/// T88d's bootstrap adds one whenever a producer on a machine with no installed helper asks for
/// anything. Either can still be waiting when somebody presses Uninstall, and the batch would then
/// install the helper and remove it — the right end state, reached by doing and undoing work in
/// front of a person reading the list.
///
/// `HelperReplace {}` for the same reason: it exists to put a newer helper on a machine that is
/// about to have none.
///
/// **`pub(crate)` for its test**, which lives in [`crate::elevation`]'s test module because that is
/// where the fixture holding a queue and a machine is.
///
/// # Errors
///
/// The wire error of a queue that could not be read or written.
pub(crate) async fn drop_helper_installations(
    elevation: &crate::elevation::Elevation,
) -> Result<(), Error> {
    for waiting in elevation.status().await?.pending {
        if matches!(
            waiting.op,
            PrivilegedOp::HelperInstall {} | PrivilegedOp::HelperReplace {}
        ) {
            elevation
                .drop_pending(&mixengine_proto::ElevationDrop {
                    op: Some(waiting.id),
                })
                .await?;
        }
    }

    Ok(())
}

/// The second reading, paired with the first row by row — or a refusal when the two cannot be.
///
/// **Rows are paired by position**, as T87 always has. A process that started between the two
/// readings adds an in-use row the first did not have, and those rows are dropped here: the run
/// refused on any in-use row before it began, so a new one is somebody who arrived after the
/// check, and the rename on the way out refuses rather than half-deletes if they are still there.
/// Anything else that makes the readings disagree is a machine that changed underneath the
/// uninstall, and mis-pairing its rows would report one thing's outcome against another's name.
pub(super) fn aligned(
    planned: &[Residue],
    mut measured: Vec<Residue>,
) -> Result<Vec<Residue>, Error> {
    measured.retain(|row| row.id != ResidueId::InUse);

    let agrees = planned.len() == measured.len()
        && planned
            .iter()
            .zip(&measured)
            .all(|(before, after)| before.id == after.id);

    match agrees {
        true => Ok(measured),
        false => Err(Error::new(
            mixengine_proto::ErrorCode::Internal,
            "this machine changed while it was being uninstalled; run the uninstall again",
        )),
    }
}

/// The second reading, with each privileged row replaced by its settled outcome.
pub(super) fn merge(measured: Vec<Residue>, privileged: Vec<Residue>) -> Vec<Residue> {
    let mut settled = privileged.into_iter();

    measured
        .into_iter()
        .map(|row| match needs_the_helper(row.id) {
            true => settled.next().unwrap_or(row),
            false => row,
        })
        .collect()
}

/// What became of a credential row, from how many keys there were and how many the store still
/// lists — T182d. Measured, like every other row.
pub(super) fn forgotten(before: usize, left: mixengine_platform::Result<usize>) -> Removal {
    match left {
        Ok(0) => Removal::Removed {
            what: format!("forgot {before}"),
        },
        Ok(left) => Removal::Failed {
            because: format!("{left} of {before} are still stored"),
        },
        // The whole chain, down to the store's own code: "cannot forget the credential" alone
        // says nothing a person can act on, and the T182a walk had to find the cause elsewhere.
        Err(error) => Removal::Failed {
            because: format!(
                "the credential store could not be read back: {}",
                mixengine_proto::flatten(&error)
            ),
        },
    }
}

/// The home stays, so the passwords its data needs stay with it — T182d, D1.
pub(super) fn keep_credentials(items: &mut [Residue]) {
    for item in items.iter_mut() {
        if matches!(
            item.id,
            ResidueId::Credentials | ResidueId::WindowCredentials
        ) && matches!(item.outcome, Removal::Planned { .. })
        {
            item.outcome = Removal::Kept {
                because: "this home is kept, and the passwords its data needs are kept with it"
                    .to_owned(),
            };
        }
    }
}

/// Is this one of the rows the elevated helper answers for?
///
/// **The whole of what [`settle`] may be applied to.** The three that need no token are answered
/// where they are acted on, and the home is answered by `arm_the_home` — which runs *after* the
/// fold, so the home is still `Planned` when the fold sees it.
pub(super) fn needs_the_helper(id: ResidueId) -> bool {
    matches!(
        id,
        ResidueId::HostsBlock
            | ResidueId::ResolverWiring
            | ResidueId::PortAccess
            | ResidueId::FirewallRules
            | ResidueId::TrustStore
            | ResidueId::Package
            | ResidueId::PrivilegedHelper
            | ResidueId::AuditLog
    )
}

/// Did the plan say there was something of ours here?
/// Remove the recorded files from the program's directory, and say what is left — read back off
/// the disk rather than taken from `remove_file`'s answer, on this module's rule.
pub(crate) fn remove_completed(directory: &std::path::Path, recorded: &[String]) -> Removal {
    let stuck: Vec<String> = recorded
        .iter()
        .map(|name| directory.join(format!("{name}{}", std::env::consts::EXE_SUFFIX)))
        .filter(|file| {
            let _ = std::fs::remove_file(file);
            file.exists()
        })
        .map(|file| file.display().to_string())
        .collect();

    match stuck.is_empty() {
        true => Removal::Removed {
            what: "the files this install added to itself are gone".to_owned(),
        },
        false => Removal::Failed {
            because: format!("still there: {}", stuck.join(", ")),
        },
    }
}

pub(super) fn planned_row(planned: &[Residue], id: ResidueId) -> bool {
    planned
        .iter()
        .any(|row| row.id == id && matches!(row.outcome, Removal::Planned { .. }))
}

/// What became of one privileged row, from the second reading rather than from what was attempted.
///
/// **`Removed` is a measurement.** The helper is honest about what it did, but it is a separate
/// process describing finished work; this is a fresh reading of the thing itself, and it costs no
/// privilege for any row on the list (the T87 design, D3).
///
/// **And `OnRestart` is settled by the queue and the disk**, not by reading the helper's sentence: an
/// operation that is no longer waiting has been applied, so a file that is nonetheless still there is
/// one the operating system accepted and has not got to yet. That is Windows and the privileged
/// helper — a running image cannot be unlinked there — and it is the only row that can answer it.
pub(super) fn settle(
    before: Residue,
    after: Residue,
    granted: bool,
    waiting: &[mixengine_proto::PendingOp],
) -> Residue {
    let Removal::Planned { how } = &before.outcome else {
        // **Absent before and present after is a failure** (the T182b design, D7): the act left
        // this on the machine, and the second reading's `Planned` would have read as nothing
        // being wrong — which is how an uninstall that put the authority back reported success.
        if matches!(before.outcome, Removal::Absent {})
            && matches!(after.outcome, Removal::Planned { .. })
        {
            return Residue {
                outcome: Removal::Failed {
                    because: "this was not here before the uninstall and it is here now".to_owned(),
                },
                ..after
            };
        }

        // Kept or Failed before anything was attempted: the second reading is the answer.
        return after;
    };

    if matches!(after.outcome, Removal::Absent {}) {
        return Residue {
            outcome: Removal::Removed { what: how.clone() },
            ..after
        };
    }

    // the helper kept it for another home that asked after the plan was read. The second
    // reading says so, and it is neither a removal nor a failure.
    if matches!(after.outcome, Removal::Kept { .. }) {
        return after;
    }

    if !granted {
        return Residue {
            outcome: Removal::Enqueued { what: how.clone() },
            ..after
        };
    }

    let still_queued = waiting.iter().any(|pending| {
        matches!(
            (&pending.op, before.id),
            (PrivilegedOp::HostsApply { .. }, ResidueId::HostsBlock)
                | (
                    PrivilegedOp::ResolverRevoke { .. },
                    ResidueId::ResolverWiring
                )
                | (PrivilegedOp::PortAccessRevoke { .. }, ResidueId::PortAccess)
                | (PrivilegedOp::FirewallApply { .. }, ResidueId::FirewallRules)
                | (PrivilegedOp::TrustCaRemove { .. }, ResidueId::TrustStore)
                | (PrivilegedOp::HelperRemove {}, ResidueId::PrivilegedHelper)
                | (PrivilegedOp::PackageRemove {}, ResidueId::Package)
                | (PrivilegedOp::AuditLogRemove {}, ResidueId::AuditLog)
        )
    });

    let outcome = match (still_queued, before.id) {
        (false, ResidueId::PrivilegedHelper) => Removal::OnRestart {
            what: format!(
                "this system accepted the removal and will carry it out {}, because a program \
                 cannot be deleted while it is running",
                mixengine_proto::privileged::AT_NEXT_RESTART
            ),
        },
        (false, _) => Removal::Failed {
            because: "the operation was applied and this is still here".to_owned(),
        },
        (true, _) => Removal::Failed {
            because: "this is still here, and the operation that removes it is still waiting for \
                      permission"
                .to_owned(),
        },
    };

    Residue { outcome, ..after }
}
