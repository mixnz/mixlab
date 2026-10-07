//! Every tunnel MixLab is running — T203, D3. Held here, not in the tab, so the tab and the tray
//! panel read one list and closing the tab stops nothing.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use mixengine_platform::process::{spawn_supervised, Limits, OutputPipe, Supervised};
use tauri::{AppHandle, Emitter, Runtime};

use super::process::{blocks_the_host, hint_for, public_url, Hint, Record};
use crate::error::AppError;

/// Said whenever the list changes; carries nothing, and the listener reads `tunnel_list`.
pub const CHANGED_EVENT: &str = "tunnel://changed";

/// How many of cloudflared's last lines a tunnel that ends before it opens shows as its error.
const TAIL_LINES: usize = 6;

/// One tunnel as the tab and the tray see it.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelInfo {
    pub id: u32,
    pub target: String,
    pub url: Option<String>,
    /// `connecting`, `open` or `failed`. A stopped tunnel leaves the list.
    pub state: &'static str,
    pub detail: Option<String>,
    pub hint: Option<Hint>,
}

struct Entry {
    info: TunnelInfo,
    process: Option<Supervised>,
    /// The last lines cloudflared printed, for a tunnel that ends before it opens.
    tail: Vec<String>,
    /// Set by `stop`, so the reader thread does not call a stop a failure.
    stopping: bool,
}

type Entries = Arc<Mutex<BTreeMap<u32, Entry>>>;

/// The managed state.
#[derive(Default)]
pub struct Tunnels {
    next: Mutex<u32>,
    entries: Entries,
}

impl Tunnels {
    pub fn list(&self) -> Vec<TunnelInfo> {
        lock(&self.entries)
            .values()
            .map(|entry| entry.info.clone())
            .collect()
    }

    /// Starts `cloudflared tunnel --url <target>` and answers the new row, still connecting.
    pub fn start<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        program: &Path,
        target: String,
        records: &Path,
    ) -> Result<TunnelInfo, AppError> {
        let id = {
            let mut next = self.next.lock().unwrap_or_else(|e| e.into_inner());
            *next += 1;
            *next
        };
        let args: Vec<OsString> = ["tunnel", "--no-autoupdate", "--url", target.as_str()]
            .into_iter()
            .map(OsString::from)
            .collect();
        let directory = program
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(std::env::temp_dir);
        let mut child = spawn_supervised(
            program,
            &args,
            &directory,
            &BTreeMap::new(),
            &Limits::default(),
        )
        .map_err(|e| err!("error.tunnelCannotStart", message = e))?;
        let stderr = child.take_stderr();
        remember(records, &child);

        let info = TunnelInfo {
            id,
            target: target.clone(),
            url: None,
            state: "connecting",
            detail: None,
            hint: None,
        };
        lock(&self.entries).insert(
            id,
            Entry {
                info: info.clone(),
                process: Some(child),
                tail: Vec::new(),
                stopping: false,
            },
        );
        let _ = app.emit(CHANGED_EVENT, ());

        let entries = Arc::clone(&self.entries);
        let app = app.clone();
        std::thread::spawn(move || read_output(&app, &entries, id, stderr));
        Ok(info)
    }

    /// Ends one tunnel and takes it off the list.
    pub fn stop<R: Runtime>(&self, app: &AppHandle<R>, id: u32) {
        let process = lock(&self.entries).get_mut(&id).and_then(|entry| {
            entry.stopping = true;
            entry.process.take()
        });
        if let Some(mut process) = process {
            let _ = process.stop();
        }
        lock(&self.entries).remove(&id);
        let _ = app.emit(CHANGED_EVENT, ());
    }

    /// Every tunnel, at MixLab's exit.
    pub fn stop_all(&self) {
        let processes: Vec<Supervised> = {
            let mut guard = lock(&self.entries);
            let taken = guard
                .values_mut()
                .filter_map(|entry| {
                    entry.stopping = true;
                    entry.process.take()
                })
                .collect();
            guard.clear();
            taken
        };
        for mut process in processes {
            let _ = process.stop();
        }
    }
}

fn lock(entries: &Entries) -> MutexGuard<'_, BTreeMap<u32, Entry>> {
    entries.lock().unwrap_or_else(|e| e.into_inner())
}

