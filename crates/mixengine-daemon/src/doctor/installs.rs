//! The checks about what is installed and what answers on the path: installs gone from
//! disk or never recorded, the Go and Java settings a home's toolchains read, and the
//! commands `bin/` fronts that something else on the path would answer first.

use std::ffi::OsStr;

use mixengine_proto::{Check, Outcome, ProblemId, RuntimeKind};

use super::Doctor;

impl Doctor {
    /// **What is recorded and not on disk.** A `Problem` for a folder that is gone, which
    /// `daemon.doctor_repair` leaves to a person; a `Note` for one this daemon cannot read, which
    /// may be a drive that is not mounted.
    pub(super) async fn missing_installs(&self) -> Check {
        let name = "installs recorded but not on disk".to_owned();

        let (runtimes, packages) = match (
            mixengine_core::runtimes::records(&self.store, None).await,
            mixengine_core::packages::records(&self.store, None).await,
        ) {
            (Ok(runtimes), Ok(packages)) => (runtimes, packages),
            (Err(error), _) | (_, Err(error)) => {
                return Check {
                    name,
                    outcome: Outcome::Skipped {
                        because: format!("this home's installs could not be read: {error}"),
                    },
                };
            }
        };

        let rows = runtimes
            .iter()
            .map(|runtime| {
                (
                    format!("{} {}", runtime.kind, runtime.version),
                    runtime.path.clone(),
                    runtime.is_missing(),
                )
            })
            .chain(packages.iter().map(|package| {
                (
                    format!("{} {}", package.package, package.version),
                    package.path.clone(),
                    package.is_missing(),
                )
            }));

        let (mut gone, mut out_of_reach) = (Vec::new(), Vec::new());
        for (what, path, missing) in rows {
            if missing {
                gone.push(format!("{what} ({path})"));
            } else if let Err(error) = tokio::fs::symlink_metadata(&path).await {
                out_of_reach.push(format!("{what} ({path}): {error}"));
            }
        }

        Check {
            name,
            outcome: missing_outcome(&gone, &out_of_reach),
        }
    }

