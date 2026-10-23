//! The half of an install that fetches: [`Installer`], which downloads what the index names into
//! `cache/downloads/`, resuming what an earlier attempt left there, and hands the verified archive
//! to the staging directory and the rename the parent module describes.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::AsyncWriteExt as _;

use super::{
    InstallError, Installed, NotAnArchive, SmokeTest, Watcher, archive, blocking, check_runs,
    present, sha256,
};
use crate::index::Artifact;
use crate::{Error, Result, RuntimeError, paths};

/// Where partial downloads live, under the cache directory.
///
/// Its own directory rather than files beside `index.json`, so that emptying it is a repair somebody
/// can be told to perform without being told to be careful about which files.
const DOWNLOADS_DIR_NAME: &str = "downloads";

/// The suffix a download carries until it is known to be complete and correct.
const PART_SUFFIX: &str = ".part";

/// How long a connection may take to be established.
///
/// A download has no *total* budget on purpose — an artifact is tens of megabytes and a slow
/// connection is not a broken one, so a ceiling large enough to be safe would be too large to mean
/// anything. What is bounded instead is silence: this, and [`READ_TIMEOUT`] between two reads. That
/// is what `docs/standards/rust.md` asks for when it says every network path carries a timeout —
/// the failure being guarded against is a socket that stops answering, not a file that is big.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the transfer may go without producing a byte.
const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// How many times a transfer that ends early is resumed before the failure is the answer.
///
/// Three because the case this exists for is a connection dropped mid-file, which is transient and
/// usually over by the second try; a server that truncates every response is not something more
/// attempts will fix. Each attempt resumes from what is already on disk, so the cost of one more is
/// bounded by what is left rather than by the size of the artifact.
const ATTEMPTS: u32 = 3;

/// How many times the final rename is attempted.
///
/// Windows refuses to rename a directory while anything holds a handle inside it, and the smoke
/// test has just run a binary from in there: the process has exited, but an antivirus scanner or the
/// search indexer may still be reading what it left. A short retry turns a race that resolves itself
/// into a pause nobody notices.
///
/// **Seven attempts over six seconds, and the number is a measurement rather than a guess.** Four
/// attempts inside 450ms was what this was, and it lost: the Windows system suite installed the same
/// Caddy four times in one job and the fourth failed with `Access is denied (os error 5)` — run
/// 33990492733. A scanner reading a freshly written 50MB executable it has just watched run does not
/// reliably finish inside half a second on a machine running somebody else's build as well. A window
/// too short to cover the thing it exists for is not a retry, it is a coin toss, and this one was
/// tossed once per install.
const RENAME_ATTEMPTS: u32 = 7;

/// How long to wait before the second attempt; doubled before each attempt after it.
///
/// Doubling rather than a fixed pause, so that covering a slow scanner costs a fast machine nothing:
/// the first attempt is still made immediately and still succeeds immediately in the ordinary case,
/// and only a home that is actually contended pays for the long tail — 100ms through 3.2s, six
/// seconds in total.
const RENAME_PAUSE: Duration = Duration::from_millis(100);

/// The percentage the download is finished at.
///
/// The four constants below divide the bar between the four things that take time, weighted by how
/// long each actually takes rather than evenly: the download dominates, and a bar that spent a fifth
/// of itself on a checksum would look stuck for the part that matters.
const DOWNLOADED_AT: u8 = 70;

/// The percentage the checksum is reported at.
const VERIFIED_AT: u8 = 78;

/// The percentage unpacking is reported at.
const UNPACKED_AT: u8 = 92;

/// The percentage the post-install check is reported at.
const CHECKED_AT: u8 = 97;

/// What the staging directory gets filled with, once the URL has been read.
#[derive(Debug, Clone)]
enum Unpacking {
    /// Decompressed by [`archive::extract`].
    Archive(archive::Format),

    /// Copied in whole, under this name.
    OneFile(String),
}

