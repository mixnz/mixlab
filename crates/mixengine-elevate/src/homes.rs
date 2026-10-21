//! Which homes this helper serves, and whether a removal would take something another one uses —
//! roadmap task **T211g**, ADR 0066.
//!
//! Every batch records its home in the ledger before any operation. `helper-remove` and
//! `audit-log-remove` then ask [`Batch::kept`] first: the helper and its log are the machine's, and
//! one home's uninstall removes them only when no other home in the ledger still exists. The answer
//! is decided here, from the disk, and not from the daemon's plan.

use std::path::Path;

use mixengine_proto::privileged::{HomesLedger, OpOutcome};

/// What this batch knows about the homes the helper serves.
#[derive(Debug)]
pub(crate) struct Batch {
    /// Whether the ledger was there before this batch recorded its home.
    existed: bool,

    /// The ledger after this batch's record, or why it could not be read.
    ledger: Result<Option<HomesLedger>, String>,
}

impl Batch {
    /// Record `home` in the ledger in `directory`, then read it back.
    ///
    /// **Never fails the batch.** A ledger that cannot be written costs a line on stderr, as an
    /// audit line does, and leaves a batch that keeps the helper and the log.
    pub(crate) fn begin(directory: &Path, home: &Path, caller: &str, now: u64) -> Self {
        let existed = match mixengine_platform::ledger::record(directory, home, caller, now) {
            Ok(recorded) => recorded.existed_before,
            Err(error) => {
                eprintln!(
                    "mixengine-elevate: cannot record this home in the ledger: {}",
                    mixengine_proto::flatten(&error)
                );
                false
            }
        };

        Self {
            existed,
            ledger: mixengine_platform::ledger::read(directory)
                .map_err(|error| mixengine_proto::flatten(&error)),
        }
    }

    /// A batch that recorded nothing — one with no token, which removes nothing either.
    pub(crate) fn unrecorded() -> Self {
        Self {
            existed: false,
            ledger: Ok(None),
        }
    }

    /// Why `what` stays for another home, as the outcome to answer with — or [`None`] when this home
    /// is the last one and it may go.
    ///
    /// `manual` is what a person would do by hand once no other home needs it.
    pub(crate) fn kept(
        &self,
        home: &Path,
        what: &str,
        manual: String,
        alive: impl Fn(&Path) -> bool,
    ) -> Option<OpOutcome> {
        let keep = |reason: String| {
            Some(OpOutcome::Unmanaged {
                reason,
                manual: manual.clone(),
            })
        };

        let ledger = match &self.ledger {
            Err(why) => {
                return keep(format!(
                    "{what} stays: the record of which homes use it could not be read ({why})"
                ));
            }
            Ok(None) => None,
            Ok(Some(ledger)) => Some(ledger),
        };

        let (true, Some(ledger)) = (self.existed, ledger) else {
            return keep(format!(
                "{what} stays: this machine has no record yet of which other homes use it"
            ));
        };

        let others = ledger.living_others(home, alive);

        match others.is_empty() {
            true => None,
            false => keep(format!(
                "{what} stays, because {} other home(s) on this machine still use it: {}",
                others.len(),
                others
                    .iter()
                    .map(|other| other.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use mixengine_proto::privileged::LedgerEntry;

    use super::*;

    fn batch(existed: bool, homes: &[&str]) -> Batch {
        Batch {
            existed,
            ledger: Ok(Some(HomesLedger {
                homes: homes
                    .iter()
                    .map(|home| LedgerEntry {
                        home: PathBuf::from(home),
                        caller: "uid 501".to_owned(),
                        last: 0,
                    })
                    .collect(),
            })),
        }
    }

    fn reason(outcome: Option<OpOutcome>) -> String {
        match outcome {
            Some(OpOutcome::Unmanaged { reason, .. }) => reason,
            other => panic!("expected the helper to keep it, got {other:?}"),
        }
    }

    /// The case T211g was found on: a second home still uses the helper.
    #[test]
    fn the_helper_keeps_itself_while_another_home_lives() {
        let kept = batch(true, &["/homes/a", "/homes/b"]).kept(
            Path::new("/homes/a"),
            "the privileged helper",
            "remove it".to_owned(),
            |_| true,
        );

        assert!(reason(kept).contains("/homes/b"));
    }

    /// A ledger this batch created has seen only this home.
    #[test]
    fn a_ledger_this_batch_created_keeps_both() {
        let kept = batch(false, &["/homes/a"]).kept(
            Path::new("/homes/a"),
            "the audit log",
            "remove it".to_owned(),
            |_| true,
        );

        assert!(!reason(kept).is_empty());
    }

    /// Keeping a file is recoverable; removing another home's is not.
    #[test]
    fn an_unreadable_ledger_keeps_both() {
        let unreadable = Batch {
            existed: true,
            ledger: Err("not JSON".to_owned()),
        };

        let kept = unreadable.kept(
            Path::new("/homes/a"),
            "the audit log",
            "remove it".to_owned(),
            |_| true,
        );

        assert!(reason(kept).contains("not JSON"));
    }

    /// The last home goes with what it used, and a home that is gone is nobody's reason to stay.
    #[test]
    fn the_last_living_home_removes_them() {
        let alone = batch(true, &["/homes/a", "/homes/gone"]);

        assert!(
            alone
                .kept(
                    Path::new("/homes/a"),
                    "the privileged helper",
                    "remove it".to_owned(),
                    |home| home != Path::new("/homes/gone"),
                )
                .is_none()
        );
    }

    /// The batch records its home before anything else, and a second batch finds the ledger there.
    #[test]
    fn a_batch_records_its_home_first() {
        let directory = tempfile::TempDir::new().expect("a directory");

        let first = Batch::begin(directory.path(), Path::new("/homes/a"), "uid 501", 1);
        let second = Batch::begin(directory.path(), Path::new("/homes/a"), "uid 501", 2);

        assert!(!first.existed);
        assert!(second.existed);
        let ledger = second.ledger.expect("readable").expect("there");
        assert_eq!(ledger.homes.len(), 1);
        assert_eq!(ledger.homes[0].home, PathBuf::from("/homes/a"));
    }
}
