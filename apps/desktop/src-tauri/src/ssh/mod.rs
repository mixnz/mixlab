use crate::error::AppError;
use crate::platform::in_background;
use crate::secrets::Redacted;
use russh::client::{self};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinHandle;
use tokio::time::timeout;

/// How to prove who you are to the SSH server.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SshAuth {
    Password {
        password: String,
    },
    PrivateKey {
        key_path: String,
        passphrase: Option<String>,
    },
}

/// Redacted by hand, for the reason `ConnectionConfig`'s is. This one covers more than itself:
/// `SshConfig` derives its `Debug` from here, and so does `TerminalTarget` from that — so the
/// terminal's copy of the problem is fixed by fixing the leaf rather than each of the three.
impl std::fmt::Debug for SshAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Password { .. } => f
                .debug_struct("Password")
                .field("password", &Redacted)
                .finish(),
            Self::PrivateKey {
                key_path,
                passphrase,
            } => f
                .debug_struct("PrivateKey")
                // The path is not a secret and is the whole of what is worth knowing when a key
                // will not load — which is the one time anybody prints this.
                .field("key_path", key_path)
                .field("passphrase", &passphrase.as_ref().map(|_| Redacted))
                .finish(),
        }
    }
}

/// The server to tunnel through. Config of this layer rather than of whatever is at the far end,
/// which is why it lives here and not with any one module's models: a terminal opened over SSH
/// wants the same four fields a tunnelled database connection does.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: SshAuth,
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const CHANNEL_OPEN_TIMEOUT: Duration = Duration::from_secs(10);

/// The interval at which russh sends a keepalive packet while the line is quiet.
///
/// 15 seconds: short enough to beat a home NAT's idle timeout (usually 300 seconds) and sshd's
/// `ClientAliveInterval`; long enough that a session left idle all day only costs a few hundred
/// bytes.
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);

/// How many consecutive unanswered keepalives before russh ends the session — also its default.
/// Multiplied by the interval above, a dead line is detected within about 45 seconds.
const KEEPALIVE_MAX: usize = 3;

/// The minimum pause after a failed authentication.
///
/// Without it, a pool trying to open connections while the network is dead would fire dozens of
/// authentications a minute at an sshd that has `MaxAuthTries` — and possibly fail2ban.
const RETRY_COOLDOWN: Duration = Duration::from_secs(3);

/// The interval at which the watcher checks the session while everything is fine.
const WATCH_IDLE: Duration = Duration::from_secs(15);

/// The interval right after the first failure, before it gradually widens.
const WATCH_MIN: Duration = Duration::from_secs(5);

/// The backoff's cap: an SSH server that really cannot be reached is tried once a minute, no more.
const WATCH_MAX: Duration = Duration::from_secs(60);

/// How long the accept loop waits after an error that does not belong to a single connection —
/// out of file descriptors, temporarily out of memory. Long enough that a repeating error does not
/// burn a whole core, short enough that the next query through the tunnel does not notice.
const ACCEPT_RETRY: Duration = Duration::from_millis(100);

/// This many consecutive failures before bothering the user — about two seconds of the port
/// accepting nothing. Any shorter and a passing run out of file descriptors would flash the banner.
const ACCEPT_ALARM: u32 = 20;

/// An `accept` error belongs to exactly one connection that just failed, not to the listening
/// port.
///
/// A connection cancelled mid-handshake is routine for a pool: Windows returns `WSAECONNRESET` or
/// `WSAECONNABORTED`, Unix returns `ECONNABORTED`, and `EINTR` is a signal interrupting the call.
/// The next `accept` still succeeds as if nothing happened.
///
/// Misclassifying costs nothing but one `ACCEPT_RETRY` beat: from here on both branches retry, and
/// this list only decides whether to wait and whether to count towards the banner.
fn is_transient_accept(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        ErrorKind::ConnectionAborted
            | ErrorKind::ConnectionReset
            | ErrorKind::ConnectionRefused
            | ErrorKind::Interrupted
    )
}

/// The watcher's next waiting interval.
///
/// Compared with `==` rather than `>=`: `WATCH_MAX` is larger than `WATCH_IDLE`, so a condition of
/// "greater than or equal to the idle interval" would pull even the capped interval back to
/// `WATCH_MIN` and turn the backoff into a loop. `WATCH_IDLE` is only set on success, so comparing
/// for equality is exact.
fn next_backoff(current: Duration, ok: bool) -> Duration {
    if ok {
        return WATCH_IDLE;
    }
    if current == WATCH_IDLE {
        return WATCH_MIN;
    }
    (current * 2).min(WATCH_MAX)
}

/// How much a channel may have in flight before the peer has to wait for the receiver to catch up.
///
/// russh defaults to 2MB, which is less than a distant link holds in flight: at 25MB/s and 100ms
/// of round trip there is 2.5MB in the air at any moment, so the sender spends part of its time
/// stopped, waiting for credit that is still travelling back. A dump is the one thing in the app
/// that runs long enough for that to be worth the memory.
const WINDOW_SIZE: u32 = 8 * 1024 * 1024;