/// The file name a one-file artifact takes, or [`None`] when its URL does not end in one.
///
/// **A name that came out of a document can never be a path** — which is [`Installer::part_file`]'s
/// own rule, and the reason that function names a `.part` file after a hash. So nothing here may
/// contain a separator, and `.` and `..` are refused by name rather than left for the file system to
/// interpret.
fn one_file_name(url: &str) -> Option<&str> {
    // A query string or fragment is not part of the file name, the same reading
    // `archive::Format::of` takes of the same URL.
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let last = path.rsplit('/').next()?;

    let refused =
        last.is_empty() || last == "." || last == ".." || last.contains('/') || last.contains('\\');

    (!refused).then_some(last)
}

/// Downloads what the index names and installs it, or leaves nothing behind.
#[derive(Debug)]
pub struct Installer {
    http: reqwest::Client,
    downloads: PathBuf,
}

impl Installer {
    /// An installer whose partial downloads live under `cache_dir`.
    ///
    /// The same directory the index cache uses, and for the same reason
    /// ([`Paths::cache`](crate::Paths::cache)): a partial download whose whole value is surviving a
    /// restart does not belong in `run/`, which is scratch belonging to the daemon currently
    /// running.
    ///
    /// # Errors
    ///
    /// [`InstallError::IndexTransport`] if the HTTP client cannot be constructed, which is a broken
    /// build rather than anything a user did — the variant is shared with the index client because
    /// both describe the same one thing that can go wrong before a request exists.
    pub fn new(cache_dir: &Path) -> Result<Self> {
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(READ_TIMEOUT)
            .user_agent(concat!("mixengine/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|source| {
                Error::Install(InstallError::IndexTransport {
                    // Not a document at all: the HTTP client itself would not build, which is a
                    // broken build rather than a fetch of anything.
                    document: "downloader",
                    url: String::new(),
                    source: Box::new(source),
                })
            })?;

        Ok(Self {
            http,
            downloads: cache_dir.join(DOWNLOADS_DIR_NAME),
        })
    }

    /// The HTTP client this installer fetches with.
    ///
    /// `pub(crate)` for the one caller that fetches something which is not an [`Artifact`] —
    /// [`crate::updates::helper`], which pulls a raw binary and its detached signature — so that
    /// this product has one client, one user agent and one set of timeouts rather than two. Not
    /// `pub`: `mixengine-daemon` does not depend on `reqwest` and must not start, which is what
    /// keeps every fetch in this workspace inside this crate.
    pub(crate) fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// Where downloads live — `pub(crate)` for [`crate::prerequisites`], the one fetch that is not an
    /// [`Artifact`] and still belongs beside the others.
    pub(crate) fn downloads(&self) -> &Path {
        &self.downloads
    }

    /// Install `artifact` at `into`, reporting to `watcher` and stopping if it says so.
    ///
    /// `into` must not exist: an install never mutates a version that is already there, which is
    /// what makes a runtime directory immutable in the sense
    /// [runtime-versions.md](../../../docs/features/runtime-versions.md) means it.
    ///
    /// `not_an_archive` says what a URL that names no archive means here — [`NotAnArchive`].
    ///
    /// # Errors
    ///
    /// [`RuntimeError::AlreadyInstalled`] when something is already at `into`;
    /// [`InstallError::ArtifactFormat`] for an archive shape this build cannot unpack;
    /// [`InstallError::ArtifactTransport`], [`InstallError::ArtifactIncomplete`] and
    /// [`InstallError::ArtifactTooLarge`] from the download; [`InstallError::ArtifactChecksum`]
    /// when what arrived is not what the index promised; [`InstallError::ArchiveUnreadable`] and
    /// [`InstallError::UnsafeArchiveEntry`] from unpacking; [`InstallError::MissingFromArtifact`]
    /// when the archive does not contain what it said it did; [`InstallError::SmokeTestFailed`]
    /// when it will not run here; [`InstallError::InstallCancelled`] when asked to stop; and
    /// [`Error::Io`] when the staging directory or the rename cannot be done.
    ///
    /// Every one of them leaves `into` untouched.
    pub async fn install<W: Watcher>(
        &self,
        artifact: &Artifact,
        into: &Path,
        smoke: Option<&SmokeTest>,
        not_an_archive: NotAnArchive,
        watcher: &W,
    ) -> Result<Installed> {
        self.install_marked(artifact, into, smoke, not_an_archive, None, watcher)
            .await
    }

