//! The Microsoft Visual C++ Redistributable, fetched from the one address MixEngine lets name it —
//! roadmap task **T150**, T148 design D6.
//!
//! **Written into the build, and nowhere else.** Not the index and not configuration: nothing a
//! third party can edit chooses the file that is about to ask Windows for administrator rights.
//! What is fetched is not believed here — `mixengine_platform`'s `Redistributables` does that,
//! against Microsoft's signature, immediately before it runs.

#[cfg(feature = "network")]
use std::path::PathBuf;

use mixengine_proto::RedistributableArch;
#[cfg(feature = "network")]
use tokio::io::AsyncWriteExt as _;

#[cfg(feature = "network")]
use crate::install::Installer;
#[cfg(feature = "network")]
use crate::{Error, InstallError, Result};

/// Microsoft's permanent address for the x64 redistributable.
pub const VISUAL_CPP_X64: &str = "https://aka.ms/vs/17/release/vc_redist.x64.exe";

/// Microsoft's permanent address for the ARM64 redistributable.
pub const VISUAL_CPP_ARM64: &str = "https://aka.ms/vs/17/release/vc_redist.arm64.exe";

/// The most this download may be. Measured 2026-09-16 at 25.6 MB for x64.
#[cfg(feature = "network")]
const LIMIT: u64 = 64 * 1024 * 1024;

/// Where the redistributable for `arch` is fetched from.
#[must_use]
pub const fn url(arch: RedistributableArch) -> &'static str {
    match arch {
        RedistributableArch::X64 => VISUAL_CPP_X64,
        RedistributableArch::Arm64 => VISUAL_CPP_ARM64,
    }
}

/// Fetch it whole into the downloads directory, replacing any earlier copy.
///
/// **Not resumed**: it has no digest to resume against, and 25 MB is less than a PHP.
///
/// # Errors
///
/// [`InstallError::ArtifactTransport`] for the fetch, [`InstallError::ArtifactTooLarge`] past the
/// limit, and [`Error::Io`] for the file.
#[cfg(feature = "network")]
pub async fn download(installer: &Installer, arch: RedistributableArch) -> Result<PathBuf> {
    let url = url(arch);
    crate::paths::create_dir(installer.downloads())?;
    let into = installer
        .downloads()
        .join(format!("vc_redist.{}.exe", arch.as_str()));

    let transport = |source: reqwest::Error| {
        Error::Install(InstallError::ArtifactTransport {
            url: url.to_owned(),
            source: Box::new(source),
        })
    };
    let write = |source: std::io::Error| Error::Io {
        action: "write",
        path: into.clone(),
        source,
    };

    let mut response = installer
        .http()
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(transport)?;

    let mut file = tokio::fs::File::create(&into).await.map_err(write)?;
    let mut written = 0u64;

    while let Some(chunk) = response.chunk().await.map_err(transport)? {
        written += u64::try_from(chunk.len()).unwrap_or(u64::MAX);
        if written > LIMIT {
            drop(file);
            let _ = tokio::fs::remove_file(&into).await;
            return Err(Error::Install(InstallError::ArtifactTooLarge {
                url: url.to_owned(),
                expected: LIMIT,
            }));
        }
        file.write_all(&chunk).await.map_err(write)?;
    }

    file.sync_all().await.map_err(write)?;
    Ok(into)
}

#[cfg(test)]
#[cfg(feature = "network")]
mod tests {
    use super::*;

    #[test]
    fn each_build_has_its_own_address() {
        assert_eq!(url(RedistributableArch::X64), VISUAL_CPP_X64);
        assert_eq!(url(RedistributableArch::Arm64), VISUAL_CPP_ARM64);
    }

    #[tokio::test]
    #[ignore = "reaches Microsoft; run with --ignored from the release checklist"]
    async fn microsofts_address_serves_a_windows_program_under_the_limit() {
        let cache =
            std::env::temp_dir().join(format!("mixengine-t150-core-{}", std::process::id()));
        let installer = Installer::new(&cache).expect("an HTTP client");

        let path = download(&installer, RedistributableArch::X64)
            .await
            .expect("a download");
        let bytes = std::fs::read(&path).expect("the file");

        assert!(bytes.starts_with(b"MZ"), "a PE image");
        assert!(u64::try_from(bytes.len()).expect("a length") < LIMIT);
    }
}
