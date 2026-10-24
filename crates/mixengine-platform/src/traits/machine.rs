//! What this machine offers the programs MixEngine installs.
//!
//! **Read, never remembered.** [`Machine::facts`] is asked once per question the daemon answers, so
//! a runtime somebody installed a minute ago is seen by the next question anybody asks. Every read
//! is a registry lookup or one call into the C library.
//!
//! **The half that is a decision is compiled everywhere**, on [`crate::AppControl`]'s pattern: the
//! parsers below are pure and their tests run on all three systems; only the calls sit behind a
//! `cfg`.

/// One fact about the machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe<T> {
    /// The machine has it, at this version.
    Present(T),

    /// The machine does not have it.
    Absent,

    /// The question could not be answered, or does not apply to this system.
    ///
    /// **Never a lack.** Glibc on macOS reads as this rather than as [`Probe::Absent`], because a
    /// judgement that read "absent" would refuse an artifact over a probe that does not exist here.
    Unknown,
}

/// A Visual C++ runtime's version, as its registry key states it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct VisualCppVersion {
    /// `Major` — 14 for every runtime since 2015.
    pub major: u32,

    /// `Minor` — 0, 1x, 2x, 3x, 4x, 50 … as the toolset moved.
    pub minor: u32,
}

/// Everything [`Machine::facts`] answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineFacts {
    /// `gnu_get_libc_version()`, on Linux.
    pub glibc: Probe<String>,

    /// `kern.osproductversion`, on macOS.
    pub macos: Probe<String>,

    /// The Visual C++ 2015-and-later runtime for x64, on Windows.
    pub visual_cpp_x64: Probe<VisualCppVersion>,

    /// The same runtime for ARM64, on Windows.
    pub visual_cpp_arm64: Probe<VisualCppVersion>,

    /// Whether the processor has AVX and the operating system saves its state. See [`avx`].
    pub avx: Probe<()>,

    /// The sonames this system's loader lists for this architecture, on Linux. See
    /// [`shared_libraries`].
    pub shared_libraries: Probe<std::collections::BTreeSet<String>>,
}

impl MachineFacts {
    /// A machine nothing could be learned about — every fact [`Probe::Unknown`].
    #[must_use]
    pub const fn unknown() -> Self {
        Self {
            glibc: Probe::Unknown,
            macos: Probe::Unknown,
            visual_cpp_x64: Probe::Unknown,
            visual_cpp_arm64: Probe::Unknown,
            avx: Probe::Unknown,
            shared_libraries: Probe::Unknown,
        }
    }
}

/// Whether this processor supports AVX, and this operating system saves its state.
///
/// **One answer for all three systems**, which is why it is here rather than in each one's
/// `machine` module: the question is the processor's, and `is_x86_feature_detected!` asks both
/// halves of it — the CPUID bit, and `OSXSAVE` with `XGETBV` — which is what a MongoDB build
/// compiled for AVX actually depends on.
///
/// **[`Probe::Unknown`] in any build that is not x86_64.** An ARM64 daemon cannot ask an x86
/// question, and the x86_64 artifact it would install on Windows on ARM runs under the operating
/// system's emulation, whose AVX depends on the Windows release; `SmokeTest` decides that case.
#[must_use]
pub fn avx() -> Probe<()> {
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("avx") {
            Probe::Present(())
        } else {
            Probe::Absent
        }
    }

    #[cfg(not(target_arch = "x86_64"))]
    {
        Probe::Unknown
    }
}

/// The sonames `ldconfig -p` lists for `arch`.
///
/// `arch` is `std::env::consts::ARCH`, and the tag it maps to is the one `ldconfig` prints beside
/// each entry: a 32-bit library carries no architecture tag at all, so a machine with only an i386
/// `libasound` does not count as having one.
///
/// **Nothing listed is no answer**, rather than everything missing: a container with no
/// `ld.so.cache` answers "0 libs found in cache", and reading that as a lack would warn about `libz`
/// on a machine that plainly has it. Parsing lives here, beside [`dotted_version`], so it is tested
/// on all three systems while only the call sits behind a `cfg`.
#[must_use]
pub fn shared_libraries(listing: &str, arch: &str) -> Probe<std::collections::BTreeSet<String>> {
    let tag = match arch {
        "x86_64" => "x86-64",
        "aarch64" => "AArch64",
        _ => return Probe::Unknown,
    };

    let found: std::collections::BTreeSet<String> = listing
        .lines()
        .filter_map(|line| {
            let entry = line.strip_prefix('\t')?;
            let (soname, rest) = entry.split_once(" (")?;
            let (tags, _) = rest.split_once(')')?;

            tags.split(',')
                .any(|one| one.trim() == tag)
                .then(|| soname.trim().to_owned())
        })
        .collect();

    match found.is_empty() {
        true => Probe::Unknown,
        false => Probe::Present(found),
    }
}

/// What this machine offers.
pub trait Machine: std::fmt::Debug + Send + Sync {
    /// Read every fact now.
    ///
    /// Infallible on purpose: a fact that cannot be read is [`Probe::Unknown`], and the caller's
    /// rule for that is already "do not refuse".
    fn facts(&self) -> MachineFacts;
}

