//! What may be written into the hosts file, decided by the binary that will write it.
//!
//! **The helper validates the request itself rather than trusting the daemon** —
//! `docs/architecture/security-model.md`. If the daemon is compromised it *is* the attacker, so
//! nothing the request asserts can be believed: not the address, not the domain, not the size of
//! the list. This module is the whole of that decision and is meant to be read in one sitting.
//!
//! The table it reads is a compile-time constant shared through `mixengine-proto`. Being handed
//! a list of permitted TLDs *in the request* would be the trust this rules out; sharing a constant
//! is not. This binary is excluded from auto-update, so its table can be older than the daemon's —
//! and that is the correct failure: a TLD a future build manages is refused here, loudly, at its own
//! index.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use std::path::Path;

use mixengine_platform::hosts::{self, Change};
use mixengine_proto::domains::{MANAGED_TLDS, is_domain_syntax};
use mixengine_proto::privileged::{HomesLedger, HostEntry, OpOutcome};

/// The two addresses a managed name may point at.
///
/// Not `IpAddr::is_loopback`, which would accept `127.0.0.2`: nothing MixEngine does needs one, and
/// an address this file did not name is precisely the hosts-file hijack Defender has a heuristic for.
const PERMITTED: [IpAddr; 2] = [
    IpAddr::V4(Ipv4Addr::LOCALHOST),
    IpAddr::V6(Ipv6Addr::LOCALHOST),
];

/// The most names one request may point at loopback.
const LIMIT: usize = 512;

/// Validate, apply, and say what happened.
///
/// `home` is the requesting home as `crate::request` resolved it; its tag is derived here, from
/// that path, rather than taken from the daemon (ADR 0066).
pub(crate) fn apply(tag: Option<&str>, entries: &[HostEntry], home: &Path) -> OpOutcome {
    if let Some(reason) = refusal(entries) {
        return OpOutcome::Refused { reason };
    }

    let ours = hosts::tag_for(home);

    // Read only when the tag is not this home's: the ordinary write never needs the ledger, and a
    // ledger that cannot be read must not stop a home writing its own block.
    let ledger = match tag {
        Some(tag) if tag != ours => mixengine_platform::elevated::audit_directory()
            .and_then(|directory| mixengine_platform::ledger::read(&directory))
            .ok()
            .flatten(),
        _ => None,
    };

    if let Some(reason) = whose(tag, entries, &ours, ledger.as_ref(), |path| {
        mixengine_platform::ledger::alive(path)
    }) {
        return OpOutcome::Refused { reason };
    }

    let file = hosts::path();

    match hosts::apply(&file, tag, entries) {
        Ok(Change::Unchanged) => OpOutcome::AlreadyDone,
        Ok(Change::Written { entries: 0 }) => OpOutcome::Applied {
            detail: format!("removed MixEngine's block from {}", file.display()),
        },
        Ok(Change::Written { entries }) => OpOutcome::Applied {
            detail: format!(
                "wrote {entries} name{} into {}",
                if entries == 1 { "" } else { "s" },
                file.display()
            ),
        },
        // What is wrong is on the machine and a person has to look at it, so the same request will
        // be refused again — which is exactly what `Refused` says.
        Err(error @ mixengine_platform::Error::MalformedBlock { .. }) => OpOutcome::Refused {
            reason: error.to_string(),
        },
        // A held lock, or an OS that said no. Nothing about the request is wrong.
        // **`flatten` and not `to_string`.** `Error::Os` renders as "cannot <action>" and keeps the
        // operating system's own words as its `#[source]`, so `to_string` alone hands back a
        // sentence with the cause cut off — which is the half a person needs. `mix` already
        // flattens the same errors at its own boundary; this is that boundary for the helper.
        Err(error) => OpOutcome::Failed {
            message: mixengine_proto::flatten(&error),
        },
    }
}

/// Why this block is not the requesting home's to write, or [`None`] — ADR 0066.
///
/// `ours` is the requesting home's tag. Another tag is accepted only to empty a block whose home
/// `ledger` names and `alive` says is gone; the legacy block (`None`) only ever to empty it.
fn whose(
    tag: Option<&str>,
    entries: &[HostEntry],
    ours: &str,
    ledger: Option<&HomesLedger>,
    alive: impl Fn(&Path) -> bool,
) -> Option<String> {
    let Some(tag) = tag else {
        return (!entries.is_empty()).then(|| {
            "the hosts block MixEngine wrote before each home kept its own can only be removed"
                .to_owned()
        });
    };

    if !hosts::is_tag(tag) {
        return Some(format!("`{tag}` is not the tag of a home's hosts block"));
    }

    if tag == ours {
        return None;
    }

    if !entries.is_empty() {
        return Some("only the requesting home's own hosts block may be written".to_owned());
    }

    let Some(ledger) = ledger else {
        return Some(
            "no record says which home this hosts block belongs to, so it is left as it is"
                .to_owned(),
        );
    };

    match ledger
        .homes
        .iter()
        .find(|entry| hosts::tag_for(&entry.home) == tag)
    {
        Some(entry) if alive(&entry.home) => Some(format!(
            "this hosts block belongs to {}, which is still there",
            entry.home.display()
        )),
        Some(_) => None,
        None => Some(
            "no home this helper has served owns this hosts block, so it is left as it is"
                .to_owned(),
        ),
    }
}