/// The buffer each direction of a forwarded connection copies through.
///
/// 8KB — what this used to read into — is a syscall and a wakeup per 8KB, which caps a forward
/// well below what the link can carry. The cost of the larger buffer is that every connection held
/// open through the tunnel keeps two of these, so it is sized to be past the point of diminishing
/// returns rather than as large as possible.
const BRIDGE_BUFFER: usize = 128 * 1024;

/// Where the fingerprint of every SSH server MixLab has connected to is remembered, keyed by
/// `host:port`. Its own file rather than OpenSSH's `~/.ssh/known_hosts`: that file is the user's,
/// written in a format with its own hashing and wildcard rules, and an app that only ever appends
/// to it has no business rewriting it.
fn known_hosts_file(app_data: &Path) -> PathBuf {
    app_data.join("known_hosts.json")
}

fn load_known_hosts(path: &Path) -> HashMap<String, String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Serialises the read-modify-write above.
///
/// Two tunnels opening at once to two servers neither of which has been seen before both read the
/// file, both add their own entry, and whichever writes second drops the other's. A lock private
/// to this process is enough: the file is MixLab's own, and nothing outside it writes there.
static KNOWN_HOSTS_LOCK: Mutex<()> = Mutex::new(());

fn remember_host(path: &Path, endpoint: &str, fingerprint: &str) -> Result<(), AppError> {
    // The guarded value is `()`, so a poisoned lock has nothing left half-written to protect —
    // panicking here would turn one panic elsewhere into every later connection failing.
    let _guard = KNOWN_HOSTS_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let mut known = load_known_hosts(path);
    known.insert(endpoint.to_string(), fingerprint.to_string());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            err!(
                "error.cannotCreateDirectory",
                path = parent.display(),
                message = e
            )
        })?;
    }
    let text = serde_json::to_string_pretty(&known)
        .map_err(|e| err!("error.cannotSaveKnownHost", message = e))?;

    /* Written beside the real file and renamed over it, rather than into it. `std::fs::write`
    truncates first, so a crash — or a full disk — between the truncate and the last byte leaves
    an empty or half-written file, `load_known_hosts` reads that as "nothing known", and every
    server the user has ever connected to is silently accepted afresh on the next run. That is
    the one failure this file exists to prevent. A rename either happened or did not. */
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, text).map_err(|e| err!("error.cannotSaveKnownHost", message = e))?;
    std::fs::rename(&temp, path).map_err(|e| {
        // Nothing was replaced, so the leftover is only clutter — but it is clutter next to a file
        // a worried user may well come and read.
        let _ = std::fs::remove_file(&temp);
        err!("error.cannotSaveKnownHost", message = e)
    })
}

/// Whether this key is the one this address answered with last time.
///
/// Split out of the handler because it is all blocking file work and the handler is `async`: the
/// caller runs it off the runtime. Never returns "no" — a key that does not match is an error with
/// something to say, and saying it is the only reason this is not a plain `bool`.
fn verify_host(path: &Path, endpoint: &str, fingerprint: &str) -> Result<(), AppError> {
    match load_known_hosts(path).get(endpoint) {
        Some(known) if known == fingerprint => Ok(()),
        Some(known) => Err(err!(
            "error.sshHostKeyChanged",
            endpoint = endpoint,
            fingerprint = fingerprint,
            known = known,
            file = path.display(),
        )),
        // First sight of this server: take it on faith, and hold it to that key from now on.
        None => remember_host(path, endpoint, fingerprint),
    }
}

/// What is happening to a tunnel, for whoever wants to tell the user about it.
///
/// `ssh/` knows nothing about Tauri — it takes a callback and calls it, and turning that into a
/// window event is the job of `commands/mod.rs`.
pub enum TunnelEvent {
    Reconnecting,
    Reconnected,
    Failed(AppError),
}

pub type TunnelNotify = Arc<dyn Fn(TunnelEvent) + Send + Sync>;

/// The SSH session in use, and a trace of the latest opening.
struct SessionSlot {
    /// `None` means there is no session yet, or the latest reopening failed.
    handle: Option<Arc<client::Handle<TunnelHandler>>>,
    /// When the current session was opened. A session younger than `RETRY_COOLDOWN` is not thrown
    /// away over one failed channel opening: a server that flatly refuses forwarding
    /// (`PermitOpen`, `AllowTcpForwarding no`) fails every time, and re-authenticating for every
    /// refused connection would only hammer the server.
    opened_at: Option<Instant>,
    /// The latest failure, so the SSH server is not hammered with a chain of failed
    /// authentications.
    failed_at: Option<Instant>,
}

/// Everything needed to reopen the session, shared by the accept loop, the watcher and every
/// bridge task.
struct TunnelInner {
    ssh: SshConfig,
    remote_host: String,
    remote_port: u16,
    app_data: PathBuf,
    notify: TunnelNotify,
    session: AsyncMutex<SessionSlot>,
}

