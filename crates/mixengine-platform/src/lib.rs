//! Everything the operating system does differently.
//!
//! Core, supervisor, daemon and the clients contain **zero** `#[cfg(target_os = …)]`; all of it
//! lives here behind traits, with `windows/`, `macos/`, `linux/` implementations and an in-memory
//! `mock/` one that is always compiled and used by tests and `--dry-run`.
//!
//! See `docs/architecture/platform-abstraction.md` for the trait list and the rules every
//! implementation follows (reversible and tagged mutations, atomic read-modify-write, `probe()`
//! before acting, [`Error::UnsupportedPlatform`] instead of `unimplemented!()`).
//!
//! **The crate is featured, and `default` is everything.** Every dependent but one takes `default`
//! and is unaffected. `mixengine-elevate` takes `default-features = false, features = ["elevated"]`
//! because it runs as root: `tokio`, `keyring` and `directories` have no business in that binary,
//! and CI diffs its dependency closure against a committed list rather than trusting that they stay
//! out. See the T40 design, D8.

#![warn(missing_docs)]

#[cfg(feature = "host")]
use std::sync::Arc;

// Documented by its own `//!` header. An outer `///` here as well would put the module's
// intra-doc links into *this* module's scope, where `owner_of` is not a name — measured, not
// reasoned about: `cargo doc` refused it.
#[cfg(feature = "elevated")]
pub mod elevated;
// What a crash report needs from the running executable: its base, its range, its build id and
// the calling thread's return addresses — T91a. The daemon's alone, so `host`.
#[cfg(feature = "host")]
pub mod crash_image;
// Documented by its own `//!` header. Under both features: the daemon reads the block and the
// helper writes it, and neither is worth a second implementation.
/// The listener an activator holds, so a stopped service can be started by the connection
/// that needed it — roadmap task T70. Beside `ipc` because it is the same shape and emphatically
/// not the same thing: see the module note.
#[cfg(feature = "ipc")]
pub mod activation;
// Documented by its own `//!` header. `host` only: the helper never chooses a home.
#[cfg(feature = "host")]
pub mod home;
#[cfg(any(feature = "host", feature = "elevated"))]
pub mod hosts;
// Documented by its own `//!` header. Under both features because it is the one question both
// sides of the privilege boundary ask: the daemon asks where the installed helper is so it can run
// it, and the helper asks the same function where to put itself — T85.
#[cfg(any(feature = "host", feature = "elevated"))]
pub mod install;
// What a package database says owns a file, as text — roadmap task **T88e**. Compiled on all
// three systems so its tests run on every one; the call itself is in `sys::install`.
#[cfg(feature = "ipc")]
pub mod ipc;
pub mod lock;
#[cfg(any(feature = "host", feature = "elevated"))]
mod packages;
// Documented by its own `//!` header. Under both features for `hosts`' reason, and now for
// `port_access`' as well: the daemon reads a managed block and the helper writes it.
#[cfg(any(feature = "host", feature = "elevated"))]
pub mod markers;
// Documented by its own `//!` header. `host` only, and not public: what a process group costs is
// asked through [`Host::process_metrics`], and `mixengine-elevate` has no business enumerating this
// machine's processes.
#[cfg(feature = "host")]
pub(crate) mod metrics;
#[cfg(feature = "host")]
pub mod mock;
// Documented by its own `//!` header. `host` only, on `metrics`' reasoning: the daemon asks it
// before an uninstall, and `mixengine-elevate` has no business enumerating processes (T182).
#[cfg(feature = "host")]
pub mod occupants;
pub mod paths;
// Documented by its own `//!` header. Under both features for `hosts`' reason.
#[cfg(feature = "handover")]
pub mod handover;
#[cfg(any(feature = "host", feature = "elevated"))]
pub mod port_access;
#[cfg(feature = "process")]
pub mod process;
// Documented by its own `//!` header. Under both features for `hosts`' reason: the daemon reads
// which TLDs this machine routes here and the helper writes them.
#[cfg(any(feature = "host", feature = "elevated"))]
pub mod resolver;
// Documented by its own `//!` header. Under both features for `hosts`' reason: the daemon reads
// whether this machine already trusts the authority and the helper is what makes it.
#[cfg(any(feature = "host", feature = "elevated"))]
pub mod trust;
// The databases Firefox and Chrome read *instead of* that store — T49b. `host` only, and that is
// the line T49 was split on: the system stores need root and ride in the first-run elevation batch,
// while these belong to the user and are never asked of the helper.
#[cfg(feature = "host")]
pub mod browsers;
// Documented by its own `//!` header. Under both features for `hosts`' reason — except that only
// the helper ever calls the write: the daemon has no firewall trait, because it never reads a rule
// set back. T74.
#[cfg(any(feature = "host", feature = "elevated"))]
pub mod firewall;
// Which networks this machine could share a site on — T74. One implementation for all three
// systems rather than a per-OS module, because the crate behind it already is per-OS; see the
// module. `host` only: `mixengine-elevate` opens the firewall rule and never asks what interfaces
// exist, and its dependency closure is a security decision CI diffs.
#[cfg(feature = "host")]
mod network;

