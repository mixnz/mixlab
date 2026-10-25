//! What an artifact asks of the machine that this machine does not have.
//!
//! **Values, because a client draws them.** The wire [`Error`](crate::Error) carries a message
//! nobody may parse, and MixLab has to put a button beside a requirement and say it in the reader's
//! language — so what is missing and what can be done about it arrive typed, and the sentences
//! below are `mix`'s rendering of them.

use std::fmt;

use crate::PackageVersion;

/// Which build of the Microsoft Visual C++ Redistributable.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum RedistributableArch {
    /// For x86_64 programs — what an emulated x64 PHP on ARM64 Windows needs too (ADR 0023).
    X64,

    /// For ARM64 programs.
    Arm64,
}

impl RedistributableArch {
    /// The word Microsoft's own file names use.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::X64 => "x64",
            Self::Arm64 => "arm64",
        }
    }
}

impl fmt::Display for RedistributableArch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// What the machine lacks.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "need", rename_all = "snake_case")]
#[non_exhaustive]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Need {
    /// A newer C library than this Linux has.
    Glibc {
        /// What the artifact was measured to need.
        at_least: String,
        /// What this machine has.
        found: String,
    },

    /// A newer macOS than this Mac runs.
    Macos {
        /// What the artifact was measured to need.
        at_least: String,
        /// What this machine runs.
        found: String,
    },

    /// A Visual C++ runtime this Windows does not have, or has too old.
    VisualCpp {
        /// The redistributable year the artifact names — `2019`.
        year: String,
        /// Which build of it.
        arch: RedistributableArch,
        /// The version present, `14.29`, when one is present and too old.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        found: Option<String>,
    },

    /// A processor feature this machine's processor lacks.
    ///
    /// Nothing installs one, so its remedy is a release that does not need it, or none.
    Cpu {
        /// The feature the artifact names, as the index spells it — `avx`.
        feature: String,
    },

    /// A shared library the build links and this machine's loader does not list.
    ///
    /// **The one need that never refuses.** Every Linux JDK links `libz`, `freetype`, X11 and ALSA
    /// from the distribution, and a headless server runs without three of the four — so this is
    /// said and the install goes on. Its remedy is always
    /// [`InstallFromDistribution`](Remedy::InstallFromDistribution).
    SharedLibrary {
        /// As the index spells it — `libasound.so.2`.
        soname: String,
    },
}

impl Need {
    /// The few words a table cell has room for.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Glibc { at_least, .. } => format!("glibc {at_least}+"),
            Self::Macos { at_least, .. } => format!("macOS {at_least}+"),
            Self::VisualCpp { year, arch, .. } => format!("Visual C++ {year} ({arch})"),
            Self::Cpu { feature } => format!("CPU with {}", feature.to_uppercase()),
            Self::SharedLibrary { soname } => soname.clone(),
        }
    }
}

impl fmt::Display for Need {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Glibc { at_least, found } => {
                write!(
                    formatter,
                    "glibc {at_least} or newer, and this machine has {found}"
                )
            }
            Self::Macos { at_least, found } => {
                write!(
                    formatter,
                    "macOS {at_least} or newer, and this Mac runs {found}"
                )
            }
            Self::VisualCpp {
                year,
                arch,
                found: None,
            } => write!(
                formatter,
                "the Microsoft Visual C++ {year} Redistributable ({arch}) or newer, and this \
                 machine has none"
            ),
            Self::VisualCpp {
                year,
                arch,
                found: Some(found),
            } => write!(
                formatter,
                "the Microsoft Visual C++ {year} Redistributable ({arch}) or newer, and this \
                 machine has {found}"
            ),
            Self::Cpu { feature } => write!(
                formatter,
                "a processor with {}, and this one does not have it",
                feature.to_uppercase()
            ),
            Self::SharedLibrary { soname } => write!(
                formatter,
                "the shared library {soname}, which this machine's loader does not list"
            ),
        }
    }
}

/// What can be done about one [`Need`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "remedy", rename_all = "snake_case")]
#[non_exhaustive]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Remedy {
    /// MixEngine can install it from Microsoft, once somebody agrees.
    InstallVisualCpp {
        /// Which build.
        arch: RedistributableArch,
    },

    /// Nothing can be installed; this is the newest release of the same kind that runs here.
    ChooseVersion {
        /// That release.
        version: PackageVersion,
    },

    /// Nothing can be installed, and no published release of the kind runs here.
    Unavailable,

    /// The distribution's package manager has it, and nothing is refused meanwhile.
    ///
    /// **No package name.** `libasound.so.2` is `libasound2` on Debian and `alsa-lib` on Fedora,
    /// and a daemon that guessed would be naming something it did not measure.
    InstallFromDistribution,
}

impl fmt::Display for Remedy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InstallVisualCpp { arch } => write!(
                formatter,
                "MixEngine can install the Microsoft Visual C++ Redistributable ({arch}) from \
                 Microsoft"
            ),
            Self::ChooseVersion { version } => {
                write!(
                    formatter,
                    "{} is the newest release that runs here",
                    version.as_str()
                )
            }
            Self::Unavailable => formatter.write_str("no published release runs on this machine"),
            Self::InstallFromDistribution => {
                formatter.write_str("install it with this distribution's package manager")
            }
        }
    }
}

/// One thing missing, and what can be done about it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Requirement {
    /// What is missing.
    pub need: Need,

    /// What can be done about it.
    pub remedy: Remedy,
}

/// What `runtime.requirements` and `package.requirements` answer.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Requirements {
    /// Everything the version lacks here. Empty when it lacks nothing, or nothing could be judged.
    pub unmet: Vec<Requirement>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A processor feature is named the way the index names it**, and said the way a person reads
    /// it.
    #[test]
    fn a_missing_processor_feature_is_said_in_capitals_and_sent_as_published() {
        let need = Need::Cpu {
            feature: "avx".to_owned(),
        };

        assert_eq!(need.label(), "CPU with AVX");
        assert_eq!(
            need.to_string(),
            "a processor with AVX, and this one does not have it"
        );
        assert_eq!(
            serde_json::to_value(&need).expect("it encodes"),
            serde_json::json!({"need": "cpu", "feature": "avx"})
        );
    }

    /// **A library is named by its soname, and what to do about it names no package.**
    #[test]
    fn a_missing_library_is_said_by_soname_and_sent_as_published() {
        let need = Need::SharedLibrary {
            soname: "libasound.so.2".to_owned(),
        };

        assert_eq!(need.label(), "libasound.so.2");
        assert_eq!(
            need.to_string(),
            "the shared library libasound.so.2, which this machine's loader does not list"
        );
        assert_eq!(
            serde_json::to_value(&need).expect("it encodes"),
            serde_json::json!({"need": "shared_library", "soname": "libasound.so.2"})
        );
        assert_eq!(
            serde_json::to_value(Remedy::InstallFromDistribution).expect("it encodes"),
            serde_json::json!({"remedy": "install_from_distribution"})
        );
    }
}
