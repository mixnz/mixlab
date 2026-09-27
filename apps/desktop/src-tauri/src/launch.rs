//! What this process was started with, and how the running copy hears about later starts.
//!
//! Three things, all about the process rather than about any module:
//!
//! - [`Opening`]: the `mixlab://` URL on the command line, if there is one, and the credential
//!   taken out of the environment for it. Read **once, on the first line of `run()`**, while
//!   `main` is still one thread: the Tauri builder spawns threads, the webview forks helpers, the
//!   terminal module opens shells, and every one of those would inherit a variable still there.
//! - [`Requests`]: tabs the backend asks the shell to open. Queued and drained rather than
//!   delivered by event alone — the opening of this very process is accepted in `setup`, before the
//!   webview has a listener, and an event fired then reaches nobody. So the event only says
//!   "look", and the looking is one command the shell also calls on mount.
//! - [`accept`]: the one place a URL is matched to a module — the Rust side of
//!   `shell/registry.ts`.
//!
//! The channel between two copies of the app is `crate::instance`; the URL itself is the database
//! module's, in `modules/db/handoff.rs`. The design:
//! `docs/specs/2026-09-03-mixengine-connection-handoff-design.md`.

#[cfg(windows)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

use crate::instance;
use crate::modules::db::handoff;
use crate::secrets::Redacted;

/// What the process was started with: a `mixlab://` URL, or nothing, and the password read for it.
pub struct Opening {
    pub url: Option<String>,
    pub secret: Option<String>,
    /// `--hidden`: started by the login entry, so only the tray comes up (ADR 0042, T168f).
    pub hidden: bool,
}

/// Written out so the secret cannot reach a log line — see `ConnectionConfig`'s for the reasoning.
impl std::fmt::Debug for Opening {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Opening")
            .field("url", &self.url)
            .field("secret", &self.secret.as_ref().map(|_| Redacted))
            .field("hidden", &self.hidden)
            .finish()
    }
}

impl Opening {
    /// This process's own command line and environment. Call it before anything else in `run()`:
    /// the variable is removed as it is read, and the removal is only worth anything while nothing
    /// has been started that could have inherited it.
    pub fn from_process() -> Self {
        Self::from_args(
            std::env::args_os()
                .skip(1)
                .map(|arg| arg.to_string_lossy().into_owned()),
            |name| {
                let value = std::env::var(name).ok();
                // A value that is not unicode still goes: nothing here can use it, and nothing
                // below should be able to see it either.
                std::env::remove_var(name);
                value
            },
        )
    }

    /// The pure half: `args` without the program name, and `take_env` standing in for reading
    /// a variable and removing it. Asked for exactly the variable the URL names, and only when
    /// that name is one a launcher would use — see [`handoff::credential_name`].
    ///
    /// `--hidden` may stand anywhere; the URL is still the first argument or nothing.
    pub fn from_args(
        args: impl Iterator<Item = String>,
        mut take_env: impl FnMut(&str) -> Option<String>,
    ) -> Self {
        let args: Vec<String> = args.collect();
        let hidden = args.iter().any(|arg| arg == HIDDEN);
        let Some(url) = args
            .into_iter()
            .next()
            .filter(|arg| arg.starts_with("mixlab://"))
        else {
            return Self {
                url: None,
                secret: None,
                hidden,
            };
        };
        let secret = handoff::credential_name(&url)
            .and_then(|name| take_env(&name))
            .filter(|value| !value.is_empty());
        Self {
            url: Some(url),
            secret,
            hidden,
        }
    }
}

/// The argument the login entry starts MixLab with — `tauri-plugin-autostart` in `lib.rs`.
pub const HIDDEN: &str = "--hidden";

/// A tab the backend wants opened: which module, and the state that module reads on mount — the
/// same slot `restored` carries between launches, so the shell learns nothing new.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabRequest {
    pub module_id: &'static str,
    pub state: serde_json::Value,
}

/// The event that tells the shell there is something to take. Carries no payload on purpose.
pub const REQUEST_EVENT: &str = "launch://request";

/// Tab requests not yet taken, in the order they were made.
#[derive(Default)]
pub struct Requests {
    pending: Mutex<Vec<TabRequest>>,
}

