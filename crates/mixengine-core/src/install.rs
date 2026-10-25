//! Getting an artifact the index names onto this machine — completely, or not at all.
//!
//! # The whole of it is one transaction, and the rename is the commit
//!
//! `docs/features/runtime-versions.md` states the invariant this module exists to hold: *a
//! half-extracted version must never appear in `list`*. So nothing is written where a reader will
//! look for it. The archive is unpacked into a staging directory beside the destination, everything
//! that could still refuse it happens there — the paths it promised are checked, the binary is made
//! to actually start — and only then is the directory renamed into place, which the operating system
//! does atomically or not at all. Any failure removes the staging directory and leaves the
//! destination as if nothing had been attempted.
//!
//! That ordering is why the post-install check lives here rather than in whatever calls this. Run
//! after the rename it would be a test of something already installed, and its failure would have
//! nothing to undo without deleting a directory a client may already have been told about.
//!
//! # A download survives the process that started it, and an install does not
//!
//! The two halves are deliberately unlike. The `.part` file is named after the hash the index
//! promises, lives in `cache/downloads/` and is *kept* across a failure, a cancellation and a daemon
//! restart, so asking again resumes rather than starts over — which for an eighty-megabyte artifact
//! on a bad connection is the difference between a feature and a wish. The staging directory is the
//! opposite: it belongs to one attempt and is removed by anything that goes wrong, including the
//! next attempt finding one left behind by a daemon that was killed.
//!
//! # What is not here
//!
//! **No job, and no method.** This is the mechanism; `runtime.install` is the daemon's, on the same
//! split the service methods and the job system use, and [`Watcher`] is shaped exactly like the
//! daemon's `JobHandle` so that wiring the two together is an impl and not an adapter. **No
//! per-runtime knowledge** either: what to run as a smoke test arrives as a [`SmokeTest`], because
//! "which flag prints the version" is a fact about PHP and Node, not about downloading.

mod error;

pub use error::InstallError;

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::index::Artifact;
use crate::{Error, Result};

pub(crate) mod archive;
#[cfg(feature = "network")]
mod installer;

#[cfg(feature = "network")]
pub use installer::Installer;

/// How long the post-install check may take before it counts as a binary that does not start.
///
/// Generous for `php -v`, and it is not really the runtime that is being waited for: a first launch
/// under Defender or Smart App Control is scanned before it is allowed to run, and a machine that
/// has never seen this file pays for that once.
const SMOKE_TIMEOUT: Duration = Duration::from_secs(60);

/// What a URL that is not one of the three archive formats means to whoever asked for it.
///
/// **The package index publishes archives and nothing else** — `mkindex.py`'s `ARCHIVE_SUFFIXES` —
/// so a fourth suffix there names an artifact this build has no decompressor for, and refusing it
/// with a reason is the honest answer. An extension is the other case: Adminer's distribution is one
/// PHP file, which is what a great many small tools ship. The caller says which of the two it is, so
/// neither has to guess from the shape of a URL what the document it came out of meant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotAnArchive {
    /// Refuse it, with [`InstallError::ArtifactFormat`]. What the package index asks for.
    Refuse,

    /// Install it as one file, named by the last segment of its URL. What an extension asks for.
    OneFile,
}

/// What an install reports to, and asks whether it should stop.
///
/// **Shaped after the daemon's `JobHandle` rather than invented here**, which is what keeps this
/// crate from knowing about jobs at all: reporting is one `async fn`, cancellation is one question,
/// and everything else — the row, the event, the state machine — stays where it belongs. A producer
/// that could write its own ending would be a producer that could disagree with the row.
///
/// Cancellation is cooperative for the reason `mixengine_core::jobs` records: a task dropped
/// mid-download leaves a staging directory behind, and nothing dropped mid-`await` removes it. So
/// this is asked between chunks and between steps, and the work returns when it sees.
pub trait Watcher: Sync {
    /// Say how far along this is, and what it is doing.
    ///
    /// Called on a change worth reporting rather than on every read: the daemon publishes these on
    /// the same bounded stream every service transition uses.
    fn report(&self, percent: u8, message: &str) -> impl Future<Output = ()> + Send;

    /// Whether somebody has asked this to stop.
    fn is_cancelled(&self) -> bool;
}