// The `netsh` output parser, compiled on all three systems so its tests run on every one of them —
// `resolver`'s reasoning, one capability along. The call itself is in `sys::reserved`.
#[cfg(feature = "host")]
mod reserved;
// Each launcher's table, compiled on all three systems so that each is tested on every one of them.
// The calls themselves are in `sys::prompt`.
#[cfg(feature = "host")]
mod prompt;
// Writing a file only this account may read — the primitive the CA private key needs and that
// `DirectoryAccess` cannot be pointed at, because that trait is about directories and one of its
// grants is directory-only. See the module.
#[cfg(feature = "host")]
mod private_file;
// The one capability whose implementation is not per-OS, because the crate behind it already is.
// What *is* per-OS is one reading of one error, in `sys::secrets` — see the module's own
// documentation for why that is a different split rather than a hole in this one.
#[cfg(feature = "host")]
mod secrets;
// Finding this install's window and starting it, shared by all three systems — the T83 design's D9
// and D11, and T165.
#[cfg(feature = "host")]
pub(crate) mod desktop;
#[cfg(feature = "signal")]
pub mod signal;
// Documented by its own `//!` header. Under no feature: it is `std::fs` only, and `mix` reads it
// as well as the daemon (T182).
pub mod tombstone;
#[cfg(feature = "host")]
mod traits;
// Documented by its own `//!` header: a runtime's bindir, heard changing (T185b). `host` only,
// since `mixengine-elevate` never waits on a directory.
#[cfg(feature = "host")]
pub mod watch;
// Documented by its own `//!` header: the MixLab window's own folders, which an uninstall removes
// with the home (T182b). Not a `Host` capability, because nothing here changes the machine.
#[cfg(feature = "host")]
pub mod window_data;

// Shared by `linux/` and `macos/`, which both name what they take from it.
#[cfg(unix)]
mod unix;

// The one thing `secrets` publishes outside this crate: everything else in it is reached through
// the `Keyring` trait a `Host` hands out, and a random string has no host to belong to.
#[cfg(feature = "host")]
pub use secrets::generate_secret;
// The other thing this crate publishes that belongs to no `Host` capability, for the same reason:
// a private key has no host, and a trait method would come with a mock that could say it restricted
// something without restricting it.
#[cfg(all(windows, feature = "host"))]
pub use private_file::is_private_file;
#[cfg(feature = "host")]
pub use private_file::write_private;
#[cfg(feature = "host")]
pub use traits::{
    APP_CONTROL_REFUSAL, APPLICATION_CONTROL_BLOCKED, AppControl, AppControlState,
    AutostartMechanism, AutostartPlan, AutostartState, BrowserChange, BrowserSurvey, BrowserTrust,
    ConnectionCount, DatabaseState, DesktopApps, DirectoryAccess, Elevation, ElevationSupport,
    Enforcement, FirewallRules, GroupReading, GroupRoot, HomeDirs, Host, HostsFile, InstalledApp,
    Installers, Interface, KEYRING_SERVICE, Keyring, LimitMechanism, LimitSupport, Located,
    Machine, MachineFacts, MemoryMeasure, NetworkInfo, OrphanGuarantee, PathIntegration,
    PathLocation, PathState, PortAccess, PortAccessMethod, PortAccessState, PortBinding,
    PortHolder, PortOwner, PortRange, Probe, ProcessMetrics, Raised, RedistributableOutcome,
    Redistributables, ReservedPorts, ResolverConfig, ResolverMethod, ResolverState,
    ResourceControl, ServiceInstaller, Started, TrustState, TrustStore, TrustStoreMethod,
    VISUAL_CPP_PUBLISHER, VisualCppVersion, WhenExceeded, avx, choose_interface, dotted_version,
    names_the_redistributable, orphan_guarantee, pkgid_of, refused_by_app_control,
    shared_libraries, visual_cpp_from_registry,
};