/// A version worth comparing: dotted decimal integers and nothing else.
///
/// Trailing NULs and whitespace are the C library's and sysctl's framing, not the version, and are
/// dropped. Anything else — a distribution suffix, an empty part — is [`Probe::Unknown`], because a
/// comparison against a guess is a refusal nobody can argue with.
#[must_use]
pub fn dotted_version(text: &str) -> Probe<String> {
    let trimmed =
        text.trim_matches(|character: char| character == '\0' || character.is_whitespace());

    let valid = !trimmed.is_empty()
        && trimmed
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));

    if valid {
        Probe::Present(trimmed.to_owned())
    } else {
        Probe::Unknown
    }
}

/// What a `VC\Runtimes\<arch>` key's three `REG_DWORD` values mean.
///
/// `installed` other than `1` — including a key with no such value — is [`Probe::Absent`], which
/// is what the redistributable's own uninstaller leaves behind. `Installed = 1` with a version that
/// cannot be read is [`Probe::Unknown`]: something is there, and how new it is is not known.
#[must_use]
pub fn visual_cpp_from_registry(
    installed: Option<u32>,
    major: Option<u32>,
    minor: Option<u32>,
) -> Probe<VisualCppVersion> {
    match (installed, major, minor) {
        (Some(1), Some(major), Some(minor)) => Probe::Present(VisualCppVersion { major, minor }),
        (Some(1), _, _) => Probe::Unknown,
        _ => Probe::Absent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_is_dotted_digits_and_nothing_else() {
        assert_eq!(dotted_version("2.35"), Probe::Present("2.35".to_owned()));
        assert_eq!(
            dotted_version("14.6.1\0"),
            Probe::Present("14.6.1".to_owned())
        );
        assert_eq!(
            dotted_version(" 13.6 \n"),
            Probe::Present("13.6".to_owned())
        );

        assert_eq!(dotted_version(""), Probe::Unknown);
        assert_eq!(dotted_version("2.35-ubuntu"), Probe::Unknown);
        assert_eq!(dotted_version("2..35"), Probe::Unknown);
        assert_eq!(dotted_version("v14"), Probe::Unknown);
    }

    /// Four lines of `ldconfig -p`, as Ubuntu prints them, measured on 2026-09-18. The third is
    /// 32-bit, which carries no architecture tag; the fourth is the shape a distribution prints
    /// when it records an ABI.
    const LISTING: &str = "363 libs found in cache `/etc/ld.so.cache'\n\
        \tlibz.so.1 (libc6,x86-64) => /lib/x86_64-linux-gnu/libz.so.1\n\
        \tlibasound.so.2 (libc6) => /lib/i386-linux-gnu/libasound.so.2\n\
        \tlibfreetype.so.6 (libc6,x86-64, OS ABI: Linux 3.2.0) => /lib/x86_64-linux-gnu/libfreetype.so.6\n";

    #[test]
    fn a_listing_names_the_libraries_of_this_architecture_only() {
        let Probe::Present(found) = shared_libraries(LISTING, "x86_64") else {
            panic!("a listing with entries is an answer");
        };

        assert!(found.contains("libz.so.1"));
        assert!(
            found.contains("libfreetype.so.6"),
            "an OS ABI tag beside the architecture is still this architecture"
        );
        assert!(
            !found.contains("libasound.so.2"),
            "a 32-bit library is not one an x86_64 build can load"
        );
    }

    #[test]
    fn an_empty_cache_or_an_architecture_with_no_tag_is_no_answer() {
        assert_eq!(
            shared_libraries("0 libs found in cache `/etc/ld.so.cache'\n", "x86_64"),
            Probe::Unknown
        );
        assert_eq!(shared_libraries(LISTING, "riscv64"), Probe::Unknown);
        assert_eq!(shared_libraries("", "x86_64"), Probe::Unknown);
    }

    /// **About the code, not the runner**: a processor with AVX and one without are both a pass.
    /// What an x86_64 build may never answer is "could not tell".
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn an_x86_64_build_answers_about_avx() {
        assert_ne!(avx(), Probe::Unknown);
    }

    /// Measured on a Windows 11 machine, 2026-09-16: `Installed=1 Major=14 Minor=50`.
    #[test]
    fn a_runtime_key_is_present_only_when_it_says_installed() {
        assert_eq!(
            visual_cpp_from_registry(Some(1), Some(14), Some(50)),
            Probe::Present(VisualCppVersion {
                major: 14,
                minor: 50
            })
        );
        assert_eq!(
            visual_cpp_from_registry(Some(1), None, Some(50)),
            Probe::Unknown
        );
        assert_eq!(
            visual_cpp_from_registry(Some(0), Some(14), Some(50)),
            Probe::Absent
        );
        assert_eq!(visual_cpp_from_registry(None, None, None), Probe::Absent);
    }

    #[test]
    fn a_newer_toolset_orders_after_an_older_one() {
        let older = VisualCppVersion {
            major: 14,
            minor: 29,
        };
        let newer = VisualCppVersion {
            major: 14,
            minor: 50,
        };
        assert!(older < newer);
        assert!(
            VisualCppVersion {
                major: 15,
                minor: 0
            } > newer
        );
    }
}