impl TunnelInner {
    /// The session in use, reopened if the old one has died.
    ///
    /// Holding the lock for the whole authentication is deliberate: a pool opening five connections
    /// at once has all five wait behind **one** `authenticate`, not five. The price is that while
    /// reopening, every new connection through this tunnel waits up to `CONNECT_TIMEOUT` (10
    /// seconds) — well within sqlx's default 30-second `acquire_timeout`.
    async fn session(&self) -> Result<Arc<client::Handle<TunnelHandler>>, AppError> {
        let mut slot = self.session.lock().await;
        if let Some(handle) = slot.handle.as_ref().filter(|handle| !handle.is_closed()) {
            return Ok(Arc::clone(handle));
        }
        if let Some(at) = slot.failed_at {
            if at.elapsed() < RETRY_COOLDOWN {
                return Err(err!("error.sshUnavailable"));
            }
        }

        (self.notify)(TunnelEvent::Reconnecting);
        match authenticate(&self.ssh, &self.app_data).await {
            Ok(session) => {
                let handle = Arc::new(session);
                *slot = SessionSlot {
                    handle: Some(Arc::clone(&handle)),
                    opened_at: Some(Instant::now()),
                    failed_at: None,
                };
                (self.notify)(TunnelEvent::Reconnected);
                Ok(handle)
            }
            Err(e) => {
                *slot = SessionSlot {
                    handle: None,
                    opened_at: None,
                    failed_at: Some(Instant::now()),
                };
                (self.notify)(TunnelEvent::Failed(e.clone()));
                Err(e)
            }
        }
    }

    /// Throws the current session away, so the next `session()` opens a new one.
    ///
    /// `is_closed()` is the quick way to detect it, not the only one: a session that just died may
    /// not have reported it yet, and the first thing to fail is `channel_open_direct_tcpip`. The
    /// cooldown here blocks the opposite case — the session is alive but the server refuses
    /// forwarding; opening a new session then helps nothing and must not be repeated for every
    /// connection.
    async fn forget_session(&self) {
        let mut slot = self.session.lock().await;
        if slot
            .opened_at
            .is_none_or(|at| at.elapsed() >= RETRY_COOLDOWN)
        {
            slot.handle = None;
            slot.opened_at = None;
        }
    }
}

/// A running port forward, torn down as soon as this is dropped.
///
/// The tasks cannot be held as bare `JoinHandle`s: dropping one of those detaches the task rather
/// than stopping it. Every connection attempt that failed *after* the tunnel came up — a mistyped
/// database password, say — would then leave an authenticated SSH session and a bound local port
/// running for the life of the process, with nothing left holding a handle to either.
pub struct Tunnel {
    inner: Arc<TunnelInner>,
    accept: JoinHandle<()>,
    watch: JoinHandle<()>,
}

impl Tunnel {
    /// A cheap handle to the same session, so the caller can reopen it without holding the lock
    /// `Tunnel` sits behind — authentication takes up to 10 seconds, and the connection map must
    /// not be locked that long. See `commands::tunnel_reconnect`.
    pub fn session_handle(&self) -> TunnelSession {
        TunnelSession(Arc::clone(&self.inner))
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.accept.abort();
        self.watch.abort();
    }
}

/// Reopens the session at the user's request, without waiting out the backoff.
pub struct TunnelSession(Arc<TunnelInner>);

impl TunnelSession {
    pub async fn reconnect(&self) -> Result<(), AppError> {
        // Clear the failure mark first, otherwise a call right after a failure would fall into the
        // cooldown and the *Retry* button would do nothing.
        {
            let mut slot = self.0.session.lock().await;
            slot.failed_at = None;
        }
        self.0.session().await.map(|_| ())
    }
}

/// Checks the server's key against what MixLab saw the last time it connected to this address.
///
/// Trust on first use: a server never seen before is accepted and its fingerprint written down,
/// and from then on a *different* key is refused. That is the half of host-key checking worth
/// having here — the first connection is taken on faith either way, but the tunnel can no longer
/// be quietly stood in front of afterwards, which is the whole reason a database is reached
/// through SSH rather than over the open network.
struct TunnelHandler {
    /// `host:port`, which is what a fingerprint is remembered under.
    endpoint: String,
    known_hosts: PathBuf,
    /// Why the key was refused, kept for the caller: `check_server_key` may only say yes or no,
    /// and "no" reaches the user as russh's own unspecific error otherwise.
    refused: Arc<Mutex<Option<AppError>>>,
}

impl client::Handler for TunnelHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        let fingerprint = server_public_key
            .fingerprint(russh::keys::HashAlg::Sha256)
            .to_string();
        let path = self.known_hosts.clone();
        let endpoint = self.endpoint.clone();

        /* Off the runtime: this reads a file and, on a first connection, writes one, and the
        thread it would do that on is the one driving the handshake. A home directory on a
        network share is where that stops being theoretical. */
        match in_background(move || verify_host(&path, &endpoint, &fingerprint)).await {
            Ok(()) => Ok(true),
            Err(e) => {
                *self.refused.lock().unwrap() = Some(e);
                Ok(false)
            }
        }
    }
}

