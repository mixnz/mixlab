use std::path::PathBuf;

use super::models::LocalShell;

/// The shells this machine can open, in suggested order — the first one is the default.
pub fn detect() -> Vec<LocalShell> {
    let mut found = Vec::new();
    #[cfg(windows)]
    detect_windows(&mut found);
    #[cfg(not(windows))]
    detect_unix(&mut found);
    found
}

/// The machine's default shell, for a `TerminalTarget::Local { shell: None, .. }` — with its
/// arguments, because `--login` belongs to "the default shell" just as much as the path does.
fn default_shell() -> (String, Vec<String>) {
    detect()
        .into_iter()
        .next()
        .map(|shell| (shell.path, shell.args))
        .unwrap_or_else(|| {
            let path = if cfg!(windows) { "cmd.exe" } else { "/bin/sh" };
            (path.to_string(), Vec::new())
        })
}

/// Adds an entry if the file really exists and that path is not in the list yet.
fn push_if_present(found: &mut Vec<LocalShell>, name: &str, path: PathBuf, args: Vec<String>) {
    if !path.is_file() {
        return;
    }
    let path = path.display().to_string();
    if found.iter().any(|shell| shell.path == path) {
        return;
    }
    found.push(LocalShell {
        name: name.to_string(),
        path,
        args,
    });
}

/// The arguments that make the shell open as a *login* shell.
///
/// This is where `.bash_profile`, `.zprofile` and `.profile` are read — and only there. A bash
/// running on a pty is interactive but not a login shell, so the PATH, aliases and variables the
/// user sets in those files simply do not exist in the session; `ssh-add -l` finding no agent is
/// the most common symptom. Every real terminal opens a login shell: the "Git Bash" shortcut runs
/// `bash --login -i`, and so does Terminal.app on macOS.
///
/// By name rather than for all: `-l` is a flag of bash, zsh and fish, while `sh` on Linux is
/// usually dash and does not accept it. `-i` is not needed — with a real pty, the shell knows it is
/// interactive by itself.
fn login_args(name: &str) -> Vec<String> {
    match name {
        "bash" | "git-bash" | "zsh" | "fish" => vec!["-l".to_string()],
        _ => Vec::new(),
    }
}

/// Finds a program on `PATH`. Used for `pwsh` and `wsl.exe`, two things with no fixed path.
#[cfg(windows)]
fn on_path(exe: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(exe))
        .find(|candidate| candidate.is_file())
}

#[cfg(windows)]
fn detect_windows(found: &mut Vec<LocalShell>) {
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    let system32 = PathBuf::from(&system_root).join("System32");

    push_if_present(
        found,
        "powershell",
        system32
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe"),
        Vec::new(),
    );
    if let Some(pwsh) = on_path("pwsh.exe") {
        push_if_present(found, "pwsh", pwsh, Vec::new());
    }
    push_if_present(found, "cmd", system32.join("cmd.exe"), Vec::new());

    for base in ["ProgramFiles", "ProgramW6432", "LOCALAPPDATA"] {
        if let Ok(dir) = std::env::var(base) {
            let git_bash = PathBuf::from(&dir).join("Git").join("bin").join("bash.exe");
            push_if_present(found, "git-bash", git_bash, login_args("git-bash"));
        }
    }

    if let Some(wsl) = on_path("wsl.exe") {
        for distro in wsl_distros() {
            found.push(LocalShell {
                name: format!("wsl:{distro}"),
                path: wsl.display().to_string(),
                args: vec!["-d".to_string(), distro],
            });
        }
    }
}

/// The installed WSL distributions. On a machine without WSL, `wsl.exe` fails and the list is empty
/// — not an error to report to anyone.
#[cfg(windows)]
fn wsl_distros() -> Vec<String> {
    /* No console window flashing up — `crate::platform::hide_console` explains why, and it is the
    only place in the whole app that sets that flag. */
    let mut command = std::process::Command::new("wsl.exe");
    command.args(["-l", "-q"]);
    let output = match crate::platform::hide_console(&mut command).output() {
        Ok(output) if output.status.success() => output,
        _ => return Vec::new(),
    };
    parse_wsl_list(&output.stdout)
}