/// The thing to run once the archive is unpacked, to find out whether it runs *here*.
///
/// **This is what the SHA-256 cannot tell you.** A checksum proves the bytes are the ones we
/// published; it says nothing about whether this machine can execute them. Every failure the
/// `requires` field of an [`Artifact`] describes — a missing VC++ redistributable, a glibc older
/// than the build's floor, an architecture running under an emulator that will not load it — is
/// invisible until something tries, and finding out at install time produces a message naming the
/// cause, while finding out later produces a loader error in the middle of somebody's work.
///
/// The executable is named as a key of [`Artifact::provides`] rather than as a path, because the
/// path inside the archive is the publisher's and the name is ours.
#[derive(Debug, Clone)]
pub struct SmokeTest {
    /// Which of the artifact's executables to run, by the name it is published under (`php`).
    pub executable: String,

    /// What to pass it. Something that exits zero quickly and touches the runtime's own machinery —
    /// `-v` for PHP, `--version` for Node.
    pub args: Vec<String>,

    /// Variables removed from the child's environment — a JVM's own option variables, for Java.
    /// Empty for every other kind.
    pub unset: &'static [&'static str],
}

/// A runtime that is now on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// Where it landed.
    pub path: PathBuf,

    /// How large the archive was, as the index declared it and the download proved it.
    pub bytes: u64,
}

/// Run `smoke` against the build in `dir` — the check an install makes before the rename, and the
/// one [`crate::adopt`] makes of a directory an earlier home left without a marker.
///
/// # Errors
///
/// [`InstallError::MissingFromArtifact`] when `provides` does not name the executable, and
/// [`InstallError::SmokeTestFailed`] when it will not start, times out or exits non-zero.
pub async fn check_runs(artifact: &Artifact, dir: &Path, smoke: &SmokeTest) -> Result<()> {
    let relative = artifact.provides.get(&smoke.executable).ok_or_else(|| {
        Error::Install(InstallError::MissingFromArtifact {
            url: artifact.url.clone(),
            executable: smoke.executable.clone(),
            path: String::new(),
        })
    })?;
    let program = crate::paths::join_stored(dir, relative);

    let failed = |detail: String| {
        Error::Install(InstallError::SmokeTestFailed {
            program: program.clone(),
            detail,
        })
    };

    // `current_dir` is the dir directory because that is where the runtime's own files are:
    // a Windows PHP resolves its DLLs from beside the executable, which is the whole reason
    // `provides` carries a path rather than only a name.
    let mut checking = tokio::process::Command::new(&program);
    checking
        .args(&smoke.args)
        .current_dir(dir)
        // A check that hung would otherwise outlive the timeout below and hold the dir
        // directory open, which is exactly what the rename cannot tolerate on Windows.
        .kill_on_drop(true);

    for variable in smoke.unset {
        checking.env_remove(variable);
    }

    // A runtime is a console program and the daemon has no console, so without this Windows
    // makes one for it — a terminal window flashing on the desktop for every `php -v`.
    mixengine_platform::process::without_a_window(checking.as_std_mut());

    let running = checking.output();

    let output = match tokio::time::timeout(SMOKE_TIMEOUT, running).await {
        Ok(Ok(output)) => output,
        // Where a missing VC++ redistributable, a glibc floor, an unloadable architecture and a
        // machine whose application control policy refuses the image all arrive: the OS refuses
        // to start it and says why. The last of those needs a sentence of its own, which
        // `why_it_would_not_start` writes.
        Ok(Err(source)) => return Err(failed(why_it_would_not_start(&source))),
        Err(_) => {
            return Err(failed(format!(
                "it did not answer within {} seconds",
                SMOKE_TIMEOUT.as_secs()
            )));
        }
    };

    if !output.status.success() {
        let complaint = String::from_utf8_lossy(&output.stderr);
        let first = complaint.lines().find(|line| !line.trim().is_empty());
        return Err(failed(match first {
            Some(line) => format!("it exited with {}: {line}", output.status),
            None => format!("it exited with {}", output.status),
        }));
    }

    Ok(())
}

/// What a smoke test says when the operating system would not start the program at all.
///
/// **A function rather than a `to_string()` at the call site**, so the one interesting case can be
/// exercised without a download. Every other failure here is transient or is the packaging's fault;
/// an application control policy refusing the image is neither, and the OS message for it is a
/// sentence about *this file* that says nothing about why every MixEngine binary meets the same
/// wall.
fn why_it_would_not_start(source: &std::io::Error) -> String {
    if mixengine_platform::refused_by_app_control(source) {
        format!("{source}; {}", mixengine_platform::APP_CONTROL_REFUSAL)
    } else {
        source.to_string()
    }
}