    /// [`install`](Self::install), leaving `marker` in the directory.
    ///
    /// `marker`, when given, is written to [`crate::adopt::marker::FILE_NAME`] in the staging
    /// directory after the smoke test and before the rename, so a directory that exists always has
    /// the marker it was installed with. The bytes are the caller's: this pipeline does not know
    /// what a marker says, only where it goes.
    ///
    /// # Errors
    ///
    /// As [`install`](Self::install), and [`Error::Io`] when the marker cannot be written — which
    /// leaves `into` untouched like every other failure.
    pub async fn install_marked<W: Watcher>(
        &self,
        artifact: &Artifact,
        into: &Path,
        smoke: Option<&SmokeTest>,
        not_an_archive: NotAnArchive,
        marker: Option<&[u8]>,
        watcher: &W,
    ) -> Result<Installed> {
        if into.exists() {
            return Err(Error::Runtime(RuntimeError::AlreadyInstalled {
                path: into.to_path_buf(),
            }));
        }

        // Read before a byte is fetched. An artifact this build cannot unpack is a refusal that
        // costs nothing now and eighty megabytes later.
        let refuse = || {
            Error::Install(InstallError::ArtifactFormat {
                url: artifact.url.clone(),
            })
        };
        let unpacking = match (archive::Format::of(&artifact.url), not_an_archive) {
            (Some(format), _) => Unpacking::Archive(format),
            (None, NotAnArchive::OneFile) => {
                Unpacking::OneFile(one_file_name(&artifact.url).ok_or_else(refuse)?.to_owned())
            }
            (None, NotAnArchive::Refuse) => return Err(refuse()),
        };

        let part = self.part_file(artifact);
        self.fetch(artifact, &part, watcher).await?;
        self.verify(artifact, &part, watcher).await?;

        let staging = staging_for(into)?;
        // A staging directory that is already there was left by a daemon that stopped part way
        // through this, and its contents are the wrong half of an archive nobody wants.
        discard(&staging).await;
        paths::create_dir(&staging)?;

        let staged = async {
            self.unpack(&part, &unpacking, &staging, watcher).await?;
            present(artifact, &staging)?;
            self.smoke(artifact, &staging, smoke, watcher).await?;
            if let Some(marker) = marker {
                let path = staging.join(crate::adopt::marker::FILE_NAME);
                tokio::fs::write(&path, marker)
                    .await
                    .map_err(|source| Error::Io {
                        action: "write",
                        path,
                        source,
                    })?;
            }
            promote(&staging, into).await
        }
        .await;

        if let Err(refusal) = staged {
            discard(&staging).await;
            return Err(refusal);
        }

        // Only now: until the rename above, this file is the only copy of work worth resuming.
        remove_file(&part).await;
        watcher.report(100, "installed").await;

        tracing::info!(path = %into.display(), bytes = artifact.size, "a runtime was installed");

        Ok(Installed {
            path: into.to_path_buf(),
            bytes: artifact.size,
        })
    }

    /// Where this artifact's partial download lives.
    ///
    /// Named after the hash rather than after the URL, so the same artifact offered by a mirror and
    /// by the default host resumes one download rather than starting two — and so a name that came
    /// out of a document can never be a path, which a file name built from a URL would have to be
    /// escaped to avoid.
    fn part_file(&self, artifact: &Artifact) -> PathBuf {
        self.downloads
            .join(format!("{}{PART_SUFFIX}", artifact.sha256))
    }