impl Requests {
    fn push(&self, request: TabRequest) {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(request);
    }

    fn take(&self) -> Vec<TabRequest> {
        std::mem::take(&mut *self.pending.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

/// Puts the queue in the app. Called once, from `lib.rs`.
pub fn register<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.manage(Requests::default())
}

/// Queues `request` and tells the shell to look.
pub fn request<R: Runtime>(app: &AppHandle<R>, request: TabRequest) {
    app.state::<Requests>().push(request);
    let _ = app.emit(REQUEST_EVENT, ());
}

/// Every request made since the last call, oldest first. The shell calls it on mount and on
/// every [`REQUEST_EVENT`].
#[tauri::command]
pub fn launch_take_requests(state: State<'_, Requests>) -> Vec<TabRequest> {
    state.take()
}

/// A URL that arrived by any door, handed to the module that answers it.
///
/// `secret` is the credential read for it — only ever `Some` for the URL a fresh process was
/// started with, or one forwarded by such a process. A URL from the OS's scheme handler has none:
/// a link cannot set a variable, and the running process's own were taken on its first line.
pub fn accept<R: Runtime>(app: &AppHandle<R>, url: &str, secret: Option<String>) {
    let host = url::Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(str::to_owned));
    match host.as_deref() {
        Some("connect") => {
            if let Err(error) = handoff::accept(app, url, secret) {
                eprintln!("mixdb: ignoring the connection this was started with: {error}");
            }
        }
        _ => eprintln!("mixdb: ignoring a URL nothing here answers: {url}"),
    }
}

/// The line a second copy sends the first. No `Debug`: nothing prints it, and it has no business
/// being printable.
#[derive(Serialize, Deserialize)]
struct Message {
    url: Option<String>,
    secret: Option<String>,
    /// Left off the line when false, so a copy from before T168f reads every line it ever did.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    hidden: bool,
}

impl Message {
    fn from(opening: &Opening) -> Self {
        Self {
            url: opening.url.clone(),
            secret: opening.secret.clone(),
            hidden: opening.hidden,
        }
    }