/// Everything the artifact said it provides is in the tree.
///
/// Checked before the smoke test rather than trusted, because the failure it catches is a *packaging*
/// bug — an archive repacked without a binary the index still lists — and the message it produces
/// names the file, while the same bug found later is a runtime that is missing something at the
/// moment somebody needs it.
///
/// **A directory is a legitimate answer, and it has to be.** macOS wraps a windowed application in
/// one, `packaging/feed.sh` names that directory in the macOS payload's `provides`, and a check
/// demanding a file would refuse the whole payload — taking the four ordinary binaries with it and
/// reporting the window. An *empty* directory is still refused: that is the shape a bundle copied
/// by something that did not follow it takes, and it is what the strictness here was really buying.
/// The one entry whose shape matters beyond existing is the smoke-test executable, and that one is
/// proved by being run.
///
/// # Errors
///
/// [`InstallError::MissingFromArtifact`] naming the first entry that is not there.
pub fn present(artifact: &Artifact, dir: &Path) -> Result<()> {
    provided(&artifact.provides, &artifact.url, dir)
}

/// [`present`], for a `provides` map read from somewhere other than an artifact — the marker an
/// earlier install left, which [`crate::adopt`] checks before it records a directory.
///
/// # Errors
///
/// [`InstallError::MissingFromArtifact`] naming the first entry that is not there, or that would
/// lead outside `dir`.
pub fn provided(provides: &BTreeMap<String, String>, url: &str, dir: &Path) -> Result<()> {
    for (executable, relative) in provides {
        let path = Path::new(relative);
        let missing = || {
            Error::Install(InstallError::MissingFromArtifact {
                url: url.to_owned(),
                executable: executable.clone(),
                path: relative.clone(),
            })
        };

        // The index is signed, so this cannot be a hostile path — and it is checked anyway, because
        // the alternative is that one field of one document decides which file on this machine gets
        // run as a post-install check.
        if !archive::safe(path) {
            return Err(missing());
        }

        let staged = dir.join(path);
        let carried = if staged.is_dir() {
            std::fs::read_dir(&staged).is_ok_and(|mut entries| entries.next().is_some())
        } else {
            staged.is_file()
        };
        if !carried {
            return Err(missing());
        }
    }

    Ok(())
}

/// Run blocking work on a thread that is allowed to block, blaming `path` if the thread itself dies.
///
/// A `JoinError` here means the blocking pool panicked, which nothing in this module does — it is
/// reported as an I/O failure rather than unwrapped because nothing in this crate panics.
#[cfg(feature = "network")]
async fn blocking<T, F>(path: &Path, work: F) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(work).await {
        Ok(outcome) => outcome,
        Err(source) => Err(Error::Io {
            action: "read",
            path: path.to_path_buf(),
            source: std::io::Error::other(source),
        }),
    }
}

/// The SHA-256 of a file, as lowercase hex — for a caller holding a file this pipeline downloaded
/// earlier, which it checks again before trusting: a `.pkg` reused after a person cancelled
/// Installer.app.
///
/// # Errors
///
/// [`Error::Io`] when the file cannot be read.
pub fn sha256_of(path: &Path) -> Result<String> {
    sha256(path)
}

/// The SHA-256 of a file, as lowercase hex.
fn sha256(path: &Path) -> Result<String> {
    use sha2::Digest as _;

    let io = |source| Error::Io {
        action: "read",
        path: path.to_path_buf(),
        source,
    };

    let mut file = std::fs::File::open(path).map_err(io)?;
    let mut hasher = sha2::Sha256::new();
    // A megabyte at a time: large enough that the syscall is not what this costs, small enough that
    // hashing an artifact does not depend on how much memory the machine has.
    let mut buffer = vec![0_u8; 1 << 20];

    loop {
        let read = file.read(&mut buffer).map_err(io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        // Cannot fail: writing to a `String` is infallible, and the format has no user input in it.
        let _ = write!(hex, "{byte:02x}");
    }

    Ok(hex)
}

#[cfg(test)]
mod tests;
