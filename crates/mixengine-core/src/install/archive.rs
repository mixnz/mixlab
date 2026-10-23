//! Unpacking a downloaded artifact, without letting it choose where its bytes land.
//!
//! # The entry path is checked here, not only by the crate that writes it
//!
//! Both `tar` and `zip` already refuse to write outside the directory they are handed —
//! `Entry::unpack_in` returns `false` and `ZipFile::enclosed_name` returns [`None`]. This module
//! checks the path itself anyway, *before* either is asked to write, for two reasons that are not
//! the same reason. The refusal becomes ours, so it has a message naming the entry and a test that
//! holds it; and the check stops being a property of whichever crate version is resolved this week,
//! for what is the single most attacked step in installing anything.
//!
//! What is deliberately **not** re-implemented here is everything below the path: the mode bits, the
//! symlinks, the hardlinks. Those are the operating system's business, `docs/standards/rust.md`
//! says a `#[cfg]` outside `mixengine-platform` fails review, and a hand-rolled unpacker would need
//! one per entry type. Delegating that to the crate whose job it is *is* the platform abstraction
//! here.
//!
//! # Why the format comes from the URL
//!
//! The URL is a field of a document this build has already verified an Ed25519 signature over, so
//! its suffix is as trustworthy as the SHA-256 beside it — and it is the same key
//! `tools/mkindex.py` writes the index from. Sniffing magic bytes instead would read a claim made by
//! the downloaded file about itself, which is a strictly weaker statement than one we signed.

#[cfg(feature = "network")]
use std::io::{BufReader, Read};
use std::path::{Component, Path};

#[cfg(feature = "network")]
use crate::{Error, InstallError, Result};

/// One of the three shapes the publishing pipeline produces.
///
/// A closed set, and it matches `ARCHIVE_SUFFIXES` in that pipeline's `mkindex.py`: an index naming
/// a fourth is describing an artifact this build has no decompressor for, which is a refusal with a
/// reason rather than something to guess at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    /// `.zip` — what Windows artifacts are packed as.
    Zip,
    /// `.tar.gz` — the fallback the pipeline packs when the runner's `tar` has no zstd.
    TarGz,
    /// `.tar.zst` — what macOS and Linux artifacts are packed as.
    TarZst,
}

impl Format {
    /// Read the format off a URL, or [`None`] for one this build cannot unpack.
    ///
    /// Longest suffix first, because `.tar.gz` also ends with `.gz` and a shorter match would pick
    /// the wrong container for the right compressor.
    pub(crate) fn of(url: &str) -> Option<Self> {
        // A query string or fragment is not part of the file name. Neither appears in a published
        // URL today; a mirror that adds one should still install rather than fail to classify.
        let path = url.split(['?', '#']).next().unwrap_or(url);

        if path.ends_with(".tar.zst") {
            Some(Self::TarZst)
        } else if path.ends_with(".tar.gz") {
            Some(Self::TarGz)
        } else if path.ends_with(".zip") {
            Some(Self::Zip)
        } else {
            None
        }
    }
}

/// Unpack `archive` into `into`, which must already exist and should be empty.
///
/// **Blocking on purpose.** Every byte of an eighty-megabyte archive is decompressed and written by
/// this call, so it belongs on a blocking thread; the caller in [`super`] is what puts it there, on
/// the rule in `docs/standards/rust.md` that nothing blocks the runtime.
///
/// # Errors
///
/// [`InstallError::UnsafeArchiveEntry`] when an entry names a path outside `into`,
/// [`InstallError::ArchiveUnreadable`] when the container or the compressor refuses what it was
/// given, and [`Error::Io`] when the file cannot be opened.
#[cfg(feature = "network")]
pub(crate) fn extract(archive: &Path, format: Format, into: &Path) -> Result<()> {
    let file = std::fs::File::open(archive).map_err(|source| Error::Io {
        action: "open",
        path: archive.to_path_buf(),
        source,
    })?;
    // Every one of the three reads the file in one forward pass, so a buffer in front of it is the
    // difference between one syscall per block and one per `read` the decompressor asks for.
    let file = BufReader::new(file);

    match format {
        Format::Zip => unpack_zip(file, archive, into),
        Format::TarGz => unpack_tar(flate2::read::GzDecoder::new(file), archive, into),
        Format::TarZst => {
            let stream = ruzstd::decoding::StreamingDecoder::new(file)
                .map_err(|source| unreadable(archive, source))?;
            unpack_tar(stream, archive, into)
        }
    }
}

