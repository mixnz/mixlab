//! A trust store that exists only in memory.

use crate::{Error, Result, TrustState, TrustStoreMethod};

/// What a test says this machine trusts.
#[derive(Debug)]
pub(crate) struct Trust {
    answer: std::result::Result<Answer, String>,

    /// What this machine's store holds, for the bundle the daemon writes out of it.
    ///
    /// **A count and not certificates**, because nothing here parses them: what a fixture has to be
    /// able to say is *how many* roots a machine offers, which is the one thing
    /// `mixengine_core::generate::ca` decides on — a store answering three is a store that was read
    /// wrong, and it writes no bundle. The bytes are made up, distinct, and never valid DER.
    roots: usize,
}

/// The two fields a fixture sets. `missing` is derived rather than given, for
/// [`super::resolver`]'s reason: a machine that does not hold the authority has one honest sentence
/// to say about it, and letting a fixture write a different one would let a test assert against a
/// machine none of the three could be.
#[derive(Debug, Clone, Copy)]
struct Answer {
    method: TrustStoreMethod,
    installed: bool,
}

impl Default for Trust {
    /// A machine with no store, trusting nothing.
    ///
    /// **Which is what every suite written before the system trust store was asked about should
    /// keep looking like.** They were written against a home nothing trusts, and a default that
    /// quietly held the authority would rewrite what they are about — the same reasoning as
    /// [`super::resolver`]'s default, and the same mistake it exists to prevent.
    fn default() -> Self {
        Self {
            answer: Ok(Answer {
                method: TrustStoreMethod::None,
                installed: false,
            }),
            roots: 0,
        }
    }
}

impl Trust {
    /// A machine using `method` that does or does not already hold the authority.
    pub(crate) fn holding(method: TrustStoreMethod, installed: bool) -> Self {
        Self {
            answer: Ok(Answer { method, installed }),
            roots: 0,
        }
    }

    /// The same machine, with `roots` certificates in its store.
    pub(crate) fn with_roots(mut self, roots: usize) -> Self {
        self.roots = roots;
        self
    }

    /// A machine that cannot say, with `reason`.
    pub(crate) fn refusing(reason: &str) -> Self {
        Self {
            answer: Err(reason.to_owned()),
            roots: 0,
        }
    }

    /// The fixture's answer, or the error it was built to give.
    fn answer(&self) -> Result<Answer> {
        self.answer
            .clone()
            .map_err(|reason| Error::UnsupportedPlatform {
                capability: "TrustStore",
                reason,
            })
    }
}

impl crate::TrustStore for Trust {
    fn method(&self) -> Result<TrustStoreMethod> {
        Ok(self.answer()?.method)
    }

    fn probe(&self, _der: &[u8]) -> Result<TrustState> {
        let answer = self.answer()?;

        let missing = (!answer.installed)
            .then(|| "this machine does not hold MixEngine's authority".to_owned());

        Ok(TrustState {
            method: answer.method,
            installed: answer.installed,
            missing,
        })
    }

    /// As many distinct made-up certificates as this fixture was built with.
    ///
    /// **Overridden rather than inherited**, because the default reads the machine the test is
    /// running on: a suite that asserted on the number of roots a CI runner happens to hold would
    /// be asserting about the runner.
    fn roots(&self) -> Result<Vec<Vec<u8>>> {
        self.answer()?;

        Ok((0..self.roots)
            .map(|index| format!("a made-up root, number {index}").into_bytes())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use crate::Host as _;

    use super::*;

    /// The fixture the daemon's own tests need, and which no machine in CI can be asked to be for a
    /// unit test — a runner that already trusted MixEngine would be a runner somebody had changed.
    #[test]
    fn a_mock_answers_what_it_was_built_with() {
        let host =
            crate::mock::Host::with_trust_store("/mixengine", TrustStoreMethod::SystemRoot, true);

        let state = host.trust_store().probe(&[1, 2, 3]).expect("a state");

        assert_eq!(state.method, TrustStoreMethod::SystemRoot);
        assert!(state.installed);
        assert_eq!(state.plan(&[1, 2, 3]), None);
    }

    /// A machine that has a store and does not hold it says so, and the planner asks.
    #[test]
    fn a_machine_that_does_not_hold_it_reports_the_gap() {
        let host = crate::mock::Host::with_trust_store(
            "/mixengine",
            TrustStoreMethod::SystemKeychain,
            false,
        );

        let state = host.trust_store().probe(&[1]).expect("a state");

        assert!(!state.installed);
        assert!(state.missing.is_some());
        assert!(state.plan(&[1]).is_some());
    }

    /// The default is a machine with no store, so every suite written before the system trust store
    /// was asked about keeps the shape it was written against.
    #[test]
    fn the_default_mock_has_no_store_and_asks_for_nothing() {
        let host = crate::mock::Host::with_home("/mixengine");

        let state = host.trust_store().probe(&[1]).expect("a state");

        assert_eq!(state.method, TrustStoreMethod::None);
        assert!(!state.installed);
        assert_eq!(state.plan(&[1]), None);
    }

    /// A machine that cannot be asked. Every caller treats this as "no answer" and carries on.
    #[test]
    fn a_mock_that_cannot_be_read_says_so() {
        let host =
            crate::mock::Host::unable_to_read_trust_store("/mixengine", "no store on this machine");

        assert!(host.trust_store().probe(&[1]).is_err());
        assert!(host.trust_store().method().is_err());
    }
}
