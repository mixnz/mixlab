//! A release archive downloaded and unpacked into a new project — roadmap task **T205**, D2.
//!
//! **Unpinned on purpose.** `latest.zip` changes with every release, so a checksum in a gallery
//! file would break it within weeks; HTTPS and the consent a person gives are the trust (D2). The
//! unpacking is `core::install::archive`'s, whose path checks already refuse an entry that leaves
//! the directory it is unpacked into.

use std::path::Path;

use crate::install::archive::{self, Format};
use crate::{Error, Result};

/// The largest archive this fetches: 1 GiB (D2).
pub const LIMIT: u64 = 1 << 30;

/// Whether the URL's suffix names a format this build unpacks — the plan's question (D2).
#[must_use]
pub fn format_known(url: &str) -> bool {
    Format::of(url).is_some()
}

/// Whether `strip` is one folder name, the only thing it may be: no separator of either system, no
/// drive, and neither `.` nor `..`. Anything else would reach outside the unpacked tree and move
/// what it found there into the project — and the consent a person gives names the URL, not this.
#[must_use]
pub fn is_folder_name(strip: &str) -> bool {
    !strip.is_empty() && strip != "." && strip != ".." && !strip.contains(['/', '\\', ':'])
}

/// How the unpacked tree reaches the project root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mover {
    /// Rename where it can, copy where the two are on different volumes (ADR 0040).
    RenameOrCopy,

    /// Always copy: the seam the cross-volume test drives.
    #[cfg(test)]
    CopyOnly,
}

/// Download `url`, unpack it into `staging`, and move its contents (or `strip`'s) into `root`.
///
/// `staging` is removed whatever happens. `root` is created when it is not there. Takes the
/// [`Installer`](crate::install::Installer) rather than an HTTP client, so the daemon fetches with
/// this product's one client and never depends on `reqwest` itself.
///
/// # Errors
///
/// [`Error::ArchiveFormatUnknown`] before any download; [`Error::ArtifactTransport`] and
/// [`Error::ArchiveTooLarge`] for the transfer; [`Error::ArchiveStripMissing`] when `strip` is not
/// at the archive's top level; and the unpacker's own refusals.
pub async fn fetch(
    installer: &crate::install::Installer,
    url: &str,
    strip: Option<&str>,
    staging: &Path,
    root: &Path,
) -> Result<()> {
    fetch_using(installer.http(), url, strip, staging, root).await
}

/// [`fetch`] with the client named, which is what a test hands a plain one to.
async fn fetch_using(
    http: &reqwest::Client,
    url: &str,
    strip: Option<&str>,
    staging: &Path,
    root: &Path,
) -> Result<()> {
    let format = Format::of(url).ok_or_else(|| Error::ArchiveFormatUnknown {
        url: url.to_owned(),
    })?;

    let _ = tokio::fs::remove_dir_all(staging).await;
    crate::paths::create_dir(staging)?;

    let outcome = fetch_into(http, url, format, strip, staging, root).await;

    let _ = tokio::fs::remove_dir_all(staging).await;
    outcome
}

async fn fetch_into(
    http: &reqwest::Client,
    url: &str,
    format: Format,
    strip: Option<&str>,
    staging: &Path,
    root: &Path,
) -> Result<()> {
    let file = staging.join("download");
    download(http, url, &file).await?;

    let unpacked = staging.join("unpacked");
    crate::paths::create_dir(&unpacked)?;

    let (from, into) = (file.clone(), unpacked.clone());
    tokio::task::spawn_blocking(move || archive::extract(&from, format, &into))
        .await
        .map_err(|error| Error::Io {
            action: "unpack",
            path: file.clone(),
            source: std::io::Error::other(error),
        })??;

    let source = match strip {
        None => unpacked.clone(),
        Some(strip) => {
            let candidate = unpacked.join(strip);
            // The manifest refuses such a `strip`; this is the second wall, for any other caller.
            if !is_folder_name(strip) || !candidate.is_dir() {
                return Err(Error::ArchiveStripMissing {
                    url: url.to_owned(),
                    strip: strip.to_owned(),
                    found: top_level(&unpacked),
                });
            }
            candidate
        }
    };

    crate::paths::create_dir(root)?;
    let (from, to) = (source.clone(), root.to_path_buf());
    tokio::task::spawn_blocking(move || move_children(&from, &to, Mover::RenameOrCopy))
        .await
        .map_err(|error| Error::Io {
            action: "move",
            path: source,
            source: std::io::Error::other(error),
        })?
}

