//! What a machine lacks before an install or an apply, and whether the person agreed to fixing it
//! first — roadmap task **T151**.

use std::io::Write as _;

use mixengine_proto::{Error, Remedy, Requirement, Requirements};

use crate::client::{Client, ask};
use crate::confirm::unanswered;
use crate::{confirm, render};

/// What somebody said in advance about a machine that lacks something — roadmap task **T151**.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Agreement {
    /// `--yes`: install what MixEngine can install, without asking.
    pub(crate) yes: bool,

    /// `--ignore-requirements`: do not judge at all.
    pub(crate) ignore_requirements: bool,
}

/// Whether to install what the machine lacks first, given what the daemon judged.
///
/// `Ok(Some(agreed))` goes on to the install; `Ok(None)` is a person who answered no. **Only an
/// installable lack is asked about**: one no installer fixes is the daemon's to refuse, in its own
/// sentence, and asking about it here would be a second wording of one refusal.
pub(crate) fn agreed_to_prerequisites(
    unmet: &[Requirement],
    yes: bool,
    json: bool,
) -> Result<Option<bool>, Error> {
    // **Said before anything is decided, and it decides nothing** — roadmap task **T27e**, D16: a
    // library the distribution provides is the person's to install, and the runtime installs
    // meanwhile. On `--json` it is left out, because the same list is in what the daemon answered.
    let advisories: Vec<Requirement> = unmet
        .iter()
        .filter(|requirement| matches!(requirement.remedy, Remedy::InstallFromDistribution))
        .cloned()
        .collect();
    if !advisories.is_empty() && !json {
        let _ = write!(std::io::stderr(), "{}", render::advisories(&advisories));
    }

    let installable = unmet
        .iter()
        .any(|requirement| matches!(requirement.remedy, Remedy::InstallVisualCpp { .. }));
    let blocked = unmet.iter().any(|requirement| {
        !matches!(
            requirement.remedy,
            Remedy::InstallVisualCpp { .. } | Remedy::InstallFromDistribution
        )
    });

    if !installable || blocked {
        return Ok(Some(false));
    }
    if yes {
        return Ok(Some(true));
    }
    if json {
        return Err(unanswered());
    }

    match confirm::ask(&format!(
        "{}install it first? Windows may ask for administrator approval. [y/N] ",
        render::requirements(unmet)
    )) {
        confirm::Answer::Yes => Ok(Some(true)),
        confirm::Answer::No => {
            let _ = writeln!(std::io::stderr(), "nothing was installed");
            Ok(None)
        }
        confirm::Answer::Unanswerable => Err(unanswered()),
    }
}

/// Ask the daemon what `target` lacks, then [`agreed_to_prerequisites`].
///
/// **A question the daemon cannot answer asks nothing.** A version the index does not publish, or an
/// index that cannot be read, is the install's to report — as a job that fails in its own words — and
/// refusing it here first would be the same failure said twice, the second time more vaguely.
pub(crate) async fn prerequisites_agreed(
    client: &mut Client,
    method: &str,
    target: Option<serde_json::Value>,
    agreement: Agreement,
    json: bool,
) -> Result<Option<bool>, Error> {
    if agreement.ignore_requirements {
        return Ok(Some(false));
    }

    match ask::<Requirements>(client, method, target).await {
        Ok(judged) => agreed_to_prerequisites(&judged.unmet, agreement.yes, json),
        Err(_) => Ok(Some(false)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixengine_proto::PackageVersion;

    /// **T151.** Nothing is asked, and nothing refused here, unless MixEngine could install it.
    #[test]
    fn only_an_installable_lack_is_asked_about() {
        use mixengine_proto::{Need, RedistributableArch};

        let installable = Requirement {
            need: Need::VisualCpp {
                year: "2019".to_owned(),
                arch: RedistributableArch::X64,
                found: None,
            },
            remedy: Remedy::InstallVisualCpp {
                arch: RedistributableArch::X64,
            },
        };
        let escapable = Requirement {
            need: Need::Glibc {
                at_least: "2.34".to_owned(),
                found: "2.31".to_owned(),
            },
            remedy: Remedy::ChooseVersion {
                version: PackageVersion::parse("8.3.33").unwrap(),
            },
        };

        assert_eq!(
            agreed_to_prerequisites(&[], false, true).unwrap(),
            Some(false)
        );
        assert_eq!(
            agreed_to_prerequisites(&[escapable], false, true).unwrap(),
            Some(false)
        );
        assert_eq!(
            agreed_to_prerequisites(std::slice::from_ref(&installable), true, true).unwrap(),
            Some(true)
        );
        assert!(
            agreed_to_prerequisites(&[installable], false, true).is_err(),
            "--json needs --yes"
        );

        // **A library the distribution provides asks nothing** — roadmap task T27e, D16.
        let advisory = Requirement {
            need: Need::SharedLibrary {
                soname: "libasound.so.2".to_owned(),
            },
            remedy: Remedy::InstallFromDistribution,
        };
        assert_eq!(
            agreed_to_prerequisites(&[advisory], false, true).unwrap(),
            Some(false),
            "it goes on, and nothing was agreed to"
        );
    }
}
