//! Which program a tunnel runs, `cloudflared` or `ngrok`. The machine's own first, at a path set in
//! Settings or on `PATH`; then one MixLab downloaded, pinned here and checked against its SHA-256
//! before it is unpacked or run. Never put on `PATH`: it belongs to MixLab, not to the machine.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

pub use super::provider::Provider;
use crate::downloads::{download, make_executable, run, verify_sha256};
use crate::error::AppError;

/// The cloudflared MixLab downloads. Bumped with its digests by `bumping-tool-downloads.md`; a new
/// cloudflared arrives with a new MixLab and never on its own.
pub const CLOUDFLARED_VERSION: &str = "2026.10.0";

/// The ngrok MixLab downloads, by the same procedure. Pinned by version from ngrok's archive:
/// its `-stable-` links move with every release.
pub const NGROK_VERSION: &str = "3.39.13";

/// What the download is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// A `.tgz` holding the program — cloudflared on macOS, ngrok everywhere.
    Tgz,
    /// The program itself — cloudflared on Linux and Windows.
    Bare,
}

/// One pinned download: the published digest and size for the asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub url: String,
    pub sha256: &'static str,
    pub size: u64,
    pub shape: Shape,
    /// The archive's member that is the program. ngrok's Windows archive holds `ngrok` with no
    /// `.exe`, so it is renamed once unpacked.
    pub member: &'static str,
}

/// The version a provider's download is pinned at.
pub fn version(provider: Provider) -> &'static str {
    match provider {
        Provider::Cloudflare => CLOUDFLARED_VERSION,
        Provider::Ngrok => NGROK_VERSION,
    }
}

/// The pinned download for a system, or `None` where upstream publishes nothing.
pub fn source_for(provider: Provider, os: &str, arch: &str) -> Option<Source> {
    match provider {
        Provider::Cloudflare => cloudflared_for(os, arch),
        Provider::Ngrok => ngrok_for(os, arch),
    }
}

/// Windows on ARM runs the x86-64 build emulated: upstream publishes no ARM64 one.
fn cloudflared_for(os: &str, arch: &str) -> Option<Source> {
    let (asset, sha256, size, shape) = match (os, arch) {
        ("macos", "x86_64") => (
            "cloudflared-darwin-amd64.tgz",
            "903845b81828c8cb3c5d13d816a2de71c06a3da5785469df8eb0e1b736d92f9f",
            21_741_581,
            Shape::Tgz,
        ),
        ("macos", "aarch64") => (
            "cloudflared-darwin-arm64.tgz",
            "a2f79ff7b9420aa537d74af239f376da170bbabeb529aec416002adac6a72e70",
            19_809_074,
            Shape::Tgz,
        ),
        ("linux", "x86_64") => (
            "cloudflared-linux-amd64",
            "d33ff2d14475178d2012c2c56beba87389ac5ded27649519f198a7d3134a99db",
            40_129_756,
            Shape::Bare,
        ),
        ("linux", "aarch64") => (
            "cloudflared-linux-arm64",
            "e6422b9d4f72d3194bc5a38676f13667c06666523217b842a877d72a80b5ac08",
            37_687_584,
            Shape::Bare,
        ),
        ("windows", "x86_64" | "aarch64") => (
            "cloudflared-windows-amd64.exe",
            "86aee4017b26625cee8484c113558f48effa4cd47f7aa05fcf425604e5d2b23c",
            55_365_048,
            Shape::Bare,
        ),
        _ => return None,
    };
    Some(Source {
        url: format!(
            "https://github.com/cloudflare/cloudflared/releases/download/{CLOUDFLARED_VERSION}/{asset}"
        ),
        sha256,
        size,
        shape,
        member: "cloudflared",
    })
}

