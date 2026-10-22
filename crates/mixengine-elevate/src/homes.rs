//! Which homes this helper serves, and whether a removal would take something another one uses —
//! ADR 0066.
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

    /// Whether the audit log was there before this batch wrote a line to it.
    log_existed: bool,

    /// The ledger after this batch's record, or why it could not be read.
    ledger: Result<Option<HomesLedger>, String>,
}

impl Batch {
    /// Record `home` in the ledger in `directory`, then read it back.
    ///
    /// **Never fails the batch.** A ledger that cannot be written costs a line on stderr, as an
    /// audit line does, and leaves a batch that keeps the helper and the log.
    pub(crate) fn begin(directory: &Path, home: &Path, caller: &str, now: u64) -> Self {
        // Before the record below, and before any operation writes a line.
        let log_existed = directory.join(crate::audit::FILE_NAME).exists();

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
            log_existed,
            ledger: mixengine_platform::ledger::read(directory)
                .map_err(|error| mixengine_proto::flatten(&error)),
        }
    }

    /// A batch that recorded nothing — one with no token, which removes nothing either.
    pub(crate) fn unrecorded() -> Self {
        Self {
            existed: false,
            log_existed: false,
            ledger: Ok(None),
        }
    }

    /// [`kept`](Self::kept) for the audit log, which may also go when this batch made it.
    ///
    /// **Found on the server M34 ran on**: an uninstall with something to revoke, on a machine
    /// whose log and ledger an earlier uninstall removed, wrote both afresh and then kept the log
    /// as one other homes might use, so the uninstall reported it left behind and kept the home.
    /// With neither there before this batch, everything in them is this batch's own. The helper
    /// keeps the cautious rule: a package may have placed it for a home that never asked for
    /// anything.
    pub(crate) fn kept_log(
        &self,
        home: &Path,
        manual: String,
        alive: impl Fn(&Path) -> bool,
    ) -> Option<OpOutcome> {
        if !self.existed && !self.log_existed {
            return None;
        }

        self.kept(home, "the audit log", manual, alive)
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
            log_existed: true,
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

    /// The case this was found on, a Mac with two homes: a second home still uses the helper.
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
            log_existed: true,
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

    /// A log and a ledger this batch made hold nothing of another home's, so the log goes.
    #[test]
    fn a_log_this_batch_made_goes() {
        let fresh = Batch {
            log_existed: false,
            ..batch(false, &["/homes/a"])
        };

        assert!(
            fresh
                .kept_log(Path::new("/homes/a"), "remove it".to_owned(), |_| true)
                .is_none()
        );
    }

    /// A log that was there before a ledger existed may hold another home's lines: it stays.
    #[test]
    fn a_log_older_than_the_ledger_stays() {
        let kept = batch(false, &["/homes/a"]).kept_log(
            Path::new("/homes/a"),
            "remove it".to_owned(),
            |_| true,
        );

        assert!(reason(kept).contains("no record yet"));
    }

    /// The helper keeps the cautious rule whatever the log says.
    #[test]
    fn a_fresh_batch_still_keeps_the_helper() {
        let fresh = Batch {
            log_existed: false,
            ..batch(false, &["/homes/a"])
        };

        let kept = fresh.kept(
            Path::new("/homes/a"),
            "the privileged helper",
            "remove it".to_owned(),
            |_| true,
        );

        assert!(!reason(kept).is_empty());
    }

    /// `begin` reads whether the log was there before anything in the batch wrote to it.
    #[test]
    fn a_batch_knows_whether_the_log_was_there() {
        let directory = tempfile::TempDir::new().expect("a directory");

        let before = Batch::begin(directory.path(), Path::new("/homes/a"), "uid 501", 1);
        std::fs::write(directory.path().join(crate::audit::FILE_NAME), "{}\n").expect("a log");
        let after = Batch::begin(directory.path(), Path::new("/homes/a"), "uid 501", 2);

        assert!(!before.log_existed);
        assert!(after.log_existed);
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