/// `wsl.exe -l -q` prints UTF-16LE, with a BOM, CRLF line endings, and when there are no
/// distributions it prints an English sentence instead of nothing.
///
/// Not marked `#[cfg(windows)]` so its tests run everywhere — what it reads is bytes, not the
/// operating system.
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_wsl_list(bytes: &[u8]) -> Vec<String> {
    // `as_chunks` instead of `chunks_exact(2)`: the same job, the same remainder dropped, but the
    // size is in the type, so each element is already a `[u8; 2]` — no slicing and indexing again.
    // clippy 1.98 requires this form (`chunks_exact_to_as_chunks`).
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    String::from_utf16_lossy(&units)
        .lines()
        .map(|line| {
            line.trim_matches(|c: char| c == '\u{feff}' || c.is_whitespace())
                .to_string()
        })
        .filter(|line| !line.is_empty() && !line.starts_with("Windows Subsystem for Linux"))
        .collect()
}

#[cfg(not(windows))]
fn detect_unix(found: &mut Vec<LocalShell>) {
    if let Ok(shell) = std::env::var("SHELL") {
        let path = PathBuf::from(&shell);
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("sh")
            .to_string();
        push_if_present(found, &name, path, login_args(&name));
    }
    for candidate in ["/bin/zsh", "/bin/bash", "/bin/sh"] {
        let path = PathBuf::from(candidate);
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("sh")
            .to_string();
        push_if_present(found, &name, path, login_args(&name));
    }
}

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use super::models::{Output, OutputSink, TerminalSize};
use super::state::Session;
use super::stream::{coalesce, QUEUE_DEPTH};
use crate::error::AppError;

/// The buffer for one read from the pty. Much smaller than an IPC frame — the batcher is what
/// decides how big a frame gets.
const READ_BUFFER: usize = 8 * 1024;

/// How long to wait for the last bytes to leave the pty after the child process has died, before
/// letting go of the master. The reader is draining continuously, so this is a pause, not a wait.
const EXIT_DRAIN: Duration = Duration::from_millis(100);

/// The cap on waiting for the reader to see EOF. Windows' `ClosePseudoConsole` is known to
/// sometimes not return, and the worst it may do is swallow a few final bytes — not hide the fact
/// that the session has ended.
const EXIT_TIMEOUT: Duration = Duration::from_secs(2);