/// ngrok's archive gives every version and target its own id under `bin.equinox.io/a/`. Windows
/// on ARM has a native build.
fn ngrok_for(os: &str, arch: &str) -> Option<Source> {
    let (id, target, sha256, size) = match (os, arch) {
        ("windows", "x86_64") => (
            "hBPGP7bkZc3",
            "windows-amd64",
            "8abf463128e03c42057092b4099501b61b2dd246b2e015cacd0329a41fbda4a9",
            12_271_588,
        ),
        ("windows", "aarch64") => (
            "dYM3BLCUmeA",
            "windows-arm64",
            "f7baa523c89894a7e111b40180f207827fa1cf488066913477911520cd2ed06e",
            10_972_414,
        ),
        ("macos", "x86_64") => (
            "j9Pipq94WPQ",
            "darwin-amd64",
            "7b9101e85898c4826b2404f468a098f3affd340933723ebf3a5adeca47aba2b5",
            12_482_503,
        ),
        ("macos", "aarch64") => (
            "bG5Zu59UHcL",
            "darwin-arm64",
            "17f580a08001c0f14d34739e2f617968af426cce20750ba9e9556c3d1ee058a0",
            11_337_193,
        ),
        ("linux", "x86_64") => (
            "h2cqbjsNLj7",
            "linux-amd64",
            "233e0647849913e6404150c1e9c5e4f31c1368aefee330958d038d7afecb8f2e",
            12_184_811,
        ),
        ("linux", "aarch64") => (
            "j83dYTwHiq9",
            "linux-arm64",
            "fb41ee8f251969f6e399ef03122f791ae537155dfd9ac4c0baf108aca95c17d7",
            11_043_230,
        ),
        _ => return None,
    };
    Some(Source {
        url: format!("https://bin.equinox.io/a/{id}/ngrok-v3-{NGROK_VERSION}-{target}.tar.gz"),
        sha256,
        size,
        shape: Shape::Tgz,
        member: "ngrok",
    })
}

/// This machine's download.
pub fn source(provider: Provider) -> Option<Source> {
    source_for(provider, std::env::consts::OS, std::env::consts::ARCH)
}

/// The program's file name, with this system's executable suffix.
pub fn file_name(provider: Provider) -> String {
    format!("{}{}", provider.program(), std::env::consts::EXE_SUFFIX)
}

/// Where MixLab keeps its own copy.
pub fn downloaded_path(provider: Provider, data_dir: &Path) -> PathBuf {
    data_dir.join("tunnel").join(file_name(provider))
}

/// A program that was found, and where it came from.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Found {
    pub path: PathBuf,
    /// `settings`, `path` or `downloaded` — what the tab says about where it came from.
    pub origin: &'static str,
}

/// The program a tunnel would run.
pub fn find(provider: Provider, configured: Option<&Path>, data_dir: &Path) -> Option<Found> {
    find_in(
        provider,
        configured,
        data_dir,
        &std::env::var_os("PATH").unwrap_or_default(),
    )
}

fn find_in(
    provider: Provider,
    configured: Option<&Path>,
    data_dir: &Path,
    path_var: &OsStr,
) -> Option<Found> {
    if let Some(path) = configured.filter(|p| p.is_file()) {
        return Some(Found {
            path: path.to_path_buf(),
            origin: "settings",
        });
    }
    if let Some(path) = mixengine_platform::process::program_on_path(provider.program(), path_var) {
        return Some(Found {
            path,
            origin: "path",
        });
    }
    let own = downloaded_path(provider, data_dir);
    own.is_file().then_some(Found {
        path: own,
        origin: "downloaded",
    })
}