    /// **What is on disk but not recorded.** A directory an earlier home installed that this one
    /// could not check. A `Note`: it costs disk and blocks an install of the same version, and
    /// nothing is broken by it. Walked at report time rather than remembered from the start, so a
    /// directory recorded or removed since is not reported.
    ///
    /// **And service data an earlier home left under `data/`**, counted here rather than in a check
    /// of its own, because both are the same leftover of one uninstall.
    pub(super) async fn unrecorded(&self) -> Check {
        let name = "installs on disk that this home has not recorded".to_owned();

        let installs = mixengine_core::adopt::walk::unrecorded(&self.store, &self.layout).await;
        let instances = mixengine_core::adopt::instances::found(
            &self.store,
            &self.layout,
            &crate::services::catalogue(),
        )
        .await;

        let (installs, instances) = match (installs, instances) {
            (Ok(installs), Ok(instances)) => (installs, instances),
            (Err(error), _) | (_, Err(error)) => {
                return Check {
                    name,
                    outcome: Outcome::Skipped {
                        because: format!("this home's installs could not be read: {error}"),
                    },
                };
            }
        };

        let mut said = Vec::new();
        if !installs.is_empty() {
            said.push(format!(
                "{} not recorded. `mix runtime adopt` or `mix package adopt` records one once it \
                 can be checked",
                installs
                    .iter()
                    .map(|found| found.path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !instances.is_empty() {
            said.push(format!(
                "{} service data director{} an earlier install left. `mix service found` lists \
                 them and `mix service adopt` brings one back",
                instances.len(),
                if instances.len() == 1 { "y" } else { "ies" }
            ));
        }

        Check {
            name,
            outcome: match said.is_empty() {
                true => Outcome::Ok {},
                false => Outcome::Note {
                    because: format!("{}; nothing here removes them", said.join("; ")),
                },
            },
        }
    }

    /// **Whether a pinned Go is the Go that builds.**
    ///
    /// The shim writes `GOTOOLCHAIN=local` for a `go` and leaves a value the session already carries
    /// alone, and it never sets `GOROOT`. So a daemon whose own environment carries either is a
    /// daemon whose children — a scaffold, a terminal it opened — get a `go` a `go.mod` can swap for
    /// another release, or one compiling with another Go's tools. A `Note`, for
    /// [`trust_bundle`](Self::trust_bundle)'s reason: the person set it, and nothing here repairs a
    /// choice.
    pub(super) async fn go_toolchain(&self) -> Check {
        let name = "the Go a project pins".to_owned();

        match mixengine_core::runtimes::records(&self.store, Some(RuntimeKind::Go)).await {
            Ok(installed) => Check {
                name,
                outcome: go_outcome(
                    !installed.is_empty(),
                    std::env::var_os("GOTOOLCHAIN").as_deref(),
                    std::env::var_os("GOROOT").as_deref(),
                ),
            },
            Err(error) => Check {
                name,
                outcome: Outcome::Skipped {
                    because: format!("this home's installed runtimes could not be read: {error}"),
                },
            },
        }
    }

    /// **Whether a pinned JDK is the JDK a build tool uses.**
    ///
    /// The shim overwrites `JAVA_HOME` for what it starts, and reaches nothing else: `mvn` and
    /// `./gradlew` typed in a terminal read the session's. The daemon's environment is the proxy for
    /// that session — a weak one where a login profile never reaches a user service, which is why the
    /// handbook carries the rule too. A `Note`, for [`go_toolchain`](Self::go_toolchain)'s reason.
    pub(super) async fn java_pin(&self) -> Check {
        let name = "the JDK a project pins".to_owned();

        match mixengine_core::runtimes::records(&self.store, Some(RuntimeKind::Java)).await {
            Ok(installed) => {
                let options: Vec<(&str, Option<std::ffi::OsString>)> =
                    ["JAVA_TOOL_OPTIONS", "_JAVA_OPTIONS", "JDK_JAVA_OPTIONS"]
                        .into_iter()
                        .map(|variable| (variable, std::env::var_os(variable)))
                        .collect();

                Check {
                    name,
                    outcome: java_outcome(
                        !installed.is_empty(),
                        &self.runtimes,
                        std::env::var_os("JAVA_HOME").as_deref(),
                        &options,
                    ),
                }
            }
            Err(error) => Check {
                name,
                outcome: Outcome::Skipped {
                    because: format!("this home's installed runtimes could not be read: {error}"),
                },
            },
        }
    }

    /// **What is on the PATH that MixEngine did not choose the name of.**
    ///
    /// Never a problem, always a `Note` or `Ok`, and deliberately: both conditions are somebody's
    /// own doing and neither is broken.
    ///
    /// - **A contested name.** A home with both MariaDB and MySQL installed has one `mysql`, and
    ///   the order that settled it is in `shims::resolve_claims`. Saying who won is the difference
    ///   between a person finding out here and finding out from `--version`.
    /// - **A discovered tool that shadows a program already on the PATH.** A globally installed
    ///   package called `git` puts a `git` ahead of the machine's own; every version manager that
    ///   fronts global tools has that property, and refusing it would be deciding on somebody's
    ///   behalf that a tool they installed is not one they meant.
    pub(super) async fn commands(&self) -> Check {
        let name = "the commands on this home's PATH".to_owned();

        let mut said = Vec::new();

        let claims = mixengine_core::services::client::claims(
            &self.store,
            &crate::services::spec::catalogue(),
            None,
        )
        .await;

        match claims {
            Ok(claims) => {
                for conflict in mixengine_core::shims::resolve_claims(&claims).1 {
                    said.push(format!(
                        "{} runs {}'s client, which {} also publishes",
                        conflict.name,
                        conflict.won,
                        conflict.lost.join(" and ")
                    ));
                }
            }
            Err(error) => {
                return Check {
                    name,
                    outcome: Outcome::Skipped {
                        because: format!(
                            "this home's installed packages could not be read: {error}"
                        ),
                    },
                };
            }
        }

        match mixengine_core::bin_commands::all(&self.store).await {
            Ok(found) => {
                for command in found.keys() {
                    if let Some(elsewhere) = elsewhere_on_the_path(command, &self.bin) {
                        said.push(format!(
                            "{command} was installed into a runtime and now comes before {}",
                            elsewhere.display()
                        ));
                    }
                }
            }
            Err(error) => said.push(format!(
                "the discovered commands could not be read: {error}"
            )),
        }

        // A language MixEngine fronts that the person also installed themselves.
        said.extend(runtimes_hidden(&self.bin, |name| {
            elsewhere_on_the_path(name, &self.bin)
        }));

        match said.is_empty() {
            true => Check {
                name,
                outcome: Outcome::Ok {},
            },
            false => Check {
                name,
                outcome: Outcome::Note {
                    because: said.join("; "),
                },
            },
        }
    }
}

/// Which of these rows claim a supervisor nobody is.
///
/// **Free and pure**, so the one decision in check 11 is tested without a database, a registry or a
/// daemon — `mixengine_platform`'s reserved-range parser one check along, and for the same reason.
/// Each item is an id and whether its row claims a supervisor at all; `held` is what this registry is
/// actually running.
/// Where else on this user's PATH a program of this name is, ahead of nothing.
///
/// Read off `PATH` rather than off the filesystem generally, because that is the question: a `git`
/// in `/usr/bin` matters and one in a directory nothing searches does not. `<root>/bin` itself is
/// skipped, since the name being asked about is the one this directory holds.
///
/// [`None`] when nothing else answers to it, which is every name on nearly every machine.
/// Each language name `bin/` fronts that is also installed further down the PATH, as a sentence
/// naming where. `elsewhere` answers that question for one name.
///
/// Without this a person who installed Node twice cannot see which one a terminal runs: `bin/` is
/// first on the PATH, so wherever MixEngine has installed a language its copy is the one that runs.
fn runtimes_hidden(
    bin: &std::path::Path,
    elsewhere: impl Fn(&str) -> Option<std::path::PathBuf>,
) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();

    mixengine_core::shims::COMMANDS
        .iter()
        .map(|command| command.name)
        .filter(|name| seen.insert(*name))
        .filter(|name| {
            bin.join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
                .is_file()
        })
        .filter_map(|name| {
            elsewhere(name).map(|theirs| {
                format!(
                    "{name}: bin/ runs MixEngine's; {} is further down the PATH",
                    theirs.display()
                )
            })
        })
        .collect()
}

fn elsewhere_on_the_path(command: &str, bin: &std::path::Path) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;

    // The spellings this system would try, which on Windows is the name plus each of the loader's
    // extensions. `mixengine_core::runtimes::globals` folds the same list on the way in.
    let spellings: Vec<String> = match cfg!(windows) {
        true => ["exe", "com", "bat", "cmd"]
            .into_iter()
            .map(|extension| format!("{command}.{extension}"))
            .collect(),
        false => vec![command.to_owned()],
    };

    std::env::split_paths(&path)
        .filter(|directory| directory != bin)
        .flat_map(|directory| {
            spellings
                .iter()
                .map(move |spelling| directory.join(spelling))
        })
        .find(|candidate| candidate.is_file())
}

/// What [`Doctor::missing_installs`] decides, as a function of what it read.
fn missing_outcome(gone: &[String], out_of_reach: &[String]) -> Outcome {
    match (gone.is_empty(), out_of_reach.is_empty()) {
        (true, true) => Outcome::Ok {},
        (false, _) => Outcome::Problem {
            id: ProblemId::InstallMissing,
            because: format!("recorded, and the folder is gone: {}", gone.join(", ")),
        },
        (true, false) => Outcome::Note {
            because: format!(
                "recorded, and the folder could not be read: {}",
                out_of_reach.join(", ")
            ),
        },
    }
}

/// What [`Doctor::go_toolchain`] decides, as a function of what it read.
///
/// `installed` is whether this home holds any Go; the two values are the daemon's own. Out here for
/// [`limit_outcome`]'s reason: every arm driven by a test, with no environment changed to do it.
///
/// [`limit_outcome`]: super::limit_outcome
fn go_outcome(installed: bool, toolchain: Option<&OsStr>, goroot: Option<&OsStr>) -> Outcome {
    if !installed {
        return Outcome::Ok {};
    }

    let mut said = Vec::new();

    // Empty is unset to Go, and `local` is what the shim would have written anyway.
    if let Some(value) = toolchain.filter(|value| !value.is_empty() && *value != "local") {
        said.push(format!("GOTOOLCHAIN={}", value.to_string_lossy()));
    }

    if let Some(value) = goroot.filter(|value| !value.is_empty()) {
        said.push(format!("GOROOT={}", value.to_string_lossy()));
    }

    match said.is_empty() {
        true => Outcome::Ok {},
        false => Outcome::Note {
            because: format!(
                "this daemon's own environment sets {}, and a go started through bin/ inherits it \
                 rather than running the Go its directory pins; unset it",
                said.join(" and ")
            ),
        },
    }
}

/// What [`Doctor::java_pin`] decides, as a function of what it read — [`go_outcome`]'s shape.
///
/// `installed` is whether this home holds any JDK, `runtimes` is its `runtimes/` directory, and the
/// rest are the daemon's own values.
fn java_outcome(
    installed: bool,
    runtimes: &std::path::Path,
    java_home: Option<&OsStr>,
    options: &[(&str, Option<std::ffi::OsString>)],
) -> Outcome {
    if !installed {
        return Outcome::Ok {};
    }

    let mut said = Vec::new();

    if let Some(value) = java_home.filter(|value| !value.is_empty())
        && !std::path::Path::new(value).starts_with(runtimes.join("java"))
    {
        said.push(format!(
            "JAVA_HOME={} (mvn and ./gradlew typed outside bin/ use that JDK)",
            value.to_string_lossy()
        ));
    }

    for (variable, value) in options {
        if value
            .as_deref()
            .is_some_and(|value| value.to_string_lossy().contains("javax.net.ssl.trustStore"))
        {
            said.push(format!(
                "{variable} naming javax.net.ssl.trustStore (every JVM reads that store instead of \
                 the cacerts MixEngine writes into)"
            ));
        }
    }

    match said.is_empty() {
        true => Outcome::Ok {},
        false => Outcome::Note {
            because: format!(
                "this daemon's own environment sets {}; unset it, or expect a JDK other than the \
                 pinned one",
                said.join(" and ")
            ),
        },
    }
}

#[cfg(test)]
mod shadowing_tests {
    use super::*;

    /// A language `bin/` fronts that is also installed further down the PATH is
    /// named, with where; a language `bin/` does not front, or that nothing else provides, is not.
    #[test]
    fn a_language_in_bin_that_hides_another_is_named() {
        let bin = tempfile::tempdir().expect("a bin/");
        let exe = |name: &str| format!("{name}{}", std::env::consts::EXE_SUFFIX);
        std::fs::write(bin.path().join(exe("node")), b"").expect("a node shim");
        std::fs::write(bin.path().join(exe("php")), b"").expect("a php shim");

        let theirs = std::path::PathBuf::from("/usr/local/bin/node");
        let said = runtimes_hidden(bin.path(), |name| (name == "node").then(|| theirs.clone()));

        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].starts_with("node:"), "{said:?}");
        assert!(said[0].contains("/usr/local/bin/node"), "{said:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A home with no Go is not asked about Go.** A variable nothing here reads is not news about
    /// this home.
    #[test]
    fn a_home_with_no_go_is_not_asked_about_go() {
        let outcome = super::go_outcome(false, Some(OsStr::new("auto")), None);

        assert!(matches!(outcome, Outcome::Ok {}), "{outcome:?}");
    }

    /// Nothing set, `local` set, or an empty value Go reads as unset: the shim's own answer wins.
    #[test]
    fn a_daemon_environment_that_leaves_the_pin_alone_is_ok() {
        for toolchain in [None, Some(OsStr::new("local")), Some(OsStr::new(""))] {
            let outcome = super::go_outcome(true, toolchain, None);

            assert!(
                matches!(outcome, Outcome::Ok {}),
                "{toolchain:?}: {outcome:?}"
            );
        }
    }

    /// **Gone is a problem, out of reach is a note.**
    #[test]
    fn a_gone_install_is_a_problem_and_one_out_of_reach_a_note() {
        assert!(matches!(super::missing_outcome(&[], &[]), Outcome::Ok {}));

        let gone = vec!["msys2 2026.10.08 (C:/h/packages/msys2/2026.10.08)".to_owned()];
        match super::missing_outcome(&gone, &[]) {
            Outcome::Problem { id, because } => {
                assert_eq!(id, ProblemId::InstallMissing);
                assert!(because.contains("msys2 2026.10.08"), "{because}");
            }
            other => panic!("{other:?}"),
        }

        let out_of_reach = vec!["ruby 3.4.11 (D:/r): access is denied".to_owned()];
        assert!(matches!(
            super::missing_outcome(&[], &out_of_reach),
            Outcome::Note { .. }
        ));
    }

    /// **A shim leaves a session's `GOTOOLCHAIN` alone**, so one inherited from the daemon's own
    /// environment is a pin a `go.mod` can step over — and the person is told which value is winning.
    #[test]
    fn a_toolchain_other_than_local_is_a_note_naming_it() {
        let outcome = super::go_outcome(true, Some(OsStr::new("auto")), None);

        let Outcome::Note { because } = outcome else {
            panic!("a pin that can be stepped over is news: {outcome:?}");
        };

        assert!(because.contains("GOTOOLCHAIN=auto"), "{because}");
    }

    /// A `GOROOT` naming another Go makes the pinned `go` compile with that Go's tools.
    #[test]
    fn a_goroot_is_a_note_naming_it() {
        let outcome = super::go_outcome(true, None, Some(OsStr::new("/usr/local/go")));

        let Outcome::Note { because } = outcome else {
            panic!("another Go's tools are news: {outcome:?}");
        };

        assert!(because.contains("GOROOT=/usr/local/go"), "{because}");
    }

    /// **A home with no Java is not asked about Java.**
    #[test]
    fn a_home_with_no_java_is_not_asked_about_java() {
        let outcome = super::java_outcome(
            false,
            std::path::Path::new("/home/me/.mixengine/runtimes"),
            Some(OsStr::new("/usr/lib/jvm/temurin-17")),
            &[],
        );

        assert!(matches!(outcome, Outcome::Ok {}), "{outcome:?}");
    }

    /// A `JAVA_HOME` inside this home's JDKs, or none, leaves `mvn` on a pinned JDK.
    #[test]
    fn a_java_home_inside_this_home_is_ok() {
        let runtimes = std::path::Path::new("/home/me/.mixengine/runtimes");
        let inside = runtimes.join("java").join("21.0.12.1");

        for java_home in [None, Some(inside.as_os_str())] {
            let outcome = super::java_outcome(true, runtimes, java_home, &[]);

            assert!(
                matches!(outcome, Outcome::Ok {}),
                "{java_home:?}: {outcome:?}"
            );
        }
    }

    /// A system JDK in `JAVA_HOME` is the one `mvn` runs, whatever the directory pins.
    #[test]
    fn a_java_home_elsewhere_is_a_note_naming_it() {
        let outcome = super::java_outcome(
            true,
            std::path::Path::new("/home/me/.mixengine/runtimes"),
            Some(OsStr::new("/usr/lib/jvm/temurin-17")),
            &[],
        );

        let Outcome::Note { because } = outcome else {
            panic!("a JDK mvn would use instead is news: {outcome:?}");
        };

        assert!(
            because.contains("JAVA_HOME=/usr/lib/jvm/temurin-17"),
            "{because}"
        );
    }

    /// A trust store named in a JVM's options replaces the `cacerts` MixEngine writes into.
    #[test]
    fn a_trust_store_in_the_options_is_a_note_naming_the_variable() {
        let options = [(
            "_JAVA_OPTIONS",
            Some(std::ffi::OsString::from(
                "-Djavax.net.ssl.trustStore=/etc/corp.jks",
            )),
        )];

        let outcome = super::java_outcome(
            true,
            std::path::Path::new("/home/me/.mixengine/runtimes"),
            None,
            &options,
        );

        let Outcome::Note { because } = outcome else {
            panic!("a replaced trust store is news: {outcome:?}");
        };

        assert!(because.contains("_JAVA_OPTIONS"), "{because}");
    }
}