    fn line(&self) -> Option<String> {
        serde_json::to_string(self).ok()
    }
}

/// How long a second copy waits for the first to take its line before giving up and becoming a
/// window of its own. MixEngine judges a started client for one second; a first copy that is
/// hung should not make the second look hung too.
const FORWARD_TIMEOUT: Duration = Duration::from_secs(3);

/// Hands `opening` to a copy of the app already running, if there is one. `true` means it was
/// taken and this process has nothing left to do. Called before the builder, so it runs the
/// exchange on Tauri's runtime itself.
pub fn forward(identifier: &str, opening: &Opening) -> bool {
    let Some(line) = Message::from(opening).line() else {
        return false;
    };
    let endpoint = instance::Endpoint::for_app(identifier);
    tauri::async_runtime::block_on(async {
        tokio::time::timeout(FORWARD_TIMEOUT, instance::forward(&endpoint, &line))
            .await
            .unwrap_or(false)
    })
}

/// Everything that happens in `setup`: listen for other copies, accept this process's own
/// opening, and on the systems where it applies, hook up the OS's scheme handler.
pub fn start<R: Runtime>(app: &AppHandle<R>, opening: Opening) {
    let endpoint = instance::Endpoint::for_app(&app.config().identifier);
    let listener = app.clone();
    tauri::async_runtime::spawn(instance::serve(endpoint, move |line| {
        received(&listener, &line)
    }));

    if let Some(url) = &opening.url {
        accept(app, url, opening.secret.clone());
    }

    // The Apple Event is the only way a URL reaches a macOS process — one already running, or one
    // `open` just started for it. Never with a credential: a link sets no variable, and this
    // process's own were taken on its first line. Not on Windows or Linux, where the plugin
    // re-emits `argv` at setup — which `Opening` already read — and a URL arriving later comes
    // over the channel above rather than through the OS.
    #[cfg(target_os = "macos")]
    {
        use tauri_plugin_deep_link::DeepLinkExt;
        let handle = app.clone();
        app.deep_link().on_open_url(move |event| {
            for url in event.urls() {
                accept(&handle, url.as_str(), None);
            }
        });
    }

    // An AppImage has no installer to write the scheme handler; the .deb's is written twice, which
    // is harmless. Best effort — the scheme is a convenience, not something the app needs to run.
    #[cfg(target_os = "linux")]
    {
        use tauri_plugin_deep_link::DeepLinkExt;
        if let Err(e) = app.deep_link().register_all() {
            eprintln!("mixlab: could not register the mixlab:// scheme: {e}");
        }
    }
}

/// On the way out: the socket file, on the systems that have one.
pub fn stop<R: Runtime>(app: &AppHandle<R>) {
    instance::cleanup(&instance::Endpoint::for_app(&app.config().identifier));
}

/// A line from another copy: bring the window up, and open what it carried, if anything.
///
/// Except a login start with nothing to open: the login entry firing while MixLab already runs
/// — somebody started it by hand first — must not throw a window at them (T168f).
fn received<R: Runtime>(app: &AppHandle<R>, line: &str) {
    match serde_json::from_str::<Message>(line) {
        Ok(Message {
            url: None,
            hidden: true,
            ..
        }) => {}
        Ok(Message {
            url: Some(url),
            secret,
            ..
        }) => {
            bring_to_front(app);
            accept(app, &url, secret);
        }
        Ok(Message { url: None, .. }) => bring_to_front(app),
        Err(e) => {
            bring_to_front(app);
            eprintln!("mixdb: ignoring a line from another copy: {e}");
        }
    }
}

/// Whether `main` has been shown once, and so had its saved maximized flag put back (Windows).
#[cfg(windows)]
static PLACED: AtomicBool = AtomicBool::new(false);

pub(crate) fn bring_to_front<R: Runtime>(app: &AppHandle<R>) {
    // Back in the Dock first, when closing it to the tray took it out (T168).
    crate::tray::show_in_dock(app);
    if let Some(window) = app.get_webview_window("main") {
        /* On Windows, tao answers anything that touches a maximized window that is still hidden —
        maximizing it, and `unminimize` too — by showing it and hiding it again at once. When the
        window-state plugin maximized `main` as it was created, every start flashed an empty window
        and took it away until `setup` was done. And `show` takes tens of milliseconds while the
        webview resizes, so maximizing just before it still flashes, and just after it shows a
        window at the wrong size first. So the first showing is one call that shows it maximized,
        and `show` after it, for tao's own record, changes nothing on screen. */
        #[cfg(windows)]
        if !PLACED.swap(true, Ordering::SeqCst) && crate::platform::saved_maximized(app, "main") {
            crate::platform::show_maximized(&window);
        }
        // `show` before `unminimize`, for the reason above: a window closed to the tray while
        // maximized is still maximized.
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What crosses the channel: both halves, and never a third field that could carry anything.
    #[test]
    fn a_message_round_trips() {
        let line = Message::from(&Opening {
            url: Some("mixlab://connect?x".to_string()),
            secret: Some("s".to_string()),
            hidden: false,
        })
        .line()
        .unwrap();
        assert_eq!(line, r#"{"url":"mixlab://connect?x","secret":"s"}"#);
        let back: Message = serde_json::from_str(&line).unwrap();
        assert_eq!(back.url.as_deref(), Some("mixlab://connect?x"));
        assert_eq!(back.secret.as_deref(), Some("s"));

        let bare = Message::from(&Opening {
            url: None,
            secret: None,
            hidden: false,
        })
        .line()
        .unwrap();
        assert_eq!(bare, r#"{"url":null,"secret":null}"#);
    }

    /// A login start says so; a line from a copy that predates it still parses, as not hidden.
    #[test]
    fn a_hidden_start_crosses_the_channel_and_old_lines_still_parse() {
        let line = Message::from(&Opening {
            url: None,
            secret: None,
            hidden: true,
        })
        .line()
        .unwrap();
        assert_eq!(line, r#"{"url":null,"secret":null,"hidden":true}"#);

        let old: Message = serde_json::from_str(r#"{"url":null,"secret":null}"#).unwrap();
        assert!(!old.hidden);
    }

    #[test]
    fn hidden_is_read_wherever_it_stands() {
        assert!(Opening::from_args(args(&["--hidden"]), |_| panic!("asked")).hidden);
        let both = Opening::from_args(
            args(&["mixlab://connect?kind=redis&host=h&port=1", "--hidden"]),
            |_| panic!("asked"),
        );
        assert!(both.hidden);
        assert!(both.url.is_some());
        assert!(!Opening::from_args(args(&["--flag"]), |_| panic!("asked")).hidden);
    }

    fn args(list: &[&str]) -> impl Iterator<Item = String> {
        list.iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn a_url_with_a_credential_variable_takes_it_out_of_the_environment() {
        let mut asked = Vec::new();
        let opening = Opening::from_args(
            args(&["mixlab://connect?kind=mysql&host=h&port=1&password_env=MIXENGINE_DB_PASSWORD"]),
            |name| {
                asked.push(name.to_string());
                Some("s3cret".to_string())
            },
        );
        assert_eq!(asked, vec!["MIXENGINE_DB_PASSWORD"]);
        assert_eq!(
            opening.url.as_deref(),
            Some("mixlab://connect?kind=mysql&host=h&port=1&password_env=MIXENGINE_DB_PASSWORD")
        );
        assert_eq!(opening.secret.as_deref(), Some("s3cret"));
    }

    #[test]
    fn a_url_without_one_asks_for_nothing() {
        let opening =
            Opening::from_args(args(&["mixlab://connect?kind=redis&host=h&port=1"]), |_| {
                panic!("asked")
            });
        assert!(opening.url.is_some());
        assert_eq!(opening.secret, None);
    }

    /// The variable is taken whether or not the rest of the URL is any good.
    #[test]
    fn a_broken_url_still_takes_the_variable() {
        let mut asked = Vec::new();
        let opening = Opening::from_args(
            args(&["mixlab://connect?password_env=MIXLAB_PASSWORD"]),
            |name| {
                asked.push(name.to_string());
                None
            },
        );
        assert_eq!(asked, vec!["MIXLAB_PASSWORD"]);
        assert_eq!(opening.secret, None);
    }

    #[test]
    fn an_empty_variable_is_no_credential() {
        let opening = Opening::from_args(
            args(&["mixlab://connect?password_env=MIXLAB_PASSWORD"]),
            |_| Some(String::new()),
        );
        assert_eq!(opening.secret, None);
    }

    #[test]
    fn anything_else_on_the_command_line_is_not_an_opening() {
        for list in [
            &[][..],
            &["--flag"][..],
            &["C:\\file.sql"][..],
            &["https://example.com"][..],
        ] {
            let opening = Opening::from_args(args(list), |_| panic!("asked"));
            assert_eq!(opening.url, None);
            assert_eq!(opening.secret, None);
        }
    }

    #[test]
    fn an_opening_never_prints_its_secret() {
        let opening = Opening {
            url: Some("mixlab://connect".to_string()),
            secret: Some("hunter2".to_string()),
            hidden: false,
        };
        let printed = format!("{opening:?}");
        assert!(!printed.contains("hunter2"), "{printed}");
        assert!(printed.contains("mixlab://connect"));
        assert!(printed.contains("Some(\"***\")"));
    }

    /// What the shell receives: `moduleId`, not `module_id`.
    #[test]
    fn a_request_is_camel_cased_for_the_shell() {
        let json = serde_json::to_value(TabRequest {
            module_id: "db",
            state: serde_json::json!({ "handoffId": "x" }),
        })
        .unwrap();
        assert_eq!(json["moduleId"], "db");
        assert_eq!(json["state"]["handoffId"], "x");
    }

    #[test]
    fn requests_are_taken_once_in_order() {
        let requests = Requests::default();
        requests.push(TabRequest {
            module_id: "db",
            state: serde_json::Value::Null,
        });
        requests.push(TabRequest {
            module_id: "rest",
            state: serde_json::Value::Null,
        });
        let taken = requests.take();
        assert_eq!(
            taken.iter().map(|r| r.module_id).collect::<Vec<_>>(),
            vec!["db", "rest"]
        );
        assert!(requests.take().is_empty());
    }
}