// The three supported operating systems keep their own directory, exactly as the architecture
// document describes them; `#[path]` maps whichever one applies onto a single `sys` name so the
// rest of the crate never spells out a target.
#[cfg(target_os = "linux")]
#[path = "linux/mod.rs"]
mod sys;
#[cfg(target_os = "macos")]
#[path = "macos/mod.rs"]
mod sys;
#[cfg(windows)]
#[path = "windows/mod.rs"]
mod sys;

// A new OS gets a directory of its own and an entry above. Failing at compile time is the point:
// silently falling back to "Linux, probably" would put a user's data somewhere no uninstaller
// knows about.
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
compile_error!(
    "MixEngine supports Windows, macOS and Linux. Porting means adding a directory next to \
     src/linux/ and an implementation of every trait in src/traits/."
);

/// Whether this binary came out of the packaging pipeline.
///
/// **`packaging/stage.sh` sets `MIXENGINE_RELEASE`, and so does the CI step that builds the
/// desktop window for the same release** (`Build the window` in `.github/workflows/_build.yml`, since phase 11); nothing
/// else in this repository does, so this is false for every `cargo build`, `cargo run`, `cargo
/// test` and `tauri dev` — which is the point. A
/// working tree carries migrations that have not shipped, and a daemon built from one will migrate
/// whatever database it opens; if that is the home a person keeps real projects in, their data ends
/// up on a schema no release can read, and editing that migration afterwards — normal, while it is
/// unreleased — leaves it openable by nothing. See
/// `docs/decisions/0024-a-build-that-is-not-a-release-keeps-its-own-home.md`.
///
/// **Provenance and not optimisation.** `cfg!(debug_assertions)` would need no packaging change and
/// would answer this for `cargo run` and `cargo test`, but it calls a developer's
/// `cargo build --release` a release — and that is the build somebody points at real work to see
/// how it behaves.
///
/// `option_env!` is tracked by cargo's fingerprint, so changing the variable rebuilds rather than
/// serving a cached answer. Measured both ways, in a crate used as a dependency, which is how this
/// one is used.
pub const RELEASE: bool = option_env!("MIXENGINE_RELEASE").is_some();

/// Where a [`Host`]'s [`keyring`](Host::keyring) keeps credentials — roadmap task **T184**.
///
/// [`Os`](Self::Os) is the machine's own store and what every release uses. [`File`](Self::File)
/// is one private file, which is what `mixengined` uses when it is not a release (ADR 0052): an
/// unsigned development build is a stranger to the Keychain after every rebuild, and a file in its
/// own home never has to ask anybody.
#[cfg(feature = "host")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Credentials {
    /// Credential Manager, the login Keychain, or the Secret Service.
    Os,
    /// This file, created on first write.
    File(std::path::PathBuf),
}

/// The machine this process is running on.
///
/// Constructed once at startup and passed down as `Arc<dyn Host>`; tests inject
/// [`mock::Host`] instead and assert on what it recorded. Its keyring is the OS store — see
/// [`host_with`] for the one caller that chooses otherwise.
#[cfg(feature = "host")]
#[must_use]
pub fn host() -> Arc<dyn Host> {
    host_with(Credentials::Os)
}

/// [`host`], with its keyring kept in `credentials`.
#[cfg(feature = "host")]
#[must_use]
pub fn host_with(credentials: Credentials) -> Arc<dyn Host> {
    Arc::new(sys::Host::with_credentials(credentials))
}