    /// Get the whole artifact onto disk, resuming what is already there.
    ///
    /// The last attempt is made outside the loop so that its failure is the one the caller is told
    /// about: the earlier ones are forgiven noise, and a message about attempt one would send
    /// somebody looking for a network fault that had already recovered.
    async fn fetch<W: Watcher>(&self, artifact: &Artifact, part: &Path, watcher: &W) -> Result<()> {
        paths::create_dir(&self.downloads)?;

        for attempt in 1..ATTEMPTS {
            match self.attempt(artifact, part, watcher).await {
                Ok(()) => return Ok(()),
                Err(refusal @ Error::Install(InstallError::ArtifactIncomplete { .. })) => {
                    tracing::warn!(
                        url = %artifact.url,
                        attempt,
                        error = %refusal,
                        "the download ended early; resuming it"
                    )
                }
                Err(other) => return Err(other),
            }
        }

        self.attempt(artifact, part, watcher).await
    }

    /// One transfer, starting where the last one stopped.
    async fn attempt<W: Watcher>(
        &self,
        artifact: &Artifact,
        part: &Path,
        watcher: &W,
    ) -> Result<()> {
        if watcher.is_cancelled() {
            return Err(Error::Install(InstallError::InstallCancelled));
        }

        let mut have = match tokio::fs::metadata(part).await {
            Ok(metadata) => metadata.len(),
            // Missing, unreadable, a directory: all of them mean "there is nothing to resume".
            Err(_) => 0,
        };

        if have > artifact.size {
            // Longer than the index says the whole artifact is, so it is not a prefix of it. A
            // truncating write would leave the first bytes of whatever this is in place.
            remove_file(part).await;
            have = 0;
        }
        if have == artifact.size && have > 0 {
            // Complete. Whether it is the *right* file is the checksum's question, not this one's.
            return Ok(());
        }

        let transport = |source: reqwest::Error| {
            Error::Install(InstallError::ArtifactTransport {
                url: artifact.url.clone(),
                source: Box::new(source),
            })
        };

        let mut request = self.http.get(&artifact.url);
        if have > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={have}-"));
        }

        let response = request.send().await.map_err(transport)?;
        let status = response.status();

        // The server says the range we asked for is past the end of the file — so what is on disk
        // is longer than what it is serving, and cannot be a prefix of it. Start over rather than
        // fail forever: without this, one bad `.part` would refuse every future install of this
        // version, and `error_for_status` below would report it as an HTTP error nobody can act on.
        if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE && have > 0 {
            remove_file(part).await;
            return Err(Error::Install(InstallError::ArtifactIncomplete {
                url: artifact.url.clone(),
                expected: artifact.size,
                received: have,
            }));
        }

        let mut response = response.error_for_status().map_err(transport)?;

        // **A `200` to a ranged request is a server that ignored the range**, which a CDN edge or a
        // proxy that strips headers really does. The body is then the whole file, so appending it
        // to what is on disk would build something that is neither — and the checksum would catch
        // it, after the whole download.
        let resuming = have > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT;

        let mut options = tokio::fs::OpenOptions::new();
        options.create(true).write(true);
        if resuming {
            options.append(true);
        } else {
            options.truncate(true);
        }
        let mut file = options.open(part).await.map_err(|source| Error::Io {
            action: "write",
            path: part.to_path_buf(),
            source,
        })?;

        let mut written = if resuming { have } else { 0 };
        let mut reported = downloaded(written, artifact.size);
        watcher
            .report(reported, &progress(written, artifact.size))
            .await;

