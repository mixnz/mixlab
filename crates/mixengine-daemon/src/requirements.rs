//! What an install needs of this machine, answered before anybody has to ask twice — roadmap task
//! **T149**.
//!
//! **The judgement is core's** (`mixengine_core::requirements`), pure and tested there. What lives
//! here is what a daemon adds to it: reading the machine *now*, and turning a judgement into a
//! refusal before a job exists.

use mixengine_core::index::{Index, Target};
use mixengine_core::requirements;
use mixengine_platform::{MachineFacts, RedistributableOutcome};
use mixengine_proto::{Error, ErrorCode, Remedy, Requirement};

use crate::error::ToWire as _;
use crate::jobs::JobHandle;
use crate::runtimes::Fetcher;

/// This machine, now.
///
/// Read for every question rather than kept (T148 design, D1): somebody told to install a runtime
/// installs it and asks again, and a daemon that remembered the machine would refuse them again.
pub(crate) fn facts() -> MachineFacts {
    mixengine_platform::host().machine().facts()
}

/// What `kind` `version` lacks on this machine — empty when it lacks nothing, when it is not
/// published here, or when nothing could be judged.
pub(crate) fn of(
    index: &Index,
    kind: &str,
    version: &str,
    facts: &MachineFacts,
) -> Vec<Requirement> {
    Target::host()
        .and_then(|target| requirements::judge(index, target, kind, version, facts))
        .unwrap_or_default()
}

/// Who a refusal is about, and the command that installs another version of it.
#[derive(Debug)]
pub(crate) struct Subject {
    /// `php 8.4.24`, or `this blueprint`.
    pub(crate) name: String,

    /// `mix runtime install php` — the prefix a suggested version completes. [`None`] where no one
    /// command installs the thing refused, which is a blueprint.
    pub(crate) install: Option<String>,
}

/// The refusal an install gets before its job exists, or [`None`] when it may start.
///
/// **A remedy no installer can carry out refuses whatever was agreed**, because agreeing to install
/// a Visual C++ runtime does not make a glibc newer. An installable one refuses only without
/// `install_prerequisites`.
pub(crate) fn refusal(
    subject: &Subject,
    unmet: &[Requirement],
    install_prerequisites: bool,
) -> Option<Error> {
    // **A warning is said by whoever asked, and refuses nothing** — roadmap task **T27e**, D16. It
    // is dropped here rather than at each caller so that "what stands in the way" means one thing.
    let unmet: Vec<Requirement> = unmet
        .iter()
        .filter(|requirement| !requirements::is_advisory(requirement))
        .cloned()
        .collect();
    let unmet = unmet.as_slice();

    if unmet.is_empty() || (!requirements::blocks(unmet) && install_prerequisites) {
        return None;
    }

    let listed = unmet
        .iter()
        .map(|requirement| requirement.need.to_string())
        .collect::<Vec<_>>()
        .join("; ");
    let refused = Error::new(
        ErrorCode::DependencyMissing,
        format!("{} needs {listed}", subject.name),
    );

    if !requirements::blocks(unmet) {
        // A blueprint's apply answers this with its own flag, because it asks several questions.
        let agree = match subject.install {
            Some(_) => "--yes",
            None => "--install-prerequisites",
        };
        return Some(refused.with_hint(format!(
            "MixEngine can install it from Microsoft first; pass `{agree}`, or agree in MixLab; \
             pass `--ignore-requirements` if this machine has it some other way"
        )));
    }

    let suggested = unmet
        .iter()
        .find_map(|requirement| match &requirement.remedy {
            Remedy::ChooseVersion { version } => Some(version.as_str().to_owned()),
            _ => None,
        });

    Some(refused.with_hint(match (suggested, &subject.install) {
        (Some(version), Some(install)) => {
            format!("`{install} {version}` installs the newest release that runs on this machine")
        }
        (Some(version), None) => format!(
            "the newest release that runs on this machine is {version}, which the blueprint's \
             constraint would have to allow"
        ),
        (None, _) => "no release published for this machine runs on it".to_owned(),
    }))
}

/// Refuse `kind` `version` before its job exists, when this machine certainly lacks something.
///
/// **An index that cannot be read refuses nothing here**: the job reads it too and reports that in
/// its own words, and a refusal about the network dressed as one about the machine would be wrong.
pub(crate) async fn gate(
    fetcher: &Fetcher,
    kind: &str,
    version: &str,
    subject: &Subject,
    install_prerequisites: bool,
) -> Result<(), Error> {
    let Ok(catalogue) = fetcher.index.kinds(&[kind]).await else {
        return Ok(());
    };

    let unmet = of(&catalogue.index, kind, version, &facts());
    refusal(subject, &unmet, install_prerequisites).map_or(Ok(()), Err)
}

