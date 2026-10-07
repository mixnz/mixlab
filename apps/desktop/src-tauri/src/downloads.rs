//! Fetching a file a module needs and checking it is the file it should be — taken out of the
//! `db` module's tool download (T203, D2) so the `tunnel` module's `cloudflared` uses the same
//! `curl`, the same SHA-256 check and the same progress counting, rather than a copy of them.
//!
//! `curl` and `tar` do the fetching and unpacking: both ship with Windows 10 and up and with every
//! macOS and Linux this app runs on.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use crate::error::AppError;
use crate::platform::hide_console;

/// Checks a downloaded archive against the checksum pinned for it.
///
/// A mismatch means the file is not the one this build of MixLab was made to unpack: a release
/// withdrawn or rebuilt under the same name, a download that came back truncated, or something
/// between here and the vendor handing over a different file. None of those should be unpacked
/// and then run with the credentials of every database the user connects to.
pub(crate) fn verify_sha256(path: &Path, expected: &str) -> Result<(), AppError> {
    use sha2::{Digest, Sha256};

    let mut file =
        std::fs::File::open(path).map_err(|e| err!("error.cannotReadDownload", message = e))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)
        .map_err(|e| err!("error.cannotReadDownload", message = e))?;
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected {
        return Err(err!(
            "error.checksumMismatch",
            actual = actual,
            expected = expected
        ));
    }
    Ok(())
}

/// A helper program, set up the way this module always wants one: nothing on stdin, output thrown
/// away, complaints kept back for the error message, and — on Windows — no console window flashing
/// up in the user's face.
pub(crate) fn helper(program: &str) -> Command {
    let mut command = Command::new(program);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    hide_console(&mut command);
    command
}

/// Waits for a helper to finish and turns a non-zero exit into `what`, carrying the tail of what
/// it printed — the part of a curl or tar failure that says which thing went wrong.
pub(crate) fn finish(child: Child, what: &'static str) -> Result<(), AppError> {
    let output = child
        .wait_with_output()
        .map_err(|e| AppError::new(what).with("message", e))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let tail = stderr.lines().rev().take(4).collect::<Vec<_>>().join(" ");
    Err(AppError::new(what).with("message", tail))
}

/// Runs a helper program, with its output thrown away and its complaints kept for the error.
pub(crate) fn run(program: &str, args: &[&str], what: &'static str) -> Result<(), AppError> {
    let child = helper(program)
        .args(args)
        .spawn()
        .map_err(|e| err!("error.helperMissing", program = program, message = e))?;
    finish(child, what)
}

/// How big the archive is going to be, asked of the server before fetching it.
///
/// Only to fill in a progress bar, so every way of failing — a HEAD the CDN refuses, a redirect
/// chain that drops the header, a body sent chunked — answers `0` and leaves the bar indeterminate
/// rather than stopping the install.
pub(crate) fn content_length(url: &str) -> u64 {
    let output = helper("curl")
        .args(["--head", "--location", "--silent", "--fail", url])
        .stdout(Stdio::piped())
        .output();
    let Ok(output) = output else { return 0 };
    if !output.status.success() {
        return 0;
    }
    // The last one wins: after a redirect the headers of every hop are printed in turn, and it is
    // the final response that describes the file actually being sent.
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_once(':'))
        .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .filter_map(|(_, value)| value.trim().parse::<u64>().ok())
        .next_back()
        .unwrap_or(0)
}

/// Fetches a file, reporting how far it has got as it goes: bytes so far, and how many in all.
///
/// The count comes from the size of the file being written rather than from curl's own progress
/// meter: curl draws that for a terminal, redrawing one line with carriage returns, and reading a
/// number back out of it is a great deal more fragile than asking the filesystem.
pub(crate) fn download(
    url: &str,
    archive: &Path,
    report: &dyn Fn(u64, u64),
) -> Result<(), AppError> {
    let total = content_length(url);
    report(0, total);

    let mut child = helper("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--retry",
            "2",
            "--output",
            &archive.to_string_lossy(),
            url,
        ])
        .spawn()
        .map_err(|e| err!("error.helperMissing", program = "curl", message = e))?;

    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                let done = std::fs::metadata(archive)
                    .map(|meta| meta.len())
                    .unwrap_or(0);
                report(done, total);
                std::thread::sleep(Duration::from_millis(250));
            }
            Err(e) => return Err(AppError::new("error.downloadFailed").with("message", e)),
        }
    }
    // `wait_with_output` after the process has already been reaped returns the status it kept, so
    // the exit code and stderr are still the ones curl left behind.
    finish(child, "error.downloadFailed")
}

/// Makes `path` executable for its owner on Unix; nothing anywhere else.
pub(crate) fn make_executable(path: &Path) -> Result<(), AppError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| err!("error.cannotWriteFile", path = path.display(), message = e))?;
    }
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::verify_sha256;
    use std::path::{Path, PathBuf};

    const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    fn temp_file(contents: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(format!("mixlab-test-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn a_download_is_only_accepted_at_the_checksum_it_was_pinned_at() {
        let path = temp_file(b"");
        let matching = verify_sha256(&path, EMPTY_SHA256);
        let mismatched = verify_sha256(&path, &"0".repeat(64));
        std::fs::remove_file(&path).unwrap();

        assert!(matching.is_ok());
        let message = mismatched.unwrap_err();
        // The message has to name both halves: which file arrived, and which was expected.
        assert_eq!(message.code, "error.checksumMismatch");
        assert_eq!(
            message.params.get("actual"),
            Some(&EMPTY_SHA256.to_string())
        );
    }

    #[test]
    fn a_missing_download_is_a_failure_rather_than_a_pass() {
        assert!(verify_sha256(Path::new("no-such-file"), EMPTY_SHA256).is_err());
    }
}