/// Downloads this machine's pinned program into the data directory and answers its path.
pub fn install(
    provider: Provider,
    data_dir: &Path,
    report: &dyn Fn(u64, u64),
) -> Result<PathBuf, AppError> {
    let source = source(provider)
        .ok_or_else(|| err!("error.tunnelNoDownload", program = provider.program()))?;
    let dir = data_dir.join("tunnel");
    std::fs::create_dir_all(&dir).map_err(|e| {
        err!(
            "error.cannotCreateDirectory",
            path = dir.display(),
            message = e
        )
    })?;
    let staging = dir.join("download.part");
    let _ = std::fs::remove_file(&staging);
    download(&source.url, &staging, report)?;
    verify_sha256(&staging, source.sha256)?;
    let target = downloaded_path(provider, data_dir);
    let unpacked = match source.shape {
        Shape::Bare => staging.clone(),
        Shape::Tgz => {
            run(
                "tar",
                &[
                    "-xzf",
                    &staging.to_string_lossy(),
                    "-C",
                    &dir.to_string_lossy(),
                    source.member,
                ],
                "error.unpackFailed",
            )?;
            let _ = std::fs::remove_file(&staging);
            dir.join(source.member)
        }
    };
    if unpacked != target {
        std::fs::rename(&unpacked, &target).map_err(|e| {
            err!(
                "error.cannotWriteFile",
                path = target.display(),
                message = e
            )
        })?;
    }
    make_executable(&target)?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGETS: [(&str, &str); 6] = [
        ("windows", "x86_64"),
        ("windows", "aarch64"),
        ("macos", "x86_64"),
        ("macos", "aarch64"),
        ("linux", "x86_64"),
        ("linux", "aarch64"),
    ];

    #[test]
    fn every_target_mixlab_ships_for_has_a_pinned_source() {
        for (os, arch) in TARGETS {
            let source =
                source_for(Provider::Cloudflare, os, arch).unwrap_or_else(|| panic!("{os}/{arch}"));
            assert_eq!(source.sha256.len(), 64, "{os}/{arch}");
            assert!(source.url.contains(CLOUDFLARED_VERSION), "{}", source.url);
        }
    }

    /// Pinned by version: ngrok's `-stable-` links move with every release, and a digest pinned
    /// against one breaks on the next.
    #[test]
    fn every_target_has_a_pinned_ngrok() {
        for (os, arch) in TARGETS {
            let source =
                source_for(Provider::Ngrok, os, arch).unwrap_or_else(|| panic!("{os}/{arch}"));
            assert_eq!(source.sha256.len(), 64, "{os}/{arch}");
            assert!(
                source.url.starts_with("https://bin.equinox.io/a/"),
                "{}",
                source.url
            );
            assert!(source.url.contains(NGROK_VERSION) && source.url.ends_with(".tar.gz"));
            assert!(!source.url.contains("stable"), "{}", source.url);
            assert_eq!(source.shape, Shape::Tgz);
        }
    }

    #[test]
    fn windows_on_arm_takes_the_x86_64_cloudflared() {
        assert_eq!(
            source_for(Provider::Cloudflare, "windows", "aarch64"),
            source_for(Provider::Cloudflare, "windows", "x86_64")
        );
    }

    #[test]
    fn windows_on_arm_takes_ngrok_s_own_arm64_build() {
        assert_ne!(
            source_for(Provider::Ngrok, "windows", "aarch64"),
            source_for(Provider::Ngrok, "windows", "x86_64")
        );
    }

    #[test]
    fn macos_is_an_archive_and_the_others_are_the_program() {
        assert_eq!(
            source_for(Provider::Cloudflare, "macos", "aarch64").map(|s| s.shape),
            Some(Shape::Tgz)
        );
        assert_eq!(
            source_for(Provider::Cloudflare, "linux", "x86_64").map(|s| s.shape),
            Some(Shape::Bare)
        );
    }

    /// ngrok's Windows archive holds `ngrok`, not `ngrok.exe`: what is unpacked is renamed.
    #[test]
    fn the_unpacked_member_is_the_bare_program_name() {
        for (os, arch) in TARGETS {
            assert_eq!(
                source_for(Provider::Ngrok, os, arch).map(|s| s.member),
                Some("ngrok")
            );
        }
        assert_eq!(
            source_for(Provider::Cloudflare, "macos", "x86_64").map(|s| s.member),
            Some("cloudflared")
        );
    }

    /// The testable core of `find`, handed an empty `PATH` so a developer's own cloudflared
    /// cannot answer for it.
    #[test]
    fn a_configured_path_that_is_not_a_file_is_not_used() {
        let dir = std::env::temp_dir().join(format!("tunnel-bin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a directory");
        let found = find_in(
            Provider::Cloudflare,
            Some(&dir.join("missing")),
            &dir,
            OsStr::new(""),
        );
        assert!(found.is_none());
    }

    #[test]
    fn mixlab_s_own_copy_answers_when_nothing_else_does() {
        let dir = std::env::temp_dir().join(format!("tunnel-own-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("tunnel")).expect("a directory");
        std::fs::write(downloaded_path(Provider::Cloudflare, &dir), b"x").expect("a file");
        let found = find_in(Provider::Cloudflare, None, &dir, OsStr::new("")).expect("found");
        assert_eq!(found.origin, "downloaded");
    }

    #[test]
    fn each_provider_keeps_its_own_downloaded_copy() {
        let dir = std::env::temp_dir().join(format!("tunnel-ngrok-own-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("tunnel")).expect("a directory");
        std::fs::write(downloaded_path(Provider::Ngrok, &dir), b"x").expect("a file");
        assert_eq!(
            find_in(Provider::Ngrok, None, &dir, OsStr::new("")).map(|f| f.origin),
            Some("downloaded")
        );
        assert!(find_in(Provider::Cloudflare, None, &dir, OsStr::new("")).is_none());
    }
}