/// A key path as sync carries it: `~/.ssh/id_rsa` is read from this machine's home, so one saved
/// host opens the same key on macOS, Windows and Linux. Any other path is taken as written, and so
/// is `~` itself when there is no home to put in its place.
fn expand_home(key_path: &str, home: Option<&Path>) -> PathBuf {
    let rest = key_path
        .strip_prefix("~/")
        .or_else(|| key_path.strip_prefix("~\\"));
    match (rest, home) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(key_path),
    }
}

async fn authenticate(
    ssh: &SshConfig,
    app_data: &Path,
) -> Result<client::Handle<TunnelHandler>, AppError> {
    match timeout(CONNECT_TIMEOUT, authenticate_inner(ssh, app_data)).await {
        Ok(result) => result,
        Err(_) => Err(err!(
            "error.sshTimeout",
            host = &ssh.host,
            port = ssh.port,
            seconds = CONNECT_TIMEOUT.as_secs(),
        )),
    }
}

async fn authenticate_inner(
    ssh: &SshConfig,
    app_data: &Path,
) -> Result<client::Handle<TunnelHandler>, AppError> {
    let config = Arc::new(client::Config {
        // Nagle's algorithm holds a small write back waiting for more to go with it, which is
        // exactly wrong under a forward: what is being delayed is usually a database's reply, and
        // nothing else is coming until the client has seen it. russh leaves it on by default.
        nodelay: true,
        window_size: WINDOW_SIZE,
        // russh sends nothing by default (`keepalive_interval: None`), so an idle session would be
        // dropped by a NAT or by sshd without anyone knowing — and `is_closed()` would never become
        // `true`. Turning it on both keeps the session alive and is the only thing that detects a
        // dead line.
        keepalive_interval: Some(KEEPALIVE_INTERVAL),
        keepalive_max: KEEPALIVE_MAX,
        // `inactivity_timeout` stays `None`: it closes the session when there is no traffic,
        // exactly what we want to avoid.
        ..client::Config::default()
    });
    let refused: Arc<Mutex<Option<AppError>>> = Arc::new(Mutex::new(None));
    let handler = TunnelHandler {
        endpoint: format!("{}:{}", ssh.host, ssh.port),
        known_hosts: known_hosts_file(app_data),
        refused: Arc::clone(&refused),
    };
    let mut session = match client::connect(config, (ssh.host.as_str(), ssh.port), handler).await {
        Ok(session) => session,
        // A refused key fails the handshake, and what russh reports for that says nothing about
        // the key — the reason the handler wrote down is the one worth showing.
        Err(e) => {
            let refused = refused.lock().unwrap().take();
            return Err(refused.unwrap_or_else(|| err!("error.sshConnectFailed", message = e)));
        }
    };

    let authenticated = match &ssh.auth {
        SshAuth::Password { password } => session
            .authenticate_password(&ssh.username, password)
            .await
            .map_err(|e| err!("error.sshAuthFailed", message = e))?,
        SshAuth::PrivateKey {
            key_path,
            passphrase,
        } => {
            let key_path = expand_home(key_path, std::env::home_dir().as_deref());
            let passphrase = passphrase.clone();
            /* Both halves belong off the runtime. Reading is disk; decoding an encrypted key is
            bcrypt-pbkdf, which is slow on purpose — a key written with OpenSSH's default rounds
            takes long enough to be felt, and every other command would wait behind it. */
            let key_pair = in_background(move || {
                let key_data = std::fs::read_to_string(&key_path)
                    .map_err(|e| err!("error.cannotReadPrivateKey", message = e))?;
                russh::keys::decode_secret_key(&key_data, passphrase.as_deref())
                    .map_err(|e| err!("error.invalidPrivateKey", message = e))
            })
            .await?;
            session
                .authenticate_publickey(
                    &ssh.username,
                    // `None` maps to the legacy `ssh-rsa` (SHA-1) signature for
                    // RSA keys, which most modern servers (OpenSSH >= 8.8)
                    // reject outright. Request SHA-256 instead; russh ignores
                    // this for non-RSA key types.
                    russh::keys::PrivateKeyWithHashAlg::new(
                        Arc::new(key_pair),
                        Some(russh::keys::HashAlg::Sha256),
                    ),
                )
                .await
                .map_err(|e| err!("error.sshAuthFailed", message = e))?
        }
    };

    match authenticated {
        russh::client::AuthResult::Success => Ok(session),
        russh::client::AuthResult::Failure {
            remaining_methods,
            partial_success,
        } => {
            // The most common cause of a "rejected" auth isn't a wrong
            // password, it's that the tried method isn't one the server
            // offers at all (e.g. server only allows keyboard-interactive
            // or publickey). Surfacing what it *does* accept saves a lot of
            // guessing.
            let accepted: Vec<&str> = remaining_methods.iter().map(<&str>::from).collect();
            Err(err!(
                "error.sshAuthRejected",
                partialSuccess = partial_success,
                methods = accepted.join(", "),
            ))
        }
    }
}