/// Why these entries will not be written, or [`None`].
fn refusal(entries: &[HostEntry]) -> Option<String> {
    if entries.len() > LIMIT {
        return Some(format!(
            "{} entries is more than the {LIMIT} this helper will write",
            entries.len()
        ));
    }

    for entry in entries {
        if !PERMITTED.contains(&entry.address) {
            return Some(format!(
                "{} is not a loopback address; only 127.0.0.1 and ::1 may be written",
                entry.address
            ));
        }

        if !is_domain_syntax(&entry.domain) {
            return Some(format!("`{}` is not a domain name", entry.domain));
        }

        let tld = entry.domain.rsplit('.').next().unwrap_or_default();

        if !MANAGED_TLDS.contains(&tld) {
            return Some(format!(
                "`{}` is outside the TLDs this helper manages ({})",
                entry.domain,
                MANAGED_TLDS.join(", ")
            ));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixengine_proto::privileged::{HomesLedger, LedgerEntry};

    fn entry(address: &str, domain: &str) -> HostEntry {
        HostEntry {
            address: address.parse().expect("a literal address"),
            domain: domain.to_owned(),
        }
    }

    /// The whole of the policy, row by row. Each line is something a compromised daemon would ask
    /// for and this binary is the last thing between it and `/etc/hosts`.
    #[test]
    fn what_this_helper_will_not_write() {
        for (bad, what) in [
            (entry("8.8.8.8", "blog.test"), "loopback"),
            (entry("127.0.0.2", "blog.test"), "loopback"),
            (entry("127.0.0.1", "evil.com"), "manages"),
            (entry("127.0.0.1", "BLOG.TEST"), "domain"),
            (entry("127.0.0.1", "*.blog.test"), "domain"),
            (entry("127.0.0.1", ""), "domain"),
        ] {
            let reason = refusal(std::slice::from_ref(&bad))
                .unwrap_or_else(|| panic!("{bad:?} was accepted"));

            assert!(reason.contains(what), "{reason}");
        }
    }

    /// An unbounded list from a compromised daemon is a denial of service against every name lookup
    /// the machine makes, and it costs one comparison to make unreachable.
    #[test]
    fn more_entries_than_anybody_needs_is_refused() {
        let many: Vec<HostEntry> = (0..=LIMIT)
            .map(|index| entry("127.0.0.1", &format!("s{index}.test")))
            .collect();

        assert!(refusal(&many).is_some());
        assert!(refusal(&many[..LIMIT]).is_none());
    }

    /// A refusal test that never says yes proves only that the code refuses.
    #[test]
    fn what_this_helper_will_write() {
        let good = [
            entry("127.0.0.1", "blog.test"),
            entry("127.0.0.1", "api.blog.test"),
            entry("127.0.0.1", "x.localhost"),
            entry("127.0.0.1", "printer.local"),
            // Permitted so the daemon can start emitting it without touching this audited binary.
            entry("::1", "blog.test"),
        ];

        assert_eq!(refusal(&good), None);
        assert_eq!(refusal(&[]), None, "an empty list removes the block");
    }

    /// The tag a test's own home would have — any 16 lowercase hex digits.
    const OURS: &str = "0123456789abcdef";

    /// A ledger holding one other home, at `/homes/b`.
    fn ledger_with_b() -> HomesLedger {
        HomesLedger {
            homes: vec![LedgerEntry {
                home: std::path::PathBuf::from("/homes/b"),
                caller: "uid 502".to_owned(),
                last: 0,
            }],
        }
    }

    /// ADR 0066: a tag is a hash's hex digits and nothing else may reach a marker line.
    #[test]
    fn a_tag_that_is_not_hex_is_refused() {
        for bad in ["../x", "ABC", "", "a\nb", "0123456789abcdeg"] {
            assert!(
                whose(Some(bad), &[], OURS, None, |_| true).is_some(),
                "{bad:?} was accepted"
            );
        }
    }

    /// This home's own block is the ordinary case, written or emptied.
    #[test]
    fn this_homes_own_block_may_be_written() {
        assert_eq!(
            whose(
                Some(OURS),
                &[entry("127.0.0.1", "blog.test")],
                OURS,
                None,
                |_| true
            ),
            None
        );
        assert_eq!(whose(Some(OURS), &[], OURS, None, |_| true), None);
    }

    /// The block every home shared before each kept its own is drained and removed, never written again.
    #[test]
    fn the_legacy_block_can_only_be_emptied() {
        assert!(
            whose(None, &[entry("127.0.0.1", "blog.test")], OURS, None, |_| {
                true
            })
            .is_some()
        );
        assert_eq!(whose(None, &[], OURS, None, |_| true), None);
    }

    /// A daemon cannot reach another living home's names by naming that home's tag.
    #[test]
    fn another_living_homes_block_is_refused() {
        let ledger = ledger_with_b();
        let theirs = hosts::tag_for(std::path::Path::new("/homes/b"));

        let reason = whose(Some(&theirs), &[], OURS, Some(&ledger), |_| true)
            .expect("refused while that home exists");

        assert!(reason.contains("/homes/b"), "{reason}");
    }

    /// A block whose home the ledger says is gone may be emptied by anyone the helper serves —
    /// emptied only, and only when the ledger can place it.
    #[test]
    fn an_orphan_block_may_be_emptied() {
        let ledger = ledger_with_b();
        let theirs = hosts::tag_for(std::path::Path::new("/homes/b"));

        assert_eq!(
            whose(Some(&theirs), &[], OURS, Some(&ledger), |_| false),
            None
        );
        assert!(
            whose(
                Some(&theirs),
                &[entry("127.0.0.1", "x.test")],
                OURS,
                Some(&ledger),
                |_| false
            )
            .is_some(),
            "an orphan's block is never written"
        );
        assert!(
            whose(Some("fedcba9876543210"), &[], OURS, Some(&ledger), |_| {
                false
            })
            .is_some(),
            "a tag the ledger cannot place is nobody's to remove"
        );
        assert!(
            whose(Some(&theirs), &[], OURS, None, |_| false).is_some(),
            "no ledger, no orphan"
        );
    }
}