/// Reads cloudflared's output until it ends, moving the row as it goes.
fn read_output<R: Runtime>(
    app: &AppHandle<R>,
    entries: &Entries,
    id: u32,
    stderr: Option<OutputPipe>,
) {
    if let Some(stderr) = stderr {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let mut changed = false;
            let mut opened = None;
            {
                let mut guard = lock(entries);
                let Some(entry) = guard.get_mut(&id) else {
                    return;
                };
                entry.tail.push(line.clone());
                if entry.tail.len() > TAIL_LINES {
                    entry.tail.remove(0);
                }
                if entry.info.url.is_none() {
                    if let Some(url) = public_url(&line) {
                        entry.info.url = Some(url.clone());
                        entry.info.state = "open";
                        opened = Some(url);
                        changed = true;
                    }
                }
                if entry.info.hint.is_none() {
                    if let Some(hint) = hint_for(&line) {
                        entry.info.hint = Some(hint);
                        changed = true;
                    }
                }
            }
            if changed {
                let _ = app.emit(CHANGED_EVENT, ());
            }
            if let Some(url) = opened {
                let (app, entries) = (app.clone(), Arc::clone(entries));
                std::thread::spawn(move || check_the_host(&app, &entries, id, &url));
            }
        }
    }
    // The output closed: the process has gone, on its own or because it was stopped.
    let process = lock(entries).get_mut(&id).and_then(|entry| {
        if !entry.stopping {
            entry.info.state = "failed";
            entry.info.detail = Some(entry.tail.join("\n"));
        }
        entry.process.take()
    });
    if let Some(mut process) = process {
        let _ = process.wait();
    }
    let _ = app.emit(CHANGED_EVENT, ());
}

/// One `GET /` through a tunnel that has just opened — T203, D4. A dev server that refuses the
/// tunnel's host (Vite 5 and later) answers `403 Blocked request`, which reads as a broken tunnel
/// when it is a setting; the row then names `server.allowedHosts`. One request, once: the module does
/// not watch the traffic.
fn check_the_host<R: Runtime>(app: &AppHandle<R>, entries: &Entries, id: u32, url: &str) {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return;
    };
    let blocked = runtime.block_on(async {
        let response = reqwest::Client::new()
            .get(url)
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await
            .ok()?;
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        Some(blocks_the_host(status, &body))
    });
    if blocked != Some(true) {
        return;
    }
    if let Some(entry) = lock(entries).get_mut(&id) {
        entry.info.hint = Some(Hint::AllowedHosts);
    }
    let _ = app.emit(CHANGED_EVENT, ());
}

/// macOS only: write the group down so the next start can end it after a crash. Elsewhere the
/// kernel does it — a job object on Windows, `PR_SET_PDEATHSIG` on Linux.
fn remember(records: &Path, child: &Supervised) {
    if !cfg!(target_os = "macos") {
        return;
    }
    let Ok(Some(started)) = child.started_at() else {
        return;
    };
    let mut existing = Record::parse_all(&std::fs::read_to_string(records).unwrap_or_default());
    existing.push(Record {
        pid: child.pid(),
        started: started.stored(),
    });
    let _ = std::fs::write(records, Record::render_all(&existing));
}

/// At start, on macOS: end what a crashed MixLab left running, and forget the file.
pub fn sweep(records: &Path) {
    if !cfg!(target_os = "macos") {
        return;
    }
    let text = std::fs::read_to_string(records).unwrap_or_default();
    for record in Record::parse_all(&text) {
        let ours = record.still_ours(|pid| {
            mixengine_platform::process::started_at(pid)
                .ok()
                .flatten()
                .map(|started| started.stored())
        });
        if ours && is_cloudflared(record.pid) {
            end_group(record.pid);
        }
    }
    let _ = std::fs::remove_file(records);
}

#[cfg(target_os = "macos")]
fn is_cloudflared(pid: u32) -> bool {
    let mut name = [0u8; 256];
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    // SAFETY: `proc_name` writes at most the length it is given into the buffer it is given.
    let len = unsafe { libc::proc_name(pid, name.as_mut_ptr().cast(), name.len() as u32) };
    usize::try_from(len).is_ok_and(|len| len > 0 && name[..len].starts_with(b"cloudflared"))
}

#[cfg(not(target_os = "macos"))]
fn is_cloudflared(_pid: u32) -> bool {
    false
}

#[cfg(unix)]
fn end_group(pid: u32) {
    if let Ok(pid) = libc::pid_t::try_from(pid) {
        // SAFETY: a signal to a process group this module recorded as its own, checked above by its
        // start time and its name.
        unsafe {
            libc::killpg(pid, libc::SIGTERM);
        }
    }
}

#[cfg(not(unix))]
fn end_group(_pid: u32) {}

/// Where the records live.
pub fn records_path(data_dir: &Path) -> PathBuf {
    data_dir.join("tunnel").join("running")
}
