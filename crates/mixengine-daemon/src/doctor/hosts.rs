//! The hosts blocks other homes left behind — roadmap task **T211g**, D5.
//!
//! Every tagged block was written by a batch that recorded its home in the helper's ledger first,
//! so the ledger can say whose each block is. A block whose home is gone has nobody left to remove
//! it; `mix doctor` names it and `--repair` asks the helper to empty it, which the helper does only
//! after checking the same ledger itself.

use std::path::{Path, PathBuf};

use mixengine_proto::privileged::HomesLedger;
use mixengine_proto::{Check, Outcome, ProblemId};

use super::Doctor;

impl Doctor {
    /// **1a.** Blocks other homes wrote and left: their home is gone, and their names still point
    /// at loopback.
    pub(super) fn orphaned_hosts_blocks(&self) -> Check {
        let name = "hosts blocks other homes left behind".to_owned();

        let tags = match self.host.hosts_file().tags() {
            Ok(tags) => tags,
            Err(error) => {
                return Check {
                    name,
                    outcome: Outcome::Skipped {
                        because: format!("the hosts file could not be read: {error}"),
                    },
                };
            }
        };

        let found = orphans(&tags, ledger().as_ref(), mixengine_platform::ledger::alive);

        match found.is_empty() {
            true => Check {
                name,
                outcome: Outcome::Ok {},
            },
            false => Check {
                name,
                outcome: Outcome::Problem {
                    id: ProblemId::HostsBlockOrphaned,
                    because: format!(
                        "{} hosts block(s) belong to homes that are gone: {}",
                        found.len(),
                        found
                            .iter()
                            .map(|orphan| orphan.home.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                },
            },
        }
    }
}

/// The helper's ledger of homes, or [`None`] where there is none or it cannot be read — either way
/// nothing can be placed, so nothing is called an orphan.
pub(crate) fn ledger() -> Option<HomesLedger> {
    mixengine_platform::elevated::audit_directory()
        .ok()
        .and_then(|directory| mixengine_platform::ledger::read(&directory).ok().flatten())
}

/// A tagged block whose home the ledger names and the disk no longer has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Orphan {
    /// The block's tag.
    pub(crate) tag: String,

    /// The home that wrote it.
    pub(crate) home: PathBuf,
}

/// The blocks among `tags` that belong to a home `ledger` names and `alive` says is gone.
///
/// A tag the ledger cannot place is not an orphan: nobody can say whose it is, and the helper would
/// refuse to remove it.
pub(crate) fn orphans(
    tags: &[String],
    ledger: Option<&HomesLedger>,
    alive: impl Fn(&Path) -> bool,
) -> Vec<Orphan> {
    let Some(ledger) = ledger else {
        return Vec::new();
    };

    tags.iter()
        .filter_map(|tag| {
            ledger
                .homes
                .iter()
                .find(|entry| mixengine_platform::hosts::tag_for(&entry.home) == *tag)
                .filter(|entry| !alive(&entry.home))
                .map(|entry| Orphan {
                    tag: tag.clone(),
                    home: entry.home.clone(),
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use mixengine_proto::privileged::LedgerEntry;

    use super::*;

    fn ledger(homes: &[&str]) -> HomesLedger {
        HomesLedger {
            homes: homes
                .iter()
                .map(|home| LedgerEntry {
                    home: PathBuf::from(home),
                    caller: "uid 501".to_owned(),
                    last: 0,
                })
                .collect(),
        }
    }

    /// A block whose home is gone is an orphan; one whose home is there, or that no ledger can
    /// place, is not.
    #[test]
    fn a_block_with_no_living_home_is_orphaned() {
        let gone = mixengine_platform::hosts::tag_for(Path::new("/homes/gone"));
        let living = mixengine_platform::hosts::tag_for(Path::new("/homes/here"));
        let tags = vec![gone.clone(), living, "fedcba9876543210".to_owned()];

        let found = orphans(
            &tags,
            Some(&ledger(&["/homes/gone", "/homes/here"])),
            |home| home == Path::new("/homes/here"),
        );

        assert_eq!(
            found,
            vec![Orphan {
                tag: gone,
                home: PathBuf::from("/homes/gone"),
            }]
        );
    }

    /// Without a ledger, nothing can be placed and nothing is an orphan.
    #[test]
    fn no_ledger_finds_no_orphans() {
        let gone = mixengine_platform::hosts::tag_for(Path::new("/homes/gone"));

        assert!(orphans(&[gone], None, |_| false).is_empty());
    }
}
