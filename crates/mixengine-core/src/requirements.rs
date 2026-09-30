//! What an artifact asks of the machine, judged against what the machine has — roadmap task
//! **T148**.
//!
//! **Only a certain lack refuses** (T148 design, D2). A fact the machine could not report, a year
//! this build has no row for, a version that is not dotted numbers, `vcredist` 2010 and 2013 whose
//! keys nobody has read on a machine that has them, a `cpu` feature other than AVX, and `tzdata`:
//! none of them is ever a lack, and `install::SmokeTest` decides them exactly as it did before this
//! module existed. AVX is judged since **T153**, the task that made MongoDB — the one kind that
//! states it — installable.
//!
//! Pure: no OS call, no network, so every rule here is tested without a machine to test it on.

use std::cmp::Ordering;

use mixengine_platform::{MachineFacts, Probe, VisualCppVersion};
use mixengine_proto::{Need, PackageVersion, RedistributableArch, Remedy, Requirement};

use crate::index::{Arch, Channel, Index, Requires, Target};

/// The Visual C++ years this build knows, and the smallest `14.x` minor that satisfies each.
///
/// Every runtime from 2015 on is one binary-compatible family under one registry key, so a newer
/// one satisfies an older year — `Minor=50` meets all four.
const VISUAL_CPP_YEARS: [(&str, u32); 4] = [("2015", 0), ("2017", 10), ("2019", 20), ("2022", 30)];

/// What `requires` asks for that `facts` certainly lacks, for an artifact built for `arch`.
#[must_use]
pub fn unmet(requires: &Requires, arch: Arch, facts: &MachineFacts) -> Vec<Need> {
    let mut unmet = Vec::new();

    if let Some(wanted) = requires.glibc.as_deref()
        && let Some(found) = short_of(wanted, &facts.glibc)
    {
        unmet.push(Need::Glibc {
            at_least: wanted.to_owned(),
            found,
        });
    }

    if let Some(wanted) = requires.macos.as_deref()
        && let Some(found) = short_of(wanted, &facts.macos)
    {
        unmet.push(Need::Macos {
            at_least: wanted.to_owned(),
            found,
        });
    }

    if let Some(year) = requires.vcredist.as_deref()
        && let Some(need) = visual_cpp(year, arch, facts)
    {
        unmet.push(need);
    }

    // **Only AVX, only an x86_64 artifact, and only a certain absence** — roadmap task **T153**.
    // The index states `avx` on MongoDB's ARM64 cells too, where it means nothing, and a feature
    // word this build has no probe for is a could-not-tell like any other.
    if requires.cpu.as_deref() == Some("avx") && arch == Arch::X86_64 && facts.avx == Probe::Absent
    {
        unmet.push(Need::Cpu {
            feature: "avx".to_owned(),
        });
    }

    // **A warning, never a refusal** — roadmap task **T27e**, its design's D15. Only a certain
    // absence, as everything above: a machine whose loader cache could not be read lacks nothing
    // here, and one that lists a soname has it whatever else is true.
    if let Probe::Present(listed) = &facts.shared_libraries {
        for soname in &requires.libraries {
            if !listed.contains(soname) {
                unmet.push(Need::SharedLibrary {
                    soname: soname.clone(),
                });
            }
        }
    }

    unmet
}

/// What stands between `target` and `kind` `version`, each with what can be done about it.
///
/// [`None`] when `target` has no artifact of that version — there is nothing to judge, and the
/// caller already has a sentence for "not published here".
#[must_use]
pub fn judge(
    index: &Index,
    target: Target,
    kind: &str,
    version: &str,
    facts: &MachineFacts,
) -> Option<Vec<Requirement>> {
    let selection = index.select(target, kind, version)?;

    let needs = unmet(&selection.artifact.requires, selection.artifact.arch, facts);
    if needs.is_empty() {
        return Some(Vec::new());
    }

    // Asked only when something cannot be installed, and asked once: it walks every release.
    let mut alternative: Option<Option<PackageVersion>> = None;

    Some(
        needs
            .into_iter()
            .map(|need| {
                let remedy = match &need {
                    Need::VisualCpp { arch, .. } => Remedy::InstallVisualCpp { arch: *arch },

                    // **Never `ChooseVersion`** (T27e, D15): every Linux release of a line links the
                    // same libraries, so offering another one would be offering the same warning.
                    Need::SharedLibrary { .. } => Remedy::InstallFromDistribution,
                    _ => match alternative
                        .get_or_insert_with(|| newest_met(index, target, kind, version, facts))
                    {
                        Some(version) => Remedy::ChooseVersion {
                            version: version.clone(),
                        },
                        None => Remedy::Unavailable,
                    },
                };
                Requirement { need, remedy }
            })
            .collect(),
    )
}