/// Install every redistributable `unmet` asks for, once each — roadmap task **T150**.
///
/// Called only inside a job, only after a person agreed. What it does not do is judge again; the
/// caller does that, on a machine read again, because an exit code is the installer's claim.
///
/// # Errors
///
/// `dependency_missing` for a declined dialog, a failed installer, a refused file or a download that
/// did not arrive; `internal` if the thread waiting on the installer is lost.
pub(crate) async fn satisfy(
    fetcher: &Fetcher,
    unmet: &[Requirement],
    handle: &JobHandle,
) -> Result<(), Error> {
    for arch in requirements::redistributables(unmet) {
        handle
            .progress(
                0,
                format!(
                    "downloading the Microsoft Visual C++ Redistributable ({arch}) from Microsoft"
                ),
            )
            .await;

        let installer = mixengine_core::prerequisites::download(&fetcher.installer, arch)
            .await
            .map_err(|error| error.to_wire())?;

        // **Said before it starts, because it cannot be taken back once it has** (D6): nothing here
        // may stop a process that holds administrator rights.
        handle
            .progress(
                0,
                format!(
                    "installing the Microsoft Visual C++ Redistributable ({arch}); Windows may \
                     ask for approval, and this step cannot be cancelled once it has started"
                ),
            )
            .await;

        let running = installer.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            mixengine_platform::host()
                .redistributables()
                .install_visual_cpp(&running)
        })
        .await
        .map_err(|error| {
            Error::new(
                ErrorCode::Internal,
                format!("the thread waiting on the installer was lost: {error}"),
            )
        })?
        .map_err(|error| Error::new(ErrorCode::DependencyMissing, error.to_string()));

        let _ = tokio::fs::remove_file(&installer).await;

        match outcome? {
            RedistributableOutcome::Installed | RedistributableOutcome::AlreadyNewer => {}
            RedistributableOutcome::RestartRequired => {
                handle
                    .progress(
                        0,
                        "the Visual C++ Redistributable is installed; Windows would like a restart \
                         when it suits you",
                    )
                    .await;
            }
            RedistributableOutcome::Declined => {
                return Err(Error::new(
                    ErrorCode::DependencyMissing,
                    "Windows' approval dialog was declined, so the Visual C++ Redistributable was \
                     not installed and nothing else was either",
                ));
            }
            RedistributableOutcome::Failed { code } => {
                return Err(Error::new(
                    ErrorCode::DependencyMissing,
                    format!(
                        "the Visual C++ Redistributable installer ended with exit code {code}, so \
                         nothing else was installed"
                    ),
                ));
            }
        }
    }

    Ok(())
}

/// Before an agreed install downloads anything: install what it lacks, then judge it again.
///
/// # Errors
///
/// What [`satisfy`] refuses with; the refusal of a machine that still lacks something afterwards;
/// and the wire error of an index that could not be obtained.
pub(crate) async fn prepare(
    fetcher: &Fetcher,
    kind: &str,
    version: &str,
    subject: &Subject,
    handle: &JobHandle,
) -> Result<(), Error> {
    let catalogue = fetcher
        .index
        .kinds(&[kind])
        .await
        .map_err(|error| error.to_wire())?;

    let unmet = of(&catalogue.index, kind, version, &facts());
    if !requirements::needs_consent(&unmet) {
        return Ok(());
    }

    satisfy(fetcher, &unmet, handle).await?;

    let still = of(&catalogue.index, kind, version, &facts());
    refusal(subject, &still, false).map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use mixengine_proto::{Need, PackageVersion, RedistributableArch};

    use super::*;

    fn subject() -> Subject {
        Subject {
            name: "php 8.4.24".to_owned(),
            install: Some("mix runtime install php".to_owned()),
        }
    }

    fn installable() -> Requirement {
        Requirement {
            need: Need::VisualCpp {
                year: "2022".to_owned(),
                arch: RedistributableArch::X64,
                found: None,
            },
            remedy: Remedy::InstallVisualCpp {
                arch: RedistributableArch::X64,
            },
        }
    }

    fn escapable() -> Requirement {
        Requirement {
            need: Need::Macos {
                at_least: "14.0".to_owned(),
                found: "13.6".to_owned(),
            },
            remedy: Remedy::ChooseVersion {
                version: PackageVersion::parse("8.3.33").expect("a version"),
            },
        }
    }

    fn advisory() -> Requirement {
        Requirement {
            need: Need::SharedLibrary {
                soname: "libasound.so.2".to_owned(),
            },
            remedy: mixengine_proto::Remedy::InstallFromDistribution,
        }
    }

    /// **A library the distribution provides refuses nothing** — roadmap task **T27e**, D16.
    #[test]
    fn a_missing_library_alone_refuses_nothing() {
        assert!(refusal(&subject(), &[advisory()], false).is_none());
        assert!(
            refusal(&subject(), &[advisory(), installable()], false).is_some(),
            "the Visual C++ runtime beside it still asks"
        );
        assert!(
            refusal(&subject(), &[advisory(), escapable()], true).is_some(),
            "a lack no installer fixes beside it still refuses"
        );
    }

    #[test]
    fn nothing_lacking_refuses_nothing() {
        assert!(refusal(&subject(), &[], false).is_none());
    }

    #[test]
    fn an_installable_lack_refuses_only_until_somebody_agrees() {
        let refused = refusal(&subject(), &[installable()], false).expect("not agreed yet");
        assert_eq!(refused.code, ErrorCode::DependencyMissing);
        assert!(
            refused
                .hint
                .as_deref()
                .is_some_and(|hint| hint.contains("--yes"))
        );

        assert!(refusal(&subject(), &[installable()], true).is_none());
    }

    #[test]
    fn a_lack_no_installer_fixes_refuses_whatever_was_agreed_and_names_the_way_out() {
        let refused = refusal(&subject(), &[installable(), escapable()], true).expect("blocked");
        assert!(
            refused
                .hint
                .as_deref()
                .is_some_and(|hint| hint.contains("mix runtime install php 8.3.33")),
            "{refused:?}"
        );
    }
}