fn pty_size(size: TerminalSize) -> PtySize {
    PtySize {
        rows: size.rows,
        cols: size.cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// Opens a shell on this machine and returns its handle.
///
/// Three flows run in parallel after this function returns: one thread reading the pty, one thread
/// writing to the pty, one thread waiting on the child process. There is only one way out, and the
/// order on it is the real order — see where `exit_rx` is awaited below.
/// A `cwd` that is not a directory is said, not silently replaced by the home directory — T205,
/// D11: a target outlives the project it was made for.
fn check_cwd(cwd: Option<&str>) -> Result<Option<&str>, AppError> {
    match cwd {
        Some(dir) if !Path::new(dir).is_dir() => Err(err!("error.terminalCwdMissing", path = dir)),
        other => Ok(other),
    }
}

/// `prepend` ahead of `inherited`, joined by this OS's rule — T205, D9.
fn joined_path(prepend: &[String], inherited: Option<OsString>) -> Option<OsString> {
    if prepend.is_empty() {
        return None;
    }
    let mut parts: Vec<PathBuf> = prepend.iter().map(PathBuf::from).collect();
    if let Some(inherited) = inherited {
        parts.extend(std::env::split_paths(&inherited));
    }
    std::env::join_paths(parts).ok()
}

pub fn spawn(
    shell: Option<String>,
    args: Vec<String>,
    cwd: Option<String>,
    env: &BTreeMap<String, String>,
    path_prepend: &[String],
    size: TerminalSize,
    out: OutputSink,
) -> Result<Session, AppError> {
    /* With no shell chosen, take the whole default entry, path and arguments — the `args` coming in
    here belong to a shell the call did not name, so they are empty. */
    let (program, args) = match shell {
        Some(path) => (path, args),
        None => default_shell(),
    };

    /* An absolute path that no longer exists — Git uninstalled, the WSL distribution removed — is
    worth saying plainly instead of letting the pty return an OS error nobody reads. A bare name
    like `cmd.exe` is skipped: it is looked up on `PATH`, not on disk. */
    if (program.contains('/') || program.contains('\\')) && !Path::new(&program).is_file() {
        return Err(err!("error.terminalShellNotFound", path = program));
    }

    let pair = native_pty_system()
        .openpty(pty_size(size))
        .map_err(|e| err!("error.terminalSpawnFailed", message = e))?;

    let mut command = CommandBuilder::new(&program);
    for arg in &args {
        command.arg(arg);
    }
    if let Some(dir) = check_cwd(cwd.as_deref())? {
        command.cwd(dir);
    }
    // What xterm.js can draw. Without it, a shell on Unix treats the terminal as dumb and turns off
    // colour entirely.
    command.env("TERM", "xterm-256color");
    for (key, value) in env {
        command.env(key, value);
    }
    if let Some(path) = joined_path(path_prepend, std::env::var_os("PATH")) {
        command.env("PATH", path);
    }

    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|e| err!("error.terminalSpawnFailed", message = e))?;
    // The slave end has to be let go right away, otherwise the reader never sees EOF.
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| err!("error.terminalSpawnFailed", message = e))?;
    let mut writer = pair
        .master
        .take_writer()
        .map_err(|e| err!("error.terminalSpawnFailed", message = e))?;
    /* `Option` rather than the thing itself: ending the session means *letting go of* the master,
    and letting go of something inside an `Arc` means lifting it out of there. See the `take()`
    below. */
    let master = Arc::new(StdMutex::new(Some(pair.master)));
    let killer = child.clone_killer();

    let (raw_tx, raw_rx) = mpsc::channel::<Vec<u8>>(QUEUE_DEPTH);
    let (input_tx, mut input_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (resize_tx, mut resize_rx) = mpsc::unbounded_channel::<TerminalSize>();
    let (exit_tx, exit_rx) = oneshot::channel::<Option<i32>>();
    let kill = CancellationToken::new();

    // Reads the pty. This is the only place holding `raw_tx`, so this thread ending is how the
    // batcher knows the bytes are done — and only then is `Exit` emitted.
    std::thread::spawn(move || {
        let mut buffer = vec![0u8; READ_BUFFER];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if raw_tx.blocking_send(buffer[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Writes what the user types. Ends when the `Session` is dropped, because then nobody holds
    // `input_tx` any more.
    std::thread::spawn(move || {
        while let Some(bytes) = input_rx.blocking_recv() {
            if writer.write_all(&bytes).is_err() || writer.flush().is_err() {
                break;
            }
        }
    });

    // Resizes. Once the session has ended, `master` is `None` and there is nothing left to resize.
    std::thread::spawn({
        let master = master.clone();
        move || {
            while let Some(size) = resize_rx.blocking_recv() {
                if let Some(master) = master.lock().unwrap().as_ref() {
                    let _ = master.resize(pty_size(size));
                }
            }
        }
    });

    // Waits for the child process, then hands the exit code to the way out — it does not emit it
    // itself, because the buffer may still hold bytes not yet flushed.
    std::thread::spawn(move || {
        let code = child.wait().ok().map(|status| status.exit_code() as i32);
        let _ = exit_tx.send(code);
    });

    // The tab closing, or the app exiting.
    tokio::spawn({
        let kill = kill.clone();
        async move {
            kill.cancelled().await;
            let mut killer = killer;
            let _ = killer.kill();
        }
    });

    /* One way out, one order: bytes done → buffer done → only then `Exit`.

    The reader does not see EOF by itself when the shell dies. On Windows, the output pipe belongs
    to ConPTY, and ConPTY lives as long as the master does — and the session holds the master so
    it can still resize. So a child process dying with nobody letting go of the master leaves the
    reader silent forever, `coalesce` never returns, and `Exit` is never emitted: the user types
    `exit` and then stares at a frozen screen with nobody telling them. (On Unix, reading the
    master after the child dies returns EIO, so this does not show, and letting go of the master
    there is harmless too: the reader holds a `dup` of its own.)

    Hence: wait for the child to die → pause one beat for the last bytes to leave the pipe → let go
    of the master → only now are the bytes done, the buffer done, and then `Exit`. */
    tokio::spawn({
        let out = out.clone();
        let data = out.clone();
        async move {
            let mut drain = tokio::spawn(coalesce(raw_rx, move |chunk| data(Output::Data(chunk))));
            let code = exit_rx.await.ok().flatten();
            tokio::time::sleep(EXIT_DRAIN).await;
            // On a blocking thread: closing ConPTY is an OS call that may hang around for a while.
            let _ = tokio::task::spawn_blocking(move || {
                master.lock().unwrap().take();
            })
            .await;
            if tokio::time::timeout(EXIT_TIMEOUT, &mut drain)
                .await
                .is_err()
            {
                drain.abort();
            }
            out(Output::Exit {
                code,
                message: None,
            });
        }
    });

    Ok(Session {
        input: input_tx,
        resize: resize_tx,
        kill,
    })
}

#[cfg(test)]
mod tests {
    use super::check_cwd;
    use super::joined_path;
    use super::login_args;
    use super::parse_wsl_list;
    use super::spawn;
    use crate::modules::terminal::models::{Output, OutputSink, TerminalSize};
    use std::sync::{Arc, Mutex};

    /// The most expensive thing a non-login session loses is `.bash_profile`, and with it the
    /// `GIT_SSH`, PATH and aliases the user sets there.
    #[test]
    fn opens_bash_as_a_login_shell() {
        assert_eq!(login_args("bash"), vec!["-l".to_string()]);
        assert_eq!(login_args("git-bash"), vec!["-l".to_string()]);
    }

    /// `.zprofile` is where macOS sets PATH, and Terminal.app reads it because it opens a login
    /// shell.
    #[test]
    fn opens_zsh_as_a_login_shell() {
        assert_eq!(login_args("zsh"), vec!["-l".to_string()]);
    }

    /// `sh` on Linux is usually dash, and dash has no `-l` — opening it that way is a session dead
    /// from the first line. `cmd` and `powershell` have no notion of a login shell.
    #[test]
    fn leaves_alone_a_shell_that_has_no_such_flag() {
        assert!(login_args("sh").is_empty());
        assert!(login_args("cmd").is_empty());
        assert!(login_args("powershell").is_empty());
    }

    /// `wsl.exe -l -q` prints UTF-16LE with CRLF — rebuilt exactly that way for the test.
    fn utf16le(text: &str) -> Vec<u8> {
        text.encode_utf16()
            .flat_map(|unit| unit.to_le_bytes())
            .collect()
    }

    #[test]
    fn reads_one_name_per_line() {
        let bytes = utf16le("Ubuntu\r\nDebian\r\n");
        assert_eq!(
            parse_wsl_list(&bytes),
            vec!["Ubuntu".to_string(), "Debian".to_string()]
        );
    }

    /// Names with spaces are normal — `Ubuntu 22.04` must not be cut in two.
    #[test]
    fn keeps_a_name_with_a_space_in_it() {
        let bytes = utf16le("Ubuntu 22.04\r\n");
        assert_eq!(parse_wsl_list(&bytes), vec!["Ubuntu 22.04".to_string()]);
    }

    #[test]
    fn drops_the_bom_and_the_blank_lines() {
        let bytes = utf16le("\u{feff}Ubuntu\r\n\r\n");
        assert_eq!(parse_wsl_list(&bytes), vec!["Ubuntu".to_string()]);
    }

    /// On a machine with no distributions, `wsl.exe` prints an English sentence rather than an
    /// empty list. That sentence is not a distro name.
    #[test]
    fn is_not_fooled_by_the_no_distributions_message() {
        let bytes = utf16le("Windows Subsystem for Linux has no installed distributions.\r\n");
        assert!(parse_wsl_list(&bytes).is_empty());
    }

    /// The path the server side of the "session ended" table takes: the shell exits by itself,
    /// nobody kills it.
    ///
    /// Runs `exit 3` instead of typing `exit` into an interactive shell — PowerShell asks for the
    /// cursor position with `ESC[6n` and then waits for an answer, and there is no xterm here to
    /// answer. What is being tested is "does the child process dying produce `Exit`", not a
    /// particular shell's prompt.
    #[tokio::test]
    async fn a_shell_that_ends_by_itself_says_so() {
        let seen: Arc<Mutex<Vec<Output>>> = Arc::new(Mutex::new(Vec::new()));
        let handle = seen.clone();
        let sink: OutputSink = Arc::new(move |output| handle.lock().unwrap().push(output));

        let (shell, args) = if cfg!(windows) {
            (
                "cmd.exe",
                vec!["/c".to_string(), "exit".to_string(), "3".to_string()],
            )
        } else {
            ("/bin/sh", vec!["-c".to_string(), "exit 3".to_string()])
        };
        let session = spawn(
            Some(shell.to_string()),
            args,
            None,
            &std::collections::BTreeMap::new(),
            &[],
            TerminalSize { cols: 80, rows: 24 },
            sink,
        )
        .expect("shell phải mở được");

        /* ConPTY opens by asking for the cursor position (`ESC[6n`) and *waits* for the answer
        before letting the child process run — in the app xterm answers, here nobody does. Answer
        on its behalf. */
        session.input.send(b"\x1b[1;1R".to_vec()).unwrap();

        /* Bounded, and the bound is shorter than `EXIT_TIMEOUT`: that safety net still emits
        `Exit` even when the reader never sees EOF, so a test only asking "is there eventually an
        `Exit`" would stay green even if the bug came back. What is being held is that the session
        reports *right away*. */
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1500);
        loop {
            if matches!(
                seen.lock().unwrap().last(),
                Some(Output::Exit { code: Some(3), .. })
            ) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "hết hạn mà chưa có Exit(3), thấy: {:?}",
                seen.lock().unwrap(),
            );
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }

        // The handle is still here — nobody killed it, and it still has to report.
        drop(session);
    }

    /// Opens the machine's default shell and then drops the handle. The session has to die and
    /// has to report `Exit` — this is the path "close the tab" takes, so it must not be silent.
    #[tokio::test]
    async fn dropping_the_session_ends_it_and_says_so() {
        let seen: Arc<Mutex<Vec<Output>>> = Arc::new(Mutex::new(Vec::new()));
        let handle = seen.clone();
        let sink: OutputSink = Arc::new(move |output| handle.lock().unwrap().push(output));

        let session = spawn(
            None,
            Vec::new(),
            None,
            &std::collections::BTreeMap::new(),
            &[],
            TerminalSize { cols: 80, rows: 24 },
            sink,
        )
        .expect("shell mặc định phải mở được");
        drop(session);

        // Kill the process, read the pty dry, flush the rest of the buffer and only then emit
        // Exit — a few hundred ms is plenty.
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

        let seen = seen.lock().unwrap();
        assert!(
            matches!(seen.last(), Some(Output::Exit { .. })),
            "khung cuối cùng phải là Exit, thấy: {seen:?}",
        );
    }

    #[test]
    fn prepending_keeps_spaces_and_unicode() {
        // Each system's own spelling: `:` separates a PATH outside Windows, so a drive letter
        // there is not a path `join_paths` accepts.
        let (bin, inherited) = match cfg!(windows) {
            true => (
                r"C:\Users\Nguyễn Văn\MixEngine\bin",
                [r"C:\Windows", r"C:\Program Files\Git\bin"],
            ),
            false => (
                "/home/Nguyễn Văn/MixEngine/bin",
                ["/usr/bin", "/opt/Git Tools/bin"],
            ),
        };
        let prepend = vec![bin.to_owned()];
        let inherited = std::env::join_paths(inherited).ok();

        let joined = joined_path(&prepend, inherited).expect("a PATH");
        let parts: Vec<_> = std::env::split_paths(&joined).collect();

        assert_eq!(parts[0], std::path::PathBuf::from(bin));
        assert_eq!(parts.len(), 3);
    }

    #[test]
    fn nothing_to_prepend_changes_nothing() {
        assert_eq!(joined_path(&[], None), None);
    }

    #[test]
    fn a_missing_cwd_is_refused_by_name() {
        let gone = std::env::temp_dir().join("mixlab-cwd-that-is-not-there");
        let error = check_cwd(Some(gone.to_string_lossy().as_ref())).expect_err("refused");
        assert!(
            format!("{error:?}").contains("terminalCwdMissing"),
            "{error:?}"
        );
    }
}
