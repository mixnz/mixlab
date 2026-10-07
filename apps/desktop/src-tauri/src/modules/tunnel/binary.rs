//! Which `cloudflared` a tunnel runs — T203, D2. The machine's own first, at a path set in Settings
//! or on `PATH`; then one MixLab downloaded, pinned here and checked against its SHA-256 before it
//! is unpacked or run. Never put on `PATH`: it belongs to MixLab, not to the machine.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::downloads::{download, make_executable, run, verify_sha256};
use crate::error::AppError;

/// The version MixLab downloads. Bumped with its digests by `bumping-tool-downloads.md`; a new
/// cloudflared arrives with a new MixLab and never on its own.
pub const VERSION: &str = "2026.10.0";

/// What the download is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// A `.tgz` holding `cloudflared` — macOS.
    Tgz,
    /// The program itself — Linux and Windows.
    Bare,
}

/// One pinned download: GitHub's published digest and size for the asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub url: String,
    pub sha256: &'static str,
    pub size: u64,
    pub shape: Shape,
}

/// The pinned download for a system, or `None` where upstream publishes nothing. Windows on ARM
/// runs the x86-64 build emulated: upstream publishes no ARM64 one.
pub fn source_for(os: &str, arch: &str) -> Option<Source> {
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
            "https://github.com/cloudflare/cloudflared/releases/download/{VERSION}/{asset}"
        ),
        sha256,
        size,
        shape,
    })
}

/// This machine's download.
pub fn source() -> Option<Source> {
    source_for(std::env::consts::OS, std::env::consts::ARCH)
}

/// `cloudflared`, with this system's executable suffix.
pub fn file_name() -> String {
    format!("cloudflared{}", std::env::consts::EXE_SUFFIX)
}

/// Where MixLab keeps its own copy.
pub fn downloaded_path(data_dir: &Path) -> PathBuf {
    data_dir.join("tunnel").join(file_name())
}

/// A `cloudflared` that was found, and where it came from.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Found {
    pub path: PathBuf,
    /// `settings`, `path` or `downloaded` — what the tab says about where it came from.
    pub origin: &'static str,
}

/// The `cloudflared` a tunnel would run.
pub fn find(configured: Option<&Path>, data_dir: &Path) -> Option<Found> {
    find_in(
        configured,
        data_dir,
        &std::env::var_os("PATH").unwrap_or_default(),
    )
}

fn find_in(configured: Option<&Path>, data_dir: &Path, path_var: &OsStr) -> Option<Found> {
    if let Some(path) = configured.filter(|p| p.is_file()) {
        return Some(Found {
            path: path.to_path_buf(),
            origin: "settings",
        });
    }
    if let Some(path) = mixengine_platform::process::program_on_path("cloudflared", path_var) {
        return Some(Found {
            path,
            origin: "path",
        });
    }
    let own = downloaded_path(data_dir);
    own.is_file().then_some(Found {
        path: own,
        origin: "downloaded",
    })
}

/// Downloads this machine's pinned `cloudflared` into the data directory and answers its path.
pub fn install(data_dir: &Path, report: &dyn Fn(u64, u64)) -> Result<PathBuf, AppError> {
    let source = source().ok_or_else(|| err!("error.tunnelNoDownload"))?;
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
    let target = downloaded_path(data_dir);
    match source.shape {
        Shape::Bare => std::fs::rename(&staging, &target).map_err(|e| {
            err!(
                "error.cannotWriteFile",
                path = target.display(),
                message = e
            )
        })?,
        Shape::Tgz => {
            run(
                "tar",
                &[
                    "-xzf",
                    &staging.to_string_lossy(),
                    "-C",
                    &dir.to_string_lossy(),
                    "cloudflared",
                ],
                "error.unpackFailed",
            )?;
            let _ = std::fs::remove_file(&staging);
        }
    }
    make_executable(&target)?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_target_mixlab_ships_for_has_a_pinned_source() {
        for (os, arch) in [
            ("windows", "x86_64"),
            ("windows", "aarch64"),
            ("macos", "x86_64"),
            ("macos", "aarch64"),
            ("linux", "x86_64"),
            ("linux", "aarch64"),
        ] {
            let source = source_for(os, arch).unwrap_or_else(|| panic!("{os}/{arch}"));
            assert_eq!(source.sha256.len(), 64, "{os}/{arch}");
            assert!(source.url.contains(VERSION), "{}", source.url);
        }
    }

    #[test]
    fn windows_on_arm_takes_the_x86_64_build() {
        assert_eq!(
            source_for("windows", "aarch64"),
            source_for("windows", "x86_64")
        );
    }

    #[test]
    fn macos_is_an_archive_and_the_others_are_the_program() {
        assert_eq!(
            source_for("macos", "aarch64").map(|s| s.shape),
            Some(Shape::Tgz)
        );
        assert_eq!(
            source_for("linux", "x86_64").map(|s| s.shape),
            Some(Shape::Bare)
        );
    }

    /// The testable core of `find`, handed an empty `PATH` so a developer's own cloudflared
    /// cannot answer for it.
    #[test]
    fn a_configured_path_that_is_not_a_file_is_not_used() {
        let dir = std::env::temp_dir().join(format!("t203-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a directory");
        let found = find_in(Some(&dir.join("missing")), &dir, std::ffi::OsStr::new(""));
        assert!(found.is_none());
    }

    #[test]
    fn mixlab_s_own_copy_answers_when_nothing_else_does() {
        let dir = std::env::temp_dir().join(format!("t203-own-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("tunnel")).expect("a directory");
        std::fs::write(downloaded_path(&dir), b"x").expect("a file");
        let found = find_in(None, &dir, std::ffi::OsStr::new("")).expect("found");
        assert_eq!(found.origin, "downloaded");
    }
}