/// Whether any of these needs a person to agree to an installer.
#[must_use]
pub fn needs_consent(requirements: &[Requirement]) -> bool {
    requirements
        .iter()
        .any(|requirement| matches!(requirement.remedy, Remedy::InstallVisualCpp { .. }))
}

/// Whether any of these is something no installer MixEngine runs can fix.
///
/// **An advisory is not one** — roadmap task **T27e**: a library the distribution provides is said
/// and stands in nobody's way, which is the whole of what [`is_advisory`] marks.
#[must_use]
pub fn blocks(requirements: &[Requirement]) -> bool {
    requirements.iter().any(|requirement| {
        !is_advisory(requirement) && !matches!(requirement.remedy, Remedy::InstallVisualCpp { .. })
    })
}

/// Whether this requirement only warns — roadmap task **T27e**, its design's D15.
#[must_use]
pub fn is_advisory(requirement: &Requirement) -> bool {
    matches!(requirement.remedy, Remedy::InstallFromDistribution)
}

/// The requirements that only warn, in the order they were judged.
#[must_use]
pub fn advisories(requirements: &[Requirement]) -> Vec<Requirement> {
    requirements
        .iter()
        .filter(|requirement| is_advisory(requirement))
        .cloned()
        .collect()
}

/// Every redistributable these ask to install, once each.
#[must_use]
pub fn redistributables(requirements: &[Requirement]) -> Vec<RedistributableArch> {
    let mut arches: Vec<RedistributableArch> = requirements
        .iter()
        .filter_map(|requirement| match requirement.remedy {
            Remedy::InstallVisualCpp { arch } => Some(arch),
            _ => None,
        })
        .collect();
    arches.sort_unstable();
    arches.dedup();
    arches
}

/// Several judgements as one list, each distinct requirement once, in the order first met.
///
/// A blueprint installing three PHPs that each need the same runtime asks about it once (D7).
#[must_use]
pub fn merged(requirements: impl IntoIterator<Item = Requirement>) -> Vec<Requirement> {
    let mut merged: Vec<Requirement> = Vec::new();
    for requirement in requirements {
        if !merged.contains(&requirement) {
            merged.push(requirement);
        }
    }
    merged
}

/// `Some(found)` when the machine certainly has less than `wanted`.
fn short_of(wanted: &str, found: &Probe<String>) -> Option<String> {
    let Probe::Present(found) = found else {
        return None;
    };
    let (Some(wanted_parts), Some(found_parts)) = (numbers(wanted), numbers(found)) else {
        return None;
    };

    (compare(&found_parts, &wanted_parts) == Ordering::Less).then(|| found.clone())
}

/// Dotted decimal integers, or [`None`] for anything else.
fn numbers(text: &str) -> Option<Vec<u64>> {
    text.split('.').map(|part| part.parse().ok()).collect()
}

/// Component by component, a missing component counting as zero — `14` equals `14.0`.
fn compare(left: &[u64], right: &[u64]) -> Ordering {
    (0..left.len().max(right.len()))
        .map(|at| {
            left.get(at)
                .copied()
                .unwrap_or(0)
                .cmp(&right.get(at).copied().unwrap_or(0))
        })
        .find(|ordering| ordering.is_ne())
        .unwrap_or(Ordering::Equal)
}