/// [`sys::replace::atomically`], for the integration suite.
///
/// The engine itself is `pub(crate)`: only `hosts` and `port_access` may replace a system file, and
/// a public entry point would be an invitation to a third caller answering to neither. The suite
/// drives it against a file it owns, which is the one thing a unit test inside either module cannot
/// do for both of them at once.
///
/// # Errors
///
/// Whatever the replace itself refuses with.
#[cfg(feature = "elevated")]
#[doc(hidden)]
pub fn replace_for_tests(path: &std::path::Path, contents: &str) -> Result<()> {
    sys::replace::atomically(path, contents)
}

/// Failure of a platform operation.
///
/// Library-local on purpose: the conversion into the wire error happens at the daemon boundary, so
/// this enum can describe OS specifics without the API having to know about them.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The capability genuinely does not exist on this OS, or the machine is not configured for it.
    ///
    /// This is a normal answer, not a bug — `reason` is shown to the user and should describe the
    /// manual workaround where one exists.
    #[error("{capability} is not available on this platform: {reason}")]
    UnsupportedPlatform {
        /// The capability that was asked for, e.g. `"PortAccess"`.
        capability: &'static str,
        /// Why it is unavailable, phrased for a user rather than a developer.
        reason: String,
    },

    /// The OS would not say where the current user's data belongs.
    ///
    /// In practice this means the environment is missing what the platform considers mandatory
    /// (`%LOCALAPPDATA%`, `$HOME`), which happens to service accounts and to stripped-down
    /// containers. Setting `MIXENGINE_HOME` is the way out, so the message says so.
    #[error(
        "cannot determine the user's data directory ({reason}) — set MIXENGINE_HOME to choose one \
         explicitly"
    )]
    NoHomeDirectory {
        /// What was missing, phrased for a user rather than a developer.
        reason: &'static str,
    },

    /// A file or directory the OS was asked about could not be touched.
    ///
    /// Shaped like `mixengine_core::Error::Io` on purpose: the path belongs in the message because
    /// "access denied" on its own names nothing, and the OS error stays the `#[source]` so a
    /// message never prints its own cause twice.
    #[error("cannot {action} {}", path.display())]
    Io {
        /// What was being attempted, e.g. `"restrict"`.
        action: &'static str,
        /// The path it was attempted on.
        path: std::path::PathBuf,
        /// The underlying OS error.
        #[source]
        source: std::io::Error,
    },

    /// The OS credential store was there and would not do it.
    ///
    /// Names the entry and never the value — a credential must not reach a log through an error
    /// message, which is the accident this whole capability exists to prevent. The store's own
    /// complaint is the `#[source]`, boxed rather than typed: what the backend crate calls its
    /// failures is not vocabulary the daemon should be matching on, and the one distinction that
    /// *is* actionable — no store on this machine at all — is already
    /// [`Error::UnsupportedPlatform`] by the time it gets here.
    #[error("cannot {action} the credential {service}/{key} in the credential store")]
    Secret {
        /// What was being attempted: `"read"`, `"store"`, `"forget"`, `"address"`.
        action: &'static str,
        /// The namespace the credential is filed under.
        service: String,
        /// The account within it.
        key: String,
        /// The store's own complaint.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// An operating-system call failed, and there is no path to name in the message.
    ///
    /// [`Error::Io`]'s sibling for the calls that are about something other than a file: reading
    /// this account's SID out of the process token, impersonating whoever is at the other end of a
    /// pipe. `action` completes the sentence "cannot …" and the OS's own message is appended as the
    /// cause, the same way it is there.
    #[error("cannot {action}")]
    Os {
        /// What was being attempted, e.g. `"identify this account"`.
        action: &'static str,
        /// The underlying OS error, generally built from `GetLastError`.
        #[source]
        source: std::io::Error,
    },

    /// A file MixEngine was about to run did not prove what it is — roadmap task **T150**.
    ///
    /// Its own variant, because nothing about it is an I/O failure: the file was read, and what it
    /// said about itself was not good enough to run it with administrator rights.
    #[error("{} was not run: {reason}", path.display())]
    NotTrusted {
        /// The file that was refused.
        path: std::path::PathBuf,
        /// Which check refused it, phrased for a person.
        reason: String,
    },

    /// The address of the local endpoint is one this OS will not accept.
    ///
    /// Not an I/O failure — nothing was attempted. The address is computed from `MIXENGINE_HOME`,
    /// so `reason` has to name the constraint the home broke, which is the only thing the user can
    /// act on.
    #[error("{address} cannot be used as a local endpoint: {reason}")]
    Address {
        /// The address that was rejected, rendered the way the OS names one.
        address: String,
        /// Which rule it broke, phrased for a user rather than a developer.
        reason: String,
    },

    /// MixEngine's block in the hosts file cannot be edited without guessing at what it means.
    ///
    /// Its own variant rather than an [`Error::Io`] because it is the caller's answer as well as the
    /// user's: the helper turns it into `Refused`, which says the same request will be refused
    /// again — correct here, since what is wrong is on the machine and a person has to look at it.
    /// No path: there is one hosts file per machine, and the message says so.
    #[error("{reason}")]
    MalformedBlock {
        /// Which rule the block broke, phrased for a person about to open the file.
        reason: String,
    },

    /// Something is already listening at the local endpoint.
    ///
    /// Its own variant rather than an [`Error::Io`] carrying `AddrInUse`, because it is the normal
    /// answer to "is a daemon already running for this home?" — the question the single-instance
    /// check asks (roadmap task T9), and one whose answer is not a failure at all.
    #[error("another process is already listening on {address}")]
    EndpointInUse {
        /// The endpoint that is taken.
        address: String,
    },

    /// Something holds the local endpoint, and it is not this account's daemon.
    ///
    /// The opposite of [`Error::EndpointInUse`] in what it asks of the reader: that one says "a
    /// daemon of yours is already up, stop it", and this one says "somebody else is on the name and
    /// there is no daemon of yours to go looking for". Both callers reach it — a client that dialled
    /// and hung up before its first byte, and a daemon whose `bind` was refused because the name was
    /// taken. Nothing is ever written to the connection that produced it.
    ///
    /// Windows only in practice. The pipe namespace is flat and machine-wide, so any account can
    /// create the name a client is about to dial; a Unix socket lives inside a `run/` directory
    /// this account owns, and another account cannot put a file there to be dialled instead.
    #[error("{address} is held by {account}, not by this account")]
    EndpointNotOurs {
        /// The endpoint that was dialled, rendered the way the OS names one.
        address: String,
        /// Who is serving it, in the OS's own identifier for an account.
        account: String,
    },

    /// No interface on this machine can be shared on, or more than one can and nothing said which.
    ///
    /// Its own variant rather than an [`Error::UnsupportedPlatform`]: every platform supports this,
    /// and what is wrong is the machine's situation at this moment — a laptop with the Wi-Fi off, or
    /// one plugged in as well as associated. `reason` carries the candidate list, because that list
    /// is the remedy the user acts on.
    #[error("{reason}")]
    NoInterface {
        /// What was wrong and what could be typed instead, phrased for a user.
        reason: String,
    },

    /// A command the platform layer shells out to failed.
    ///
    /// Kept distinct from [`Error::Io`]: the binary ran and said no, which is a different problem
    /// from not being able to run it, and its own diagnostics are the only ones worth showing.
    #[error("{command} failed{} ({status}){}", about(path.as_deref()), said(output))]
    Command {
        /// The program that was run, e.g. `"icacls"`.
        command: &'static str,
        /// The path it was run against, when it was run against one. `None` for a tool that was
        /// asked about the machine rather than about a file.
        path: Option<std::path::PathBuf>,
        /// How it exited, rendered for a human.
        status: String,
        /// Whatever it wrote to stderr, trimmed. Empty when it said nothing.
        output: String,
    },
}

/// ` for <path>`, when there is one.
fn about(path: Option<&std::path::Path>) -> String {
    path.map_or_else(String::new, |path| format!(" for {}", path.display()))
}

/// The tool's own complaint, when it made one. A tool that fails silently should not leave a
/// dangling colon behind in the message.
fn said(output: &str) -> String {
    if output.is_empty() {
        String::new()
    } else {
        format!(": {output}")
    }
}

/// Result of a platform operation.
pub type Result<T> = std::result::Result<T, Error>;
