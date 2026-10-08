use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// The frame size, in character cells.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct TerminalSize {
    pub cols: u16,
    pub rows: u16,
}

/// A shell detected on this machine.
#[derive(Debug, Clone, Serialize)]
pub struct LocalShell {
    /// A stable identifier — `shells.ts` turns it into a display label.
    pub name: String,
    pub path: String,
    /// Fixed arguments; empty for most, `["-d", "<distro>"]` for WSL.
    pub args: Vec<String>,
}

/// Where the session opens.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TerminalTarget {
    Local {
        /// `None` is "the machine's default shell".
        shell: Option<String>,
        #[serde(default)]
        args: Vec<String>,
        cwd: Option<String>,
        /// Extra variables for the shell — T205, D9. Generic: this module knows nothing of what
        /// they mean.
        #[serde(default)]
        env: std::collections::BTreeMap<String, String>,
        /// Directories put ahead of the inherited `PATH` — T205, D9.
        #[serde(default, rename = "pathPrepend")]
        path_prepend: Vec<String>,
    },
    /// The server the session opens on. Exactly the `SshConfig` the tunnel uses — those four
    /// fields are the four fields of an SSH server, not of whatever sits at the other end.
    Ssh(crate::ssh::SshConfig),
}

/// The only thing the session sends back up to the UI as JSON. Bytes go straight, unwrapped.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TerminalEvent {
    Exit {
        code: Option<i32>,
        message: Option<String>,
    },
}

/// What the far end says. `commands.rs` is the only place that turns this into an IPC frame —
/// thanks to that the whole session layer runs under `cargo test` without a webview.
#[derive(Debug, Clone)]
pub enum Output {
    Data(Vec<u8>),
    Exit {
        code: Option<i32>,
        message: Option<String>,
    },
}

pub type OutputSink = Arc<dyn Fn(Output) + Send + Sync>;