/// The Visual C++ need, when `year` is one this build knows and the runtime is certainly short.
fn visual_cpp(year: &str, arch: Arch, facts: &MachineFacts) -> Option<Need> {
    let (_, minimum) = VISUAL_CPP_YEARS.iter().find(|(known, _)| *known == year)?;

    let (redistributable, probe) = match arch {
        Arch::X86_64 => (RedistributableArch::X64, &facts.visual_cpp_x64),
        Arch::Aarch64 => (RedistributableArch::Arm64, &facts.visual_cpp_arm64),
    };
    let wanted = VisualCppVersion {
        major: 14,
        minor: *minimum,
    };

    match probe {
        Probe::Unknown => None,
        Probe::Present(found) if *found >= wanted => None,
        Probe::Absent => Some(Need::VisualCpp {
            year: year.to_owned(),
            arch: redistributable,
            found: None,
        }),
        Probe::Present(found) => Some(Need::VisualCpp {
            year: year.to_owned(),
            arch: redistributable,
            found: Some(format!("{}.{}", found.major, found.minor)),
        }),
    }
}

/// The newest stable release of `kind`, other than `asked`, whose artifact this machine lacks
/// nothing for.
///
/// **A warning is not a lack here** — roadmap task **T27e**: every Linux release of a JDK line links
/// the same libraries, so counting one would leave a machine short of glibc with nothing to be
/// offered at all.
fn newest_met(
    index: &Index,
    target: Target,
    kind: &str,
    asked: &str,
    facts: &MachineFacts,
) -> Option<PackageVersion> {
    index
        .installable_for(target, kind)
        .filter(|package| package.channel == Channel::Stable && package.version != asked)
        .filter(|package| {
            package.select(target).is_some_and(|selection| {
                unmet(&selection.artifact.requires, selection.artifact.arch, facts)
                    .iter()
                    .all(|need| matches!(need, Need::SharedLibrary { .. }))
            })
        })
        .filter_map(|package| PackageVersion::parse(package.version.clone()).ok())
        .max_by(PackageVersion::cmp_precedence)
}

#[cfg(test)]
mod tests {
    use mixengine_platform::{MachineFacts, Probe, VisualCppVersion};
    use mixengine_proto::{Need, RedistributableArch, Remedy};

    use super::*;
    use crate::index::{Arch, Index, Os, Requires, Target};

    fn requires(json: &str) -> Requires {
        serde_json::from_str(json).expect("a requires object")
    }

    fn linux(glibc: &str) -> MachineFacts {
        MachineFacts {
            glibc: Probe::Present(glibc.to_owned()),
            ..MachineFacts::unknown()
        }
    }

    fn mac(version: &str) -> MachineFacts {
        MachineFacts {
            macos: Probe::Present(version.to_owned()),
            ..MachineFacts::unknown()
        }
    }

    fn windows(x64: Probe<VisualCppVersion>) -> MachineFacts {
        MachineFacts {
            visual_cpp_x64: x64,
            ..MachineFacts::unknown()
        }
    }

    const fn runtime(minor: u32) -> Probe<VisualCppVersion> {
        Probe::Present(VisualCppVersion { major: 14, minor })
    }