        while let Some(chunk) = response.chunk().await.map_err(transport)? {
            if watcher.is_cancelled() {
                // The part file stays. Somebody who cancels an install at sixty percent and asks
                // again has not asked for those bytes to be thrown away.
                return Err(Error::Install(InstallError::InstallCancelled));
            }

            written += chunk.len() as u64;
            if written > artifact.size {
                // A body longer than the index declared, which is either a mirror serving something
                // else or a stream that never ends. Bounded here rather than by the checksum, so a
                // disk is not filled to find out.
                remove_file(part).await;
                return Err(Error::Install(InstallError::ArtifactTooLarge {
                    url: artifact.url.clone(),
                    expected: artifact.size,
                }));
            }

            file.write_all(&chunk).await.map_err(|source| Error::Io {
                action: "write",
                path: part.to_path_buf(),
                source,
            })?;

            let now = downloaded(written, artifact.size);
            if now != reported {
                reported = now;
                watcher.report(now, &progress(written, artifact.size)).await;
            }
        }

        // `sync_all` and not just `flush`: the entire point of this file is that it survives the
        // process that wrote it, and bytes the OS has only promised to write do not.
        file.flush().await.map_err(|source| Error::Io {
            action: "write",
            path: part.to_path_buf(),
            source,
        })?;
        file.sync_all().await.map_err(|source| Error::Io {
            action: "write",
            path: part.to_path_buf(),
            source,
        })?;

        if written != artifact.size {
            return Err(Error::Install(InstallError::ArtifactIncomplete {
                url: artifact.url.clone(),
                expected: artifact.size,
                received: written,
            }));
        }

        Ok(())
    }

    /// Check what arrived against the hash the signed index promised.
    ///
    /// A mismatch **deletes the download**, which
    /// [security-model.md](../../../docs/architecture/security-model.md) requires and which is
    /// also the only way out of a loop: a `.part` that cannot verify would otherwise be resumed
    /// forever, arriving at the same wrong answer each time.
    async fn verify<W: Watcher>(
        &self,
        artifact: &Artifact,
        part: &Path,
        watcher: &W,
    ) -> Result<()> {
        watcher.report(VERIFIED_AT, "verifying the download").await;

        let path = part.to_path_buf();
        let found = blocking(part, move || sha256(&path)).await?;

        // Hex from a hand-written pipeline, compared against hex we formatted: the index is written
        // lowercase today and a mirror rewriting the case would be a refusal nobody could diagnose.
        if !found.eq_ignore_ascii_case(&artifact.sha256) {
            remove_file(part).await;
            return Err(Error::Install(InstallError::ArtifactChecksum {
                url: artifact.url.clone(),
                expected: artifact.sha256.clone(),
                found,
            }));
        }

        Ok(())
    }

    /// Fill the staging directory from the downloaded file, off the runtime.
    ///
    /// **Both arms go through [`blocking`]**, because copying an artifact is as much disk work as
    /// decompressing one and `docs/standards/rust.md`'s rule is about the runtime rather than
    /// about compression.
    async fn unpack<W: Watcher>(
        &self,
        part: &Path,
        unpacking: &Unpacking,
        staging: &Path,
        watcher: &W,
    ) -> Result<()> {
        if watcher.is_cancelled() {
            return Err(Error::Install(InstallError::InstallCancelled));
        }
        watcher.report(UNPACKED_AT, "unpacking").await;

        let (archive, into) = (part.to_path_buf(), staging.to_path_buf());

        match unpacking {
            Unpacking::Archive(format) => {
                let format = *format;
                blocking(part, move || archive::extract(&archive, format, &into)).await
            }
            Unpacking::OneFile(name) => {
                let target = into.join(name);
                blocking(part, move || {
                    std::fs::copy(&archive, &target)
                        .map(|_| ())
                        .map_err(|source| Error::Io {
                            action: "write",
                            path: target,
                            source,
                        })
                })
                .await
            }
        }
    }

    /// Run the artifact once, from the staging directory, before anything is renamed into place.
    async fn smoke<W: Watcher>(
        &self,
        artifact: &Artifact,
        staging: &Path,
        smoke: Option<&SmokeTest>,
        watcher: &W,
    ) -> Result<()> {
        let Some(smoke) = smoke else {
            return Ok(());
        };
        if watcher.is_cancelled() {
            return Err(Error::Install(InstallError::InstallCancelled));
        }

        watcher.report(CHECKED_AT, "checking it runs here").await;

        check_runs(artifact, staging, smoke).await
    }
}