/// Authenticates against the SSH server without opening any port forward.
/// Used by the UI's "Test tunnel" action to validate credentials/connectivity
/// independently of the database connection itself.
pub async fn test_connection(ssh: &SshConfig, app_data: &Path) -> Result<(), AppError> {
    let session = authenticate(ssh, app_data).await?;
    let _ = session
        .disconnect(russh::Disconnect::ByApplication, "", "English")
        .await;
    Ok(())
}

/// Opens an SSH connection and a direct-tcpip channel to (remote_host, remote_port),
/// bridged to a freshly bound local TCP port. Returns the local port to connect to
/// instead of the real database host, plus the {@link Tunnel} keeping the bridge alive —
/// dropping that is what closes the forward again.
pub async fn open_tunnel(
    ssh: &SshConfig,
    remote_host: &str,
    remote_port: u16,
    app_data: &Path,
    notify: TunnelNotify,
) -> Result<(u16, Tunnel), AppError> {
    // The first authentication stands outside `session()`: it has to fail outwards for
    // `connect_db` to see, and there is no "reconnecting" to report when there has never been a
    // connection.
    let session = authenticate(ssh, app_data).await?;

    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| err!("error.cannotBindTunnelPort", message = e))?;
    let local_port = listener
        .local_addr()
        .map_err(|e| err!("error.cannotBindTunnelPort", message = e))?
        .port();

    let inner = Arc::new(TunnelInner {
        ssh: ssh.clone(),
        remote_host: remote_host.to_string(),
        remote_port,
        app_data: app_data.to_path_buf(),
        notify,
        session: AsyncMutex::new(SessionSlot {
            handle: Some(Arc::new(session)),
            opened_at: Some(Instant::now()),
            failed_at: None,
        }),
    });

    let accept: JoinHandle<()> = tokio::spawn({
        let inner = Arc::clone(&inner);
        async move {
            /* How many consecutive `accept` errors do not belong to a single connection. Counted to
            know when to speak up, and when to say again that things are fine. */
            let mut failures: u32 = 0;
            loop {
                let (local_stream, _) = match listener.accept().await {
                    Ok(pair) => {
                        // Accepting again after having complained means taking it back: the banner
                        // is saying the tunnel is broken, and it has just accepted a connection.
                        if failures >= ACCEPT_ALARM {
                            (inner.notify)(TunnelEvent::Reconnected);
                        }
                        failures = 0;
                        pair
                    }
                    // A single connection died mid-handshake. The next one still arrives, so no
                    // wait and no count.
                    Err(e) if is_transient_accept(&e) => continue,
                    Err(e) => {
                        failures += 1;
                        /* This used to `break` on the first one, and the tunnel died silently: the
                        watcher only looks at the SSH session, saw it still alive so no banner
                        showed, and every query after that just returned `connectionLost`. Speak up
                        exactly once, at exactly the `ACCEPT_ALARM`-th time. */
                        if failures == ACCEPT_ALARM {
                            (inner.notify)(TunnelEvent::Failed(err!(
                                "error.tunnelAcceptFailed",
                                message = e
                            )));
                        }
                        /* And keep trying, just as the watcher keeps trying: running out of file
                        descriptors passes, while leaving the loop here leaves nothing that can
                        reopen the port — it is only bound once, in `open_tunnel`. The loop lives
                        exactly as long as the `Tunnel`, whose `Drop` aborts this task. */
                        tokio::time::sleep(ACCEPT_RETRY).await;
                        continue;
                    }
                };
                // A DB connection pool (or multiple in-flight queries) can hold several physical
                // connections open at once, so each accepted local connection gets its own
                // bridging task instead of being handled inline — an inline loop would block
                // `accept()` for as long as that one connection stays open, starving every other
                // connection the pool tries to establish through this tunnel.
                let inner = Arc::clone(&inner);
                tokio::spawn(async move {
                    bridge_connection(&inner, local_stream).await;
                });
            }
        }
    });

    // Reopening only when someone knocks means the banner only shows after the user has clicked
    // something and waited. The watcher makes the tab heal itself: the computer wakes up, the
    // network comes back, and the banner has already switched to "reconnected" before the user
    // touches anything.
    let watch: JoinHandle<()> = tokio::spawn({
        let inner = Arc::clone(&inner);
        async move {
            let mut wait = WATCH_IDLE;
            loop {
                tokio::time::sleep(wait).await;
                let dead = {
                    let slot = inner.session.lock().await;
                    slot.handle.as_ref().is_none_or(|handle| handle.is_closed())
                };
                if !dead {
                    wait = WATCH_IDLE;
                    continue;
                }
                wait = next_backoff(wait, inner.session().await.is_ok());
            }
        }
    });

    Ok((
        local_port,
        Tunnel {
            inner,
            accept,
            watch,
        },
    ))
}