/// Whether an entry may be written at all.
///
/// Accepts a path made only of ordinary names and `.` — the `./bin/php` shape every `tar -C tree .`
/// entry has — and refuses everything that could mean somewhere else: `..`, a leading `/`, and a
/// Windows prefix such as `C:` or `\\?\`. A path that names nothing at all is refused too, the empty
/// one and the bare `./` alike: as a recorded path it is a row that runs no file, and as an archive
/// entry it is the root that [`names_the_destination`] takes out of the unpackers' way before they
/// ask this.
///
/// `pub(crate)` rather than `pub(super)` since T25: [`crate::runtimes::program`] asks the same
/// question of the same value months later. What an archive entry was allowed to be when it was
/// unpacked and what a recorded path is allowed to be when it is run have to be one rule, or the
/// second reader is a way around the first.
pub(crate) fn safe(path: &Path) -> bool {
    let mut named = false;
    for component in path.components() {
        match component {
            Component::Normal(_) => named = true,
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    named
}

/// Whether an entry names the directory it is being unpacked into rather than anything inside it.
///
/// **The `./` that GNU and BSD `tar` write first.** `tar -C tree .` records the directory it was
/// pointed at as an entry of its own, and that name resolves to the destination — which already
/// exists, and which nothing has to write. [`safe`] answers `false` for it, correctly for the
/// question it is asked elsewhere, where a path naming no file is a row that can run nothing; the
/// unpackers ask this one first and skip what it accepts, so the check after it only ever sees
/// entries that mean somewhere.
///
/// Refusing it instead was a real failure and worth naming: no zip carries such an entry, so every
/// Windows install went on working while every macOS and Linux one refused its own artifact at the
/// unpack step.
#[cfg(feature = "network")]
fn names_the_destination(path: &Path) -> bool {
    path.components()
        .all(|component| component == Component::CurDir)
}

/// Unpack a zip, refusing any entry that names somewhere else first.
///
/// The names are walked in full before a single byte is written, so an archive whose last entry is
/// the malicious one does not get to leave the first hundred behind. `by_index_raw` reads the entry
/// header without starting its decompressor, which is what makes that first pass cheap.
#[cfg(feature = "network")]
fn unpack_zip<R: Read + std::io::Seek>(file: R, archive: &Path, into: &Path) -> Result<()> {
    let mut zip = zip::ZipArchive::new(file).map_err(|source| unreadable(archive, source))?;

    for index in 0..zip.len() {
        let entry = zip
            .by_index_raw(index)
            .map_err(|source| unreadable(archive, source))?;

        // `enclosed_name` is `None` for an absolute path, a `..`, and — the one a reader forgets —
        // a name whose backslashes make it a different path on Windows than on Unix.
        let refused = match entry.enclosed_name() {
            Some(path) => !safe(&path) && !names_the_destination(&path),
            None => true,
        };
        if refused {
            return Err(Error::Install(InstallError::UnsafeArchiveEntry {
                archive: archive.to_path_buf(),
                // A name that will not decode is still named, as best its bytes allow.
                entry: entry.name().map_or_else(
                    |_| String::from_utf8_lossy(entry.name_raw()).into_owned(),
                    std::borrow::Cow::into_owned,
                ),
            }));
        }
    }

    zip.extract(into)
        .map_err(|source| unreadable(archive, source))
}

/// Unpack a tar stream, one entry at a time.
///
/// Streamed rather than walked twice: a `.tar.zst` cannot be rewound without decompressing it
/// again, so the two-pass shape [`unpack_zip`] uses would cost a second full decompression. The trade is
/// that a refusal here can leave earlier entries on disk — which costs nothing, because the caller
/// removes the staging directory on any failure and nothing has been renamed into place yet.
///
/// `preserve_permissions` is set because it is **off by default** in `tar`, and a PHP unpacked
/// without its mode bits is a `php` that cannot be executed — a failure that would surface later,
/// somewhere else, as a permission error nobody would trace back to here.
#[cfg(feature = "network")]
fn unpack_tar<R: Read>(reader: R, archive: &Path, into: &Path) -> Result<()> {
    let mut tar = tar::Archive::new(reader);
    tar.set_preserve_permissions(true);
    tar.set_overwrite(true);
    // Extended attributes are the publisher's machine leaking into the user's — quarantine flags,
    // SELinux labels — and none of them is something this project means to carry.
    tar.set_unpack_xattrs(false);

    for entry in tar
        .entries()
        .map_err(|source| unreadable(archive, source))?
    {
        let mut entry = entry.map_err(|source| unreadable(archive, source))?;

        let path = entry
            .path()
            .map_err(|source| unreadable(archive, source))?
            .into_owned();
        let refused = |named: &Path| {
            Error::Install(InstallError::UnsafeArchiveEntry {
                archive: archive.to_path_buf(),
                entry: named.display().to_string(),
            })
        };

        // The archive's own root, which is the directory these bytes are already going into.
        // Skipped rather than checked: there is nothing to write, and `unpack_in` — which stops at
        // the same entry, for the same reason — would answer `true` having done nothing.
        if names_the_destination(&path) {
            continue;
        }

        if !safe(&path) {
            return Err(refused(&path));
        }

        // Asked as well as checked. `unpack_in` answers `false` for what it will not write, and the
        // cases it catches and the check above does not are the ones a symlink makes: an entry
        // whose *parent* is a link somebody planted earlier in the same archive.
        if !entry
            .unpack_in(into)
            .map_err(|source| unreadable(archive, source))?
        {
            return Err(refused(&path));
        }
    }

    Ok(())
}

/// Blame the archive rather than the reader, for anything a decompressor refuses.
#[cfg(feature = "network")]
fn unreadable(
    archive: &Path,
    source: impl std::error::Error + Send + Sync + 'static,
) -> crate::Error {
    Error::Install(InstallError::ArchiveUnreadable {
        archive: archive.to_path_buf(),
        source: Box::new(source),
    })
}

#[cfg(test)]
#[cfg(feature = "network")]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn the_three_published_shapes_are_recognised_and_nothing_else_is() {
        let format = |name: &str| Format::of(&format!("https://example.invalid/php-8.3.33{name}"));

        assert_eq!(format(".zip"), Some(Format::Zip));
        assert_eq!(format(".tar.gz"), Some(Format::TarGz));
        assert_eq!(format(".tar.zst"), Some(Format::TarZst));

        // `.tar.gz` ends with `.gz`, and a bare `.gz` is not a container this can unpack.
        assert_eq!(format(".gz"), None);
        assert_eq!(format(".tar"), None);
        assert_eq!(format(".7z"), None);
        assert_eq!(format(""), None);
    }

    #[test]
    fn a_query_string_does_not_hide_the_format() {
        assert_eq!(
            Format::of("https://mirror.invalid/php.tar.zst?token=abc#part"),
            Some(Format::TarZst)
        );
    }

    #[test]
    fn an_entry_that_names_somewhere_else_is_refused() {
        for name in [
            "../outside",
            "a/../../outside",
            "/etc/passwd",
            "",
            ".",
            #[cfg(windows)]
            r"C:\windows\system32",
            #[cfg(windows)]
            r"..\outside",
        ] {
            assert!(!safe(&PathBuf::from(name)), "{name:?} should be refused");
        }
    }

    /// The first entry of every artifact this project publishes for macOS and Linux, and the one
    /// [`safe`] refuses on its own: it names the destination, so the unpackers step over it.
    #[test]
    fn the_root_entry_tar_writes_first_names_the_destination() {
        for name in ["./", ".", "./.", ""] {
            assert!(
                names_the_destination(&PathBuf::from(name)),
                "{name:?} is the destination itself"
            );
        }

        for name in ["bin", "./bin/php", "..", "/"] {
            assert!(
                !names_the_destination(&PathBuf::from(name)),
                "{name:?} names something other than the destination"
            );
        }
    }

    #[test]
    fn an_ordinary_entry_is_allowed_including_the_leading_dot_tar_writes() {
        for name in ["php.exe", "bin/php", "./bin/php", "ext/php_curl.dll"] {
            assert!(safe(&PathBuf::from(name)), "{name:?} should be allowed");
        }
    }
}
