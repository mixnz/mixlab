//! The Tools module's only command, and it **only reads**.
//!
//! There is no `kill` here and there never will be: the tool prints the kill command for the user
//! to copy and run themselves, and that is the safety boundary of the whole module, not laziness.

use crate::error::AppError;
use crate::platform::{hide_console, in_background};
use std::process::Command;

use super::ports::{self, ListeningPort};

/// Which ports on this machine are being listened on.
///
/// Runs on a blocking thread: `netstat` on a machine with many connections takes a few hundred
/// milliseconds, and `lsof` takes longer still when there are network drives.
#[tauri::command]
pub async fn tools_listening_ports() -> Result<Vec<ListeningPort>, AppError> {
    in_background(collect).await
}

/// Runs an operating system program and takes its stdout.
///
/// `hide_console` is required, not cosmetic: `netstat`, `tasklist` and `lsof` are all console
/// programs, so on Windows every opening of the tool or press of Refresh would flash a black window
/// up and away — exactly what users read as software running behind their back. See
/// `crate::platform::hide_console`.
fn output_of(program: &str, args: &[&str]) -> Option<String> {
    let mut command = Command::new(program);
    command.args(args);
    let out = hide_console(&mut command).output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(target_os = "windows")]
fn collect() -> Result<Vec<ListeningPort>, AppError> {
    let netstat = output_of("netstat", &["-ano"])
        .ok_or_else(|| err!("error.portScanFailed", tool = "netstat"))?;
    // If process names cannot be looked up the table is still useful: the port and PID already
    // answer the main question.
    let tasklist = output_of("tasklist", &["/FO", "CSV", "/NH"]).unwrap_or_default();
    Ok(ports::parse_netstat(&netstat, &tasklist))
}

#[cfg(target_os = "linux")]
fn collect() -> Result<Vec<ListeningPort>, AppError> {
    if let Some(text) = output_of("ss", &["-lntp"]) {
        return Ok(ports::parse_ss(&text));
    }
    // A machine without `ss` falls back to `lsof` — the same parser as macOS.
    let text = output_of("lsof", &["-nP", "-iTCP", "-sTCP:LISTEN", "-Fpcn"])
        .ok_or_else(|| err!("error.portScanFailed", tool = "ss"))?;
    Ok(ports::parse_lsof(&text))
}

#[cfg(target_os = "macos")]
fn collect() -> Result<Vec<ListeningPort>, AppError> {
    let text = output_of("lsof", &["-nP", "-iTCP", "-sTCP:LISTEN", "-Fpcn"])
        .ok_or_else(|| err!("error.portScanFailed", tool = "lsof"))?;
    Ok(ports::parse_lsof(&text))
}

#[cfg(test)]
mod tests {
    /// Runs the real command of the machine running the test and reads the result.
    ///
    /// `#[ignore]` because it depends on the machine: a CI container has no ports being listened
    /// on, and on Linux it also needs `ss` or `lsof` to be present. Run by hand with
    /// `cargo test -- --ignored` on a desktop machine — this is the only thing proving the I/O half
    /// connects correctly to the parser half, which fixtures cannot say.
    #[test]
    #[ignore]
    fn quet_duoc_cong_that_cua_may_nay() {
        let ports = super::collect().expect("chạy được lệnh của hệ điều hành");

        // A desktop machine always has at least one port being listened on.
        assert!(!ports.is_empty(), "không thấy cổng nào đang nghe");
        // Port 0 means the parser read the wrong column.
        assert!(ports.iter().all(|p| p.port != 0), "có cổng đọc ra 0");
        // At least one row must resolve a process name; none at all is a sign the name table is
        // broken.
        assert!(
            ports.iter().any(|p| p.process.is_some()),
            "không dòng nào tra được tên tiến trình"
        );
    }
}