async fn bridge_connection(inner: &Arc<TunnelInner>, mut local_stream: tokio::net::TcpStream) {
    let channel = match open_channel(inner).await {
        Some(channel) => channel,
        None => {
            // The session looks alive but is not. Throw it away and try exactly once more with a
            // new session, before letting go of the local socket.
            inner.forget_session().await;
            match open_channel(inner).await {
                Some(channel) => channel,
                // Either the SSH server rejected/never answered the forward request (e.g. it
                // can't reach remote_host:remote_port itself, or AllowTcpForwarding/PermitOpen
                // blocks it). Drop this local connection so the DB client sees a closed socket
                // instead of hanging indefinitely.
                None => return,
            }
        }
    };

    // The same reasoning as the SSH socket's `nodelay`, for the hop between the app and whatever
    // the local end of the forward is: a driver's query has nothing following it, so there is
    // never anything to be gained by holding it back.
    let _ = local_stream.set_nodelay(true);

    // Both directions at once, rather than a `select!` that could only ever be carrying one of
    // them: a dump is one long download whose acknowledgements travel the other way, and a restore
    // is the same in reverse. Copying them in turn made each wait on the other.
    //
    // The copy also ends the way the hand-written loop did — one side reaching EOF shuts the other
    // side's write half down, which is the SSH channel's EOF, and the transfer in the other
    // direction is allowed to finish before the channel is dropped.
    let mut remote_stream = channel.into_stream();
    let _ = tokio::io::copy_bidirectional_with_sizes(
        &mut local_stream,
        &mut remote_stream,
        BRIDGE_BUFFER,
        BRIDGE_BUFFER,
    )
    .await;
}

/// One attempt to open a forwarding channel on the current session. `None` is a failure, without
/// saying why — the caller only has two choices, retry or let go.
async fn open_channel(inner: &Arc<TunnelInner>) -> Option<russh::Channel<client::Msg>> {
    let session = inner.session().await.ok()?;
    let opened = timeout(
        CHANNEL_OPEN_TIMEOUT,
        session.channel_open_direct_tcpip(
            inner.remote_host.clone(),
            inner.remote_port as u32,
            "127.0.0.1",
            0,
        ),
    )
    .await;
    match opened {
        Ok(Ok(channel)) => Some(channel),
        Ok(Err(_)) | Err(_) => None,
    }
}

/// A shell running on the server, plus the SSH session keeping it alive.
///
/// The session travels with the channel rather than staying in the function: `client::Handle` is
/// what runs russh's event loop, and dropping it makes the channel die within milliseconds. The
/// caller has to keep both alive for exactly as long, so this function hands over both at once.
pub struct RemoteShell {
    session: client::Handle<TunnelHandler>,
    read: russh::ChannelReadHalf,
    write: russh::ChannelWriteHalf<client::Msg>,
}

impl RemoteShell {
    /// Splits into two halves for two tasks: one reads, one writes. The SSH session stays with the
    /// write half — that is the half living exactly as long as the terminal session, while the read
    /// half ends as soon as the far end goes quiet.
    pub fn split(self) -> (russh::ChannelReadHalf, RemoteWriter) {
        (
            self.read,
            RemoteWriter {
                session: self.session,
                write: self.write,
            },
        )
    }
}

/// The write half of a shell session: typed bytes, resizes, and closing.
///
/// Also holds the `client::Handle`, because all three ways in and out of a session pass through
/// here — so dropping this closes the whole connection, and there is no way to leave an SSH session
/// open behind.
pub struct RemoteWriter {
    session: client::Handle<TunnelHandler>,
    write: russh::ChannelWriteHalf<client::Msg>,
}

impl RemoteWriter {
    /// The bytes the user types. A failure means the line is broken — the caller stops and does not
    /// retry.
    pub async fn write(&self, bytes: Vec<u8>) -> Result<(), AppError> {
        self.write
            .data_bytes(bytes)
            .await
            .map_err(|e| err!("error.sshShellFailed", message = e))
    }

    /// A resize frame. `pix_width`/`pix_height` are left at 0: the far end uses cols/rows, and a
    /// webview's pixel count says nothing about its character cells.
    pub async fn resize(&self, cols: u16, rows: u16) -> Result<(), AppError> {
        self.write
            .window_change(cols as u32, rows as u32, 0, 0)
            .await
            .map_err(|e| err!("error.sshShellFailed", message = e))
    }

    /// Closes cleanly: end of input, close the channel, then say goodbye to the server. Dropping
    /// `RemoteWriter` also closes it, but by dropping the handle without a word — and an sshd that
    /// keeps logs deserves to be told.
    pub async fn close(self) {
        let _ = self.write.eof().await;
        let _ = self.write.close().await;
        let _ = self
            .session
            .disconnect(russh::Disconnect::ByApplication, "", "English")
            .await;
    }
}