/// Where an install is assembled before it is allowed to exist.
///
/// Beside the destination rather than in a temporary directory, and that is the load-bearing part:
/// a rename is atomic only within one filesystem, and `MIXENGINE_HOME` can be on a disk the system
/// temporary directory is not. Assembling somewhere else would turn the commit into a copy, which
/// can be interrupted half way — the one thing this whole module is arranged to prevent.
fn staging_for(into: &Path) -> Result<PathBuf> {
    let misuse = |what: &'static str| Error::Io {
        action: "install",
        path: into.to_path_buf(),
        source: std::io::Error::other(what),
    };

    let parent = into.parent().ok_or_else(|| misuse("has no parent"))?;
    let name = into
        .file_name()
        .ok_or_else(|| misuse("does not name a directory"))?;

    // Built as an `OsString` rather than through `with_extension`, which would read `8.3.33` as a
    // stem of `8.3` and an extension of `33` and produce a name for a different version.
    let mut staging = OsString::from(".");
    staging.push(name);
    staging.push(".staging");

    Ok(parent.join(staging))
}

/// Move the staged tree into place, which is the moment the install exists.
///
/// See [`RENAME_ATTEMPTS`] for why this is retried rather than attempted once.
async fn promote(staging: &Path, into: &Path) -> Result<()> {
    let mut pause = RENAME_PAUSE;

    for _ in 1..RENAME_ATTEMPTS {
        if tokio::fs::rename(staging, into).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(pause).await;
        pause *= 2;
    }

    tokio::fs::rename(staging, into)
        .await
        .map_err(|source| Error::Io {
            action: "install",
            path: into.to_path_buf(),
            source,
        })
}

/// Remove a staging directory, and say so rather than fail if it cannot be.
///
/// **Deliberately infallible.** Every caller is already on its way out with something else to
/// report, and replacing that with "and the cleanup failed too" would bury the reason the install
/// stopped. What is left behind is removed by the next attempt, which is why [`Installer::install`]
/// clears the directory before it uses it.
async fn discard(staging: &Path) {
    let path = staging.to_path_buf();
    let removed = tokio::task::spawn_blocking(move || match std::fs::remove_dir_all(&path) {
        Err(source) if source.kind() != std::io::ErrorKind::NotFound => Err((path, source)),
        _ => Ok(()),
    })
    .await;

    match removed {
        Ok(Err((path, source))) => tracing::warn!(
            path = %path.display(),
            %source,
            "a staging directory could not be removed; the next install will clear it"
        ),
        Err(source) => tracing::warn!(%source, "the staging directory could not be removed"),
        Ok(Ok(())) => {}
    }
}

/// Remove a file that is no longer worth keeping, if it is there at all.
async fn remove_file(path: &Path) {
    if let Err(source) = tokio::fs::remove_file(path).await
        && source.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(path = %path.display(), %source, "a partial download could not be removed");
    }
}

/// Where the download has got to, on the shared bar.
fn downloaded(written: u64, size: u64) -> u8 {
    if size == 0 {
        return DOWNLOADED_AT;
    }
    let scaled = written.saturating_mul(u64::from(DOWNLOADED_AT)) / size;
    u8::try_from(scaled)
        .unwrap_or(DOWNLOADED_AT)
        .min(DOWNLOADED_AT)
}

/// The same thing in words, since the percentage alone does not say how much is left.
fn progress(written: u64, size: u64) -> String {
    const MIB: u64 = 1 << 20;
    format!("downloading, {} of {} MiB", written / MIB, size / MIB)
}

#[cfg(test)]
mod tests;