/// The transfer, with the ceiling (D2). `prerequisites::download`'s shape, unpinned the same way.
async fn download(http: &reqwest::Client, url: &str, into: &Path) -> Result<()> {
    use tokio::io::AsyncWriteExt;

    let transport = |source: reqwest::Error| Error::ArtifactTransport {
        url: url.to_owned(),
        source: Box::new(source),
    };
    let write = |source: std::io::Error| Error::Io {
        action: "write",
        path: into.to_path_buf(),
        source,
    };

    let mut response = http
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(transport)?;

    let mut file = tokio::fs::File::create(into).await.map_err(write)?;
    let mut written = 0u64;

    while let Some(chunk) = response.chunk().await.map_err(transport)? {
        written += u64::try_from(chunk.len()).unwrap_or(u64::MAX);
        if written > LIMIT {
            return Err(Error::ArchiveTooLarge {
                url: url.to_owned(),
                limit: LIMIT,
            });
        }
        file.write_all(&chunk).await.map_err(write)?;
    }

    file.flush().await.map_err(write)
}

/// The names at the archive's top level, sorted, for the refusal (D2).
fn top_level(unpacked: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(unpacked)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// Move every child of `from` into `to`.
fn move_children(from: &Path, to: &Path, mover: Mover) -> Result<()> {
    let read = |source| Error::Io {
        action: "read",
        path: from.to_path_buf(),
        source,
    };

    for entry in std::fs::read_dir(from).map_err(read)? {
        let entry = entry.map_err(read)?;
        let target = to.join(entry.file_name());

        let renamed = match mover {
            Mover::RenameOrCopy => std::fs::rename(entry.path(), &target).is_ok(),
            #[cfg(test)]
            Mover::CopyOnly => false,
        };

        if !renamed {
            copy_tree(&entry.path(), &target)?;
        }
    }

    Ok(())
}

/// A recursive copy, for the cross-volume case.
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    let io = |action: &'static str, path: &Path| {
        let path = path.to_path_buf();
        move |source| Error::Io {
            action,
            path,
            source,
        }
    };

    if from.is_dir() {
        std::fs::create_dir_all(to).map_err(io("create", to))?;
        for entry in std::fs::read_dir(from).map_err(io("read", from))? {
            let entry = entry.map_err(io("read", from))?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to)
            .map(|_| ())
            .map_err(io("copy", from))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn zip_of(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut bytes);
            let options = zip::write::SimpleFileOptions::default();
            for (name, body) in entries {
                writer.start_file(*name, options).expect("an entry");
                writer.write_all(body.as_bytes()).expect("its body");
            }
            writer.finish().expect("a zip");
        }
        bytes.into_inner()
    }

    async fn served(bytes: Vec<u8>) -> (mixengine_testkit::registry::MockRegistry, String) {
        let registry =
            mixengine_testkit::registry::MockRegistry::start(&serde_json::json!({})).await;
        let url = registry.publish_asset("/latest.zip", bytes);
        (registry, url)
    }

    #[tokio::test]
    async fn the_strip_folder_becomes_the_root() {
        let (_registry, url) = served(zip_of(&[
            ("wordpress/index.php", "<?php"),
            ("wordpress/wp-admin/install.php", "<?php"),
        ]))
        .await;
        let temp = tempfile::tempdir().expect("a temp dir");
        let root = temp.path().join("site");
        let staging = temp.path().join("staging");

        fetch_using(
            &reqwest::Client::new(),
            &url,
            Some("wordpress"),
            &staging,
            &root,
        )
        .await
        .expect("fetched");

        assert!(root.join("index.php").is_file());
        assert!(root.join("wp-admin").join("install.php").is_file());
        assert!(!staging.exists(), "the staging directory is left behind");
    }

    #[tokio::test]
    async fn without_strip_the_whole_archive_is_the_root() {
        let (_registry, url) = served(zip_of(&[("index.php", "<?php")])).await;
        let temp = tempfile::tempdir().expect("a temp dir");
        let root = temp.path().join("site");

        fetch_using(
            &reqwest::Client::new(),
            &url,
            None,
            &temp.path().join("s"),
            &root,
        )
        .await
        .expect("fetched");

        assert!(root.join("index.php").is_file());
    }

    /// **A `strip` that leaves the unpacked tree moves nothing** — the manifest refuses one, and
    /// this is the second wall, for a caller that did not read a manifest.
    #[tokio::test]
    async fn a_strip_outside_the_archive_moves_nothing() {
        let (_registry, url) = served(zip_of(&[("index.php", "<?php")])).await;
        let temp = tempfile::tempdir().expect("a temp dir");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&outside).expect("a folder");
        std::fs::write(outside.join("keep.txt"), "mine").expect("a file");
        let root = temp.path().join("site");

        let error = fetch_using(
            &reqwest::Client::new(),
            &url,
            Some("../../outside"),
            &temp.path().join("staging"),
            &root,
        )
        .await
        .expect_err("refused");

        assert!(error.to_string().contains("outside"), "{error}");
        assert!(
            outside.join("keep.txt").is_file(),
            "the folder outside was moved"
        );
        assert!(!root.join("keep.txt").exists());
    }

    #[tokio::test]
    async fn a_missing_strip_fails_and_names_the_top_level() {
        let (_registry, url) = served(zip_of(&[
            ("joomla/index.php", "<?php"),
            ("extra/readme", "x"),
        ]))
        .await;
        let temp = tempfile::tempdir().expect("a temp dir");
        let root = temp.path().join("site");
        let staging = temp.path().join("staging");

        let error = fetch_using(
            &reqwest::Client::new(),
            &url,
            Some("wordpress"),
            &staging,
            &root,
        )
        .await
        .expect_err("refused");

        let said = error.to_string();
        assert!(
            said.contains("wordpress") && said.contains("joomla") && said.contains("extra"),
            "{said}"
        );
        assert!(!root.exists() || std::fs::read_dir(&root).expect("readable").next().is_none());
        assert!(!staging.exists());
    }

    #[tokio::test]
    async fn an_unknown_suffix_is_refused_before_downloading() {
        let temp = tempfile::tempdir().expect("a temp dir");
        let error = fetch_using(
            &reqwest::Client::new(),
            "https://example.invalid/latest.rar",
            None,
            &temp.path().join("s"),
            &temp.path().join("r"),
        )
        .await
        .expect_err("refused");

        assert!(
            matches!(error, Error::ArchiveFormatUnknown { .. }),
            "{error:?}"
        );
    }

    #[test]
    fn moving_falls_back_to_copying() {
        let temp = tempfile::tempdir().expect("a temp dir");
        let from = temp.path().join("from");
        std::fs::create_dir_all(from.join("sub")).expect("dirs");
        std::fs::write(from.join("sub").join("a.txt"), "a").expect("a file");
        let to = temp.path().join("to");
        std::fs::create_dir_all(&to).expect("dir");

        move_children(&from, &to, Mover::CopyOnly).expect("moved");

        assert_eq!(
            std::fs::read_to_string(to.join("sub").join("a.txt")).expect("read"),
            "a"
        );
    }
}