/// Opens a shell on the server: connect, authenticate, request a pty, request a shell.
///
/// Shares `authenticate()` with the tunnel — the same fingerprint check against
/// `known_hosts.json`, the same two ways of authenticating — but **the connection is its own**: a
/// terminal's lifetime is the tab's lifetime, while a tunnel's lifetime is a database connection's.
/// Sharing one would make closing a terminal tab drop the database connection.
pub async fn open_shell(
    ssh: &SshConfig,
    app_data: &Path,
    cols: u16,
    rows: u16,
) -> Result<RemoteShell, AppError> {
    let session = authenticate(ssh, app_data).await?;

    let channel = match timeout(CHANNEL_OPEN_TIMEOUT, session.channel_open_session()).await {
        Ok(Ok(channel)) => channel,
        Ok(Err(e)) => return Err(err!("error.sshShellFailed", message = e)),
        Err(_) => {
            return Err(err!(
                "error.sshTimeout",
                host = &ssh.host,
                port = ssh.port,
                seconds = CHANNEL_OPEN_TIMEOUT.as_secs(),
            ))
        }
    };

    /* `want_reply: true` for both: a server refusing to grant a pty has to say so, and its answer
    arrives as `ChannelMsg::Success`/`Failure` in the channel's queue. The reader ignores both —
    what it waits for is bytes — but a `Failure` always brings the channel closing with it, and the
    session ends right away instead of hanging on a mute terminal. */
    channel
        .request_pty(true, "xterm-256color", cols as u32, rows as u32, 0, 0, &[])
        .await
        .map_err(|e| err!("error.sshShellFailed", message = e))?;
    channel
        .request_shell(true)
        .await
        .map_err(|e| err!("error.sshShellFailed", message = e))?;

    let (read, write) = channel.split();
    Ok(RemoteShell {
        session,
        read,
        write,
    })
}

#[cfg(test)]
mod expand_home_tests {
    use super::expand_home;
    use std::path::{Path, PathBuf};

    #[test]
    fn a_path_under_home_is_read_from_this_machines_home() {
        let home = Path::new("/home/me");
        assert_eq!(
            expand_home("~/.ssh/id_rsa", Some(home)),
            home.join(".ssh/id_rsa")
        );
        assert_eq!(
            expand_home("~\\.ssh\\id_rsa", Some(home)),
            home.join(".ssh\\id_rsa")
        );
    }

    #[test]
    fn any_other_path_is_left_as_it_was_written() {
        let home = Some(Path::new("/home/me"));
        assert_eq!(
            expand_home("/opt/keys/id", home),
            PathBuf::from("/opt/keys/id")
        );
        assert_eq!(expand_home("~other/id", home), PathBuf::from("~other/id"));
        assert_eq!(expand_home("~/id", None), PathBuf::from("~/id"));
    }
}

#[cfg(test)]
mod tests {
    use super::verify_host;
    use super::{
        is_transient_accept, known_hosts_file, load_known_hosts, next_backoff, remember_host,
    };
    use super::{ACCEPT_ALARM, ACCEPT_RETRY, WATCH_IDLE, WATCH_MAX, WATCH_MIN};
    use std::io::{Error, ErrorKind};
    use std::time::Duration;