    #[test]
    fn an_older_glibc_is_a_lack_and_an_equal_or_newer_one_is_not() {
        let wanted = requires(r#"{"glibc": "2.34"}"#);

        assert_eq!(
            unmet(&wanted, Arch::X86_64, &linux("2.31")),
            [Need::Glibc {
                at_least: "2.34".to_owned(),
                found: "2.31".to_owned()
            }]
        );
        assert!(unmet(&wanted, Arch::X86_64, &linux("2.34")).is_empty());
        assert!(unmet(&wanted, Arch::X86_64, &linux("2.39")).is_empty());
    }

    #[test]
    fn versions_compare_as_numbers_with_missing_parts_as_zero() {
        let wanted = requires(r#"{"macos": "14.0"}"#);

        assert_eq!(unmet(&wanted, Arch::Aarch64, &mac("13.6.1")).len(), 1);
        assert!(unmet(&wanted, Arch::Aarch64, &mac("14")).is_empty());
        assert_eq!(
            unmet(
                &requires(r#"{"macos": "10.14"}"#),
                Arch::X86_64,
                &mac("10.9")
            )
            .len(),
            1
        );
    }

    #[test]
    fn nothing_uncertain_is_ever_a_lack() {
        let unknown = MachineFacts::unknown();

        assert!(unmet(&requires(r#"{"glibc": "2.34"}"#), Arch::X86_64, &unknown).is_empty());
        assert!(
            unmet(
                &requires(r#"{"glibc": "2.34-rc"}"#),
                Arch::X86_64,
                &linux("2.31")
            )
            .is_empty()
        );
        assert!(
            unmet(
                &requires(r#"{"vcredist": "2026"}"#),
                Arch::X86_64,
                &windows(Probe::Absent)
            )
            .is_empty()
        );
        assert!(
            unmet(
                &requires(r#"{"vcredist": "2010"}"#),
                Arch::X86_64,
                &windows(Probe::Absent)
            )
            .is_empty()
        );
        assert!(
            unmet(
                &requires(r#"{"vcredist": "2019"}"#),
                Arch::X86_64,
                &windows(Probe::Unknown)
            )
            .is_empty()
        );
        assert!(unmet(&requires(r#"{"cpu": "avx"}"#), Arch::X86_64, &unknown).is_empty());
    }

    /// `Minor=50` is what a Windows 11 machine read on 2026-09-16, and it is newer than every year
    /// the index names.
    #[test]
    fn a_year_is_a_floor_on_the_runtime_minor() {
        let wanted = requires(r#"{"vcredist": "2022"}"#);

        assert!(unmet(&wanted, Arch::X86_64, &windows(runtime(50))).is_empty());
        assert!(unmet(&wanted, Arch::X86_64, &windows(runtime(30))).is_empty());
        assert_eq!(
            unmet(&wanted, Arch::X86_64, &windows(runtime(29))),
            [Need::VisualCpp {
                year: "2022".to_owned(),
                arch: RedistributableArch::X64,
                found: Some("14.29".to_owned())
            }]
        );
        assert_eq!(
            unmet(
                &requires(r#"{"vcredist": "2015"}"#),
                Arch::X86_64,
                &windows(Probe::Absent)
            ),
            [Need::VisualCpp {
                year: "2015".to_owned(),
                arch: RedistributableArch::X64,
                found: None
            }]
        );
    }

    /// ADR 0023: an emulated x86_64 build needs the x64 runtime even on an ARM64 machine.
    #[test]
    fn the_artifact_and_not_the_machine_picks_which_runtime() {
        let wanted = requires(r#"{"vcredist": "2019"}"#);
        let arm_machine = MachineFacts {
            visual_cpp_x64: Probe::Absent,
            visual_cpp_arm64: runtime(50),
            ..MachineFacts::unknown()
        };

        assert_eq!(unmet(&wanted, Arch::X86_64, &arm_machine).len(), 1);
        assert!(unmet(&wanted, Arch::Aarch64, &arm_machine).is_empty());
    }

    fn index(packages: &str) -> Index {
        serde_json::from_str::<crate::index::schema1::Document>(&format!(
            r#"{{"schema": 1, "generated_at": "2026-09-16T00:00:00Z", "packages": [{packages}]}}"#
        ))
        .expect("an index")
        .into()
    }

    fn php(version: &str, os: &str, requires: &str) -> String {
        format!(
            r#"{{"kind": "php", "version": "{version}", "channel": "stable", "artifacts": [{{
                "os": "{os}", "arch": "aarch64", "url": "https://example.invalid/{version}",
                "sha256": "00", "size": 1, "provides": {{"php": "bin/php"}},
                "requires": {requires}
            }}]}}"#
        )
    }

    #[test]
    fn a_lack_nothing_can_install_names_the_newest_release_that_runs() {
        let index = index(
            &[
                php("8.4.24", "macos", r#"{"macos": "14.0"}"#),
                php("8.3.33", "macos", r#"{"macos": "11.0"}"#),
                php("8.2.33", "macos", r#"{"macos": "11.0"}"#),
            ]
            .join(","),
        );
        let target = Target::new(Os::Macos, Arch::Aarch64);

        let judged = judge(&index, target, "php", "8.4.24", &mac("13.6")).expect("published here");

        assert_eq!(judged.len(), 1);
        assert!(
            matches!(&judged[0].remedy, Remedy::ChooseVersion { version } if version.as_str() == "8.3.33"),
            "{judged:?}"
        );
        assert!(blocks(&judged));
        assert!(!needs_consent(&judged));
    }

    /// A machine that lists what a build links, and one that does not — roadmap task **T27e**.
    fn linux_listing(sonames: &[&str]) -> MachineFacts {
        MachineFacts {
            glibc: Probe::Present("2.39".to_owned()),
            shared_libraries: Probe::Present(sonames.iter().map(|one| (*one).to_owned()).collect()),
            ..MachineFacts::unknown()
        }
    }

    fn java(version: &str, requires: &str) -> String {
        format!(
            r#"{{"kind": "java", "version": "{version}", "channel": "stable", "artifacts": [{{
                "os": "linux", "arch": "x86_64", "url": "https://example.invalid/{version}",
                "sha256": "00", "size": 1, "provides": {{"java": "bin/java"}},
                "requires": {requires}
            }}]}}"#
        )
    }

    /// **Only a certain absence**, which is T148's D2 applied to one more field.
    #[test]
    fn a_library_the_loader_lists_is_not_a_lack_and_one_it_does_not_is_a_warning() {
        let wanted = requires(r#"{"libraries": ["libz.so.1", "libasound.so.2"]}"#);

        assert_eq!(
            unmet(&wanted, Arch::X86_64, &linux_listing(&["libz.so.1"])),
            vec![Need::SharedLibrary {
                soname: "libasound.so.2".to_owned()
            }]
        );
        assert!(
            unmet(&wanted, Arch::X86_64, &MachineFacts::unknown()).is_empty(),
            "a machine whose loader could not be asked lacks nothing"
        );
        assert!(
            unmet(
                &wanted,
                Arch::X86_64,
                &linux_listing(&["libz.so.1", "libasound.so.2"])
            )
            .is_empty()
        );
    }

    /// **A warning refuses nothing and asks nothing** — roadmap task **T27e**, its design's D15.
    #[test]
    fn a_missing_library_refuses_nothing_and_asks_nothing() {
        let index = index(&java("21.0.12.1", r#"{"libraries": ["libasound.so.2"]}"#));

        let judged = judge(
            &index,
            Target::new(Os::Linux, Arch::X86_64),
            "java",
            "21.0.12.1",
            &linux_listing(&["libz.so.1"]),
        )
        .expect("published here");

        assert!(
            matches!(judged[0].remedy, Remedy::InstallFromDistribution),
            "{judged:?}"
        );
        assert!(!blocks(&judged));
        assert!(!needs_consent(&judged));
        assert_eq!(advisories(&judged).len(), 1);
    }

    /// **A warning does not hide the release that runs** — every Linux JDK links the same set, so
    /// counting it would leave a machine short of glibc with nothing to be offered.
    #[test]
    fn a_warning_does_not_hide_the_release_that_runs() {
        let index = index(
            &[
                java(
                    "25.0.4.1",
                    r#"{"glibc": "2.99", "libraries": ["libasound.so.2"]}"#,
                ),
                java(
                    "21.0.12.1",
                    r#"{"glibc": "2.17", "libraries": ["libasound.so.2"]}"#,
                ),
            ]
            .join(","),
        );

        let judged = judge(
            &index,
            Target::new(Os::Linux, Arch::X86_64),
            "java",
            "25.0.4.1",
            &linux_listing(&["libz.so.1"]),
        )
        .expect("published here");

        assert!(
            judged.iter().any(|one| matches!(
                &one.remedy,
                Remedy::ChooseVersion { version } if version.as_str() == "21.0.12.1"
            )),
            "{judged:?}"
        );
        assert!(blocks(&judged), "the glibc floor still refuses");
    }

    #[test]
    fn a_lack_no_release_escapes_is_unavailable() {
        let index = index(&php("8.4.24", "macos", r#"{"macos": "14.0"}"#));
        let judged = judge(
            &index,
            Target::new(Os::Macos, Arch::Aarch64),
            "php",
            "8.4.24",
            &mac("13.6"),
        )
        .expect("published here");

        assert!(
            matches!(judged[0].remedy, Remedy::Unavailable),
            "{judged:?}"
        );
    }

    #[test]
    fn a_missing_runtime_is_something_mixengine_can_install() {
        let index = index(&php("8.3.33", "windows", r#"{"vcredist": "2019"}"#));
        let arm = MachineFacts {
            visual_cpp_arm64: Probe::Absent,
            ..MachineFacts::unknown()
        };

        let judged = judge(
            &index,
            Target::new(Os::Windows, Arch::Aarch64),
            "php",
            "8.3.33",
            &arm,
        )
        .expect("published here");

        assert!(matches!(
            judged[0].remedy,
            Remedy::InstallVisualCpp {
                arch: RedistributableArch::Arm64
            }
        ));
        assert!(needs_consent(&judged));
        assert!(!blocks(&judged));
        assert_eq!(redistributables(&judged), [RedistributableArch::Arm64]);
    }

    #[test]
    fn a_version_not_published_for_the_target_cannot_be_judged() {
        let index = index(&php("8.3.33", "windows", "{}"));
        assert!(
            judge(
                &index,
                Target::new(Os::Linux, Arch::Aarch64),
                "php",
                "8.3.33",
                &linux("2.39")
            )
            .is_none()
        );
        assert_eq!(
            judge(
                &index,
                Target::new(Os::Windows, Arch::Aarch64),
                "php",
                "8.3.33",
                &MachineFacts::unknown()
            ),
            Some(Vec::new())
        );
    }

    fn without_avx() -> MachineFacts {
        MachineFacts {
            avx: Probe::Absent,
            ..MachineFacts::unknown()
        }
    }

    /// Roadmap task **T153**: MongoDB 5.0 and later refuse to start on an x86_64 without AVX.
    #[test]
    fn an_x86_64_artifact_that_needs_avx_is_refused_on_a_processor_without_it() {
        assert_eq!(
            unmet(&requires(r#"{"cpu": "avx"}"#), Arch::X86_64, &without_avx()),
            [Need::Cpu {
                feature: "avx".to_owned()
            }]
        );
    }

    #[test]
    fn avx_is_not_a_lack_where_it_cannot_be_one() {
        let wanted = requires(r#"{"cpu": "avx"}"#);
        let with = MachineFacts {
            avx: Probe::Present(()),
            ..MachineFacts::unknown()
        };

        assert!(unmet(&wanted, Arch::X86_64, &with).is_empty());
        assert!(
            unmet(&wanted, Arch::Aarch64, &without_avx()).is_empty(),
            "the index states avx on ARM64 cells too, where it means nothing"
        );
        assert!(
            unmet(
                &requires(r#"{"cpu": "avx512f"}"#),
                Arch::X86_64,
                &without_avx()
            )
            .is_empty(),
            "a feature this build has no probe for is a could-not-tell"
        );
    }

    fn mongodb(version: &str) -> String {
        format!(
            r#"{{"kind": "mongodb", "version": "{version}", "channel": "stable", "artifacts": [{{
                "os": "linux", "arch": "x86_64", "url": "https://example.invalid/{version}",
                "sha256": "00", "size": 1, "provides": {{"mongod": "bin/mongod"}},
                "requires": {{"cpu": "avx", "glibc": "2.34"}}
            }}]}}"#
        )
    }

    /// Every MongoDB release needs AVX, so no other version is a way out.
    #[test]
    fn a_lack_every_release_shares_has_nowhere_to_go() {
        let index = index(&[mongodb("8.3.11"), mongodb("7.0.43")].join(","));

        let judged = judge(
            &index,
            Target::new(Os::Linux, Arch::X86_64),
            "mongodb",
            "8.3.11",
            &without_avx(),
        )
        .expect("published here");

        assert!(
            matches!(judged[0].remedy, Remedy::Unavailable),
            "{judged:?}"
        );
        assert!(blocks(&judged));
    }

    #[test]
    fn the_same_requirement_twice_is_asked_about_once() {
        let one = Requirement {
            need: Need::VisualCpp {
                year: "2019".to_owned(),
                arch: RedistributableArch::X64,
                found: None,
            },
            remedy: Remedy::InstallVisualCpp {
                arch: RedistributableArch::X64,
            },
        };
        let merged = merged([one.clone(), one.clone()]);
        assert_eq!(merged, [one]);
    }
}
