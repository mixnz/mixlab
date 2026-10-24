//! Windows: MixEngine does not search browser certificate databases here.
//!
//! **This says what MixEngine did and not what Firefox reads.** Firefox on Windows may well read
//! the machine's own store, through `security.enterprise_roots`; no machine available when this was
//! written had one installed, so nothing here claims it either way.
//! `docs/specs/2026-08-25-t49b-nss-databases-design.md`, D14, records how to find out, which is one
//! `about:config` entry away for anybody who has one.
//!
//! It is also a mechanism decision and not only a scope decision. Windows ships an unrelated
//! `certutil.exe` in `C:\WINDOWS\system32` — CryptoAPI's, with an entirely different command line —
//! so a `PATH`-resolved `certutil` here finds the wrong program. Confining the search to Linux
//! means that collision never arises.

use crate::{BrowserChange, BrowserSurvey, BrowserTrust, Result};

/// This system's answer.
#[derive(Debug, Default)]
pub(crate) struct Browsers;

impl BrowserTrust for Browsers {
    fn survey(&self, _der: &[u8]) -> Result<BrowserSurvey> {
        Ok(BrowserSurvey::NotSearched {
            because: "MixEngine does not search browser certificate databases on Windows; this \
                      machine's own trusted roots are what the line above reports"
                .to_owned(),
        })
    }

    /// Nothing is searched here, so there is nothing to write into — see the module header. Not a
    /// refusal: a machine with no databases has none to fail on.
    fn install(&self, _der: &[u8]) -> Result<BrowserChange> {
        Ok(BrowserChange::default())
    }

    /// And nothing to take out of.
    fn remove(&self, _key_id: &str) -> Result<BrowserChange> {
        Ok(BrowserChange::default())
    }
}