    /// What the handler reads and writes between connections. The handshake around it needs a real
    /// SSH server to exercise; this is the part that decides whether a key is the one seen before.
    #[test]
    fn a_remembered_host_is_read_back_and_can_be_replaced() {
        let dir = std::env::temp_dir().join(format!("mixlab-test-{}", uuid::Uuid::new_v4()));
        let file = known_hosts_file(&dir);

        // Nothing remembered yet — a first connection has nothing to check against.
        assert!(load_known_hosts(&file).is_empty());

        remember_host(&file, "db.example:22", "SHA256:aaa").unwrap();
        remember_host(&file, "other.example:2222", "SHA256:bbb").unwrap();
        let known = load_known_hosts(&file);
        assert_eq!(
            known.get("db.example:22").map(String::as_str),
            Some("SHA256:aaa")
        );
        assert_eq!(known.len(), 2);

        // Each host stands on its own: accepting a rebuilt server's new key leaves the others be.
        remember_host(&file, "db.example:22", "SHA256:ccc").unwrap();
        let known = load_known_hosts(&file);
        assert_eq!(
            known.get("db.example:22").map(String::as_str),
            Some("SHA256:ccc")
        );
        assert_eq!(
            known.get("other.example:2222").map(String::as_str),
            Some("SHA256:bbb")
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Two first connections at once. Without a lock around the read-modify-write both threads
    /// read the same file, both add their own host, and whichever writes last is the only one
    /// remembered — so the other server is a stranger again next time, and TOFU accepts whatever
    /// answers for it.
    #[test]
    fn hosts_remembered_at_the_same_time_all_survive() {
        let dir = std::env::temp_dir().join(format!("mixlab-test-{}", uuid::Uuid::new_v4()));
        let file = known_hosts_file(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        std::thread::scope(|scope| {
            for n in 0..8 {
                let file = &file;
                scope.spawn(move || {
                    for round in 0..8 {
                        let endpoint = format!("host{n}.example:22");
                        remember_host(file, &endpoint, &format!("SHA256:{n}-{round}")).unwrap();
                    }
                });
            }
        });

        let known = load_known_hosts(&file);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(known.len(), 8, "an entry was lost to a concurrent write");
    }

    /// The three answers TOFU has, from the one place that decides them.
    #[test]
    fn a_changed_key_is_refused_and_the_known_one_is_not() {
        let dir = std::env::temp_dir().join(format!("mixlab-test-{}", uuid::Uuid::new_v4()));
        let file = known_hosts_file(&dir);

        // Never seen: accepted, and written down.
        verify_host(&file, "db.example:22", "SHA256:aaa").unwrap();
        assert_eq!(
            load_known_hosts(&file)
                .get("db.example:22")
                .map(String::as_str),
            Some("SHA256:aaa"),
        );

        // Seen, same key: accepted again.
        verify_host(&file, "db.example:22", "SHA256:aaa").unwrap();

        // Seen, different key: refused, and the fingerprint on file is left alone — accepting the
        // new one is the user's decision, taken by deleting the entry, not ours.
        assert!(verify_host(&file, "db.example:22", "SHA256:zzz").is_err());
        assert_eq!(
            load_known_hosts(&file)
                .get("db.example:22")
                .map(String::as_str),
            Some("SHA256:aaa"),
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A file that has been corrupted (or written by a future version) reads as "nothing known"
    /// rather than failing every SSH connection the app makes.
    #[test]
    fn an_unreadable_store_is_treated_as_empty() {
        let dir = std::env::temp_dir().join(format!("mixlab-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = known_hosts_file(&dir);
        std::fs::write(&file, "not json").unwrap();

        let known = load_known_hosts(&file);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(known.is_empty());
    }

    /// The watcher's interval. Success goes back to the idle interval; the first failure drops to
    /// the fastest interval, then doubles gradually up to the cap — and stays at the cap instead of
    /// returning to the fast interval.
    #[test]
    fn the_watcher_backs_off_while_the_tunnel_stays_down() {
        // While things are fine every interval is WATCH_IDLE, whatever interval it just failed at.
        assert_eq!(next_backoff(WATCH_IDLE, true), WATCH_IDLE);
        assert_eq!(next_backoff(WATCH_MIN, true), WATCH_IDLE);
        assert_eq!(next_backoff(WATCH_MAX, true), WATCH_IDLE);

        // The first failure — the current interval is the idle one — retries quickly.
        assert_eq!(next_backoff(WATCH_IDLE, false), WATCH_MIN);

        // Then doubles.
        assert_eq!(next_backoff(WATCH_MIN, false), Duration::from_secs(10));
        assert_eq!(
            next_backoff(Duration::from_secs(10), false),
            Duration::from_secs(20)
        );

        // Hitting the cap stops at the cap, neither exceeding it nor going back to WATCH_MIN.
        assert_eq!(next_backoff(Duration::from_secs(40), false), WATCH_MAX);
        assert_eq!(next_backoff(WATCH_MAX, false), WATCH_MAX);
    }

    /// What the accept loop above decides: which errors belong to a single connection, and which to
    /// the port.
    ///
    /// Checked with the operating system's own error codes rather than hand-written `ErrorKind`s,
    /// because what is being asserted is that *Windows' and Unix's codes land in exactly the
    /// `ErrorKind`s the loop catches* — the part easiest to get wrong and the part that cannot be
    /// read off the code.
    #[test]
    fn an_aborted_connection_is_not_a_broken_listener() {
        // Raw codes are translated by the *running* operating system, so each half only runs at
        // home — CI runs both. A pool dropping half-open sockets produces exactly these codes.
        #[cfg(windows)]
        {
            assert!(is_transient_accept(&Error::from_raw_os_error(10054))); // WSAECONNRESET
            assert!(is_transient_accept(&Error::from_raw_os_error(10053))); // WSAECONNABORTED
                                                                            // Running out of
                                                                            // handles is not about
                                                                            // one connection: wait,
                                                                            // then retry.
            assert!(!is_transient_accept(&Error::from_raw_os_error(10024))); // WSAEMFILE
        }
        // Unix has no shared number table: Linux numbers `ECONNRESET` 104, macOS 54 — so take them
        // from the `libc` of the operating system being built for, not hand-written numbers.
        #[cfg(unix)]
        {
            assert!(is_transient_accept(&Error::from_raw_os_error(
                libc::ECONNRESET
            )));
            assert!(is_transient_accept(&Error::from_raw_os_error(
                libc::ECONNABORTED
            )));
            assert!(!is_transient_accept(&Error::from_raw_os_error(
                libc::EMFILE
            )));
        }

        // And a signal interrupting the call, everywhere.
        assert!(is_transient_accept(&Error::from(ErrorKind::Interrupted)));

        // Two seconds of a silent port is the threshold for bothering the user: long enough for a
        // passing blip to clear up silently.
        assert_eq!(ACCEPT_RETRY * ACCEPT_ALARM, Duration::from_secs(2));
    }
}
