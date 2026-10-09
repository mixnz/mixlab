//! Where everything lives inside `MIXENGINE_HOME`.
//!
//! The layout is identical on all three operating systems — only the root differs, and choosing it
//! is the platform layer's job ([`mixengine_platform::HomeDirs`]). Nothing outside this root is
//! ever written except the handful of system files listed in
//! `docs/architecture/overview.md`, all of them through `mixengine-elevate` — and the
//! directories the user themselves moved with `[paths]`, which are still MixEngine's to remove.

use std::path::{Path, PathBuf};

use mixengine_platform::Host;
use mixengine_proto::ServiceId;

use crate::config::{FILE_NAME as CONFIG_FILE_NAME, PathOverrides};
use crate::{Error, Result};

/// The SQLite database, directly under the root: the single source of truth.
pub const DATABASE_FILE_NAME: &str = "mixengine.db";

/// The single-instance lock, inside `run/`.
///
/// Held open for as long as the daemon runs; its contents are the holder's pid, and its *existence*
/// means nothing — see [`mixengine_platform::lock`].
pub const LOCK_FILE_NAME: &str = "mixengined.lock";

/// A development build's credential store, directly under the root — T184, ADR 0052. A release
/// never writes it.
pub const CREDENTIALS_FILE_NAME: &str = "credentials.json";

/// The daemon's own log, inside `logs/`.
///
/// Rotated copies sit next to it as `daemon.log.1` … `daemon.log.5`; the daemon owns that naming
/// because it is the only process that writes the file. Service logs are somewhere else entirely
/// — see [`Paths::service_logs`] — because these are `tracing` output, not a program's stdout.
pub const DAEMON_LOG_FILE_NAME: &str = "daemon.log";

/// Where the per-service log directories live, inside `logs/`.
///
/// A directory of its own rather than files beside `daemon.log`, so that a service id can never
/// collide with the daemon's own file and so that everything one service ever wrote — the live file
/// and its rotated copies — can be removed by removing one directory.
const SERVICES_LOG_DIR_NAME: &str = "services";

/// Where a crash report is written, inside `logs/` — roadmap task **T91**.
const CRASHES_DIR_NAME: &str = "crashes";

/// Decide which directory is `MIXENGINE_HOME`.
///
/// `override_` comes from the environment or the command line and wins outright; without one the
/// platform decides. Either way the result is made absolute — the daemon outlives any particular
/// working directory, and a relative root would quietly follow it around.
///
/// The directory is not created and need not exist yet, so this stops short of `canonicalize`,
/// which would both require existence and hand back a `\\?\` path on Windows. What it does do is
/// [`mixengine_platform::paths::in_full`], which is the same answer spelled the way the
/// filesystem spells it — a home reached through an 8.3 alias is a home nginx refuses every
/// file in.
///
/// # Errors
///
/// [`Error::EmptyHome`] when the override is an empty string, [`Error::Platform`] when there is no
/// override and the OS cannot say where user data belongs, and [`Error::Io`] if the path cannot be
/// made absolute.
pub fn resolve_root(override_: Option<&Path>, host: &dyn Host) -> Result<PathBuf> {
    resolve_root_with(override_, || Ok(host.home_dirs().default_home()?))
}

/// [`resolve_root`] with no [`Host`]: the platform default comes from
/// [`mixengine_platform::home::default_home`], which is the same answer.
///
/// **The shim's.** A `Host` keeps every capability's DLLs in the import table of the binary that
/// builds one, and the shim runs in front of every `php` a person types — see the note on that
/// function. Everything else keeps [`resolve_root`], which a test can hand a mock.
///
/// # Errors
///
/// As [`resolve_root`].
pub fn resolve_root_default(override_: Option<&Path>) -> Result<PathBuf> {
    resolve_root_with(override_, || Ok(mixengine_platform::home::default_home()?))
}

/// The one body both spellings share, given how to find the platform default.
fn resolve_root_with(
    override_: Option<&Path>,
    default: impl FnOnce() -> Result<PathBuf>,
) -> Result<PathBuf> {
    let root = match override_ {
        // A guard at the library boundary, not the daemon's first line of defence: `clap` refuses
        // an empty `--home` and an empty `MIXENGINE_HOME` before either reaches this function, so
        // `mixengined` never gets here. `resolve_root` is public and `core` cannot assume its
        // caller is a `clap` binary — and the one thing that must never happen is treating an
        // empty override as "not given", which would point a sandbox run at the real install.
        Some(path) if path.as_os_str().is_empty() => return Err(Error::EmptyHome),
        Some(path) => path.to_path_buf(),
        None => default()?,
    };

    let absolute = std::path::absolute(&root).map_err(|source| Error::Io {
        action: "resolve",
        path: root,
        source,
    })?;

    Ok(mixengine_platform::paths::in_full(&absolute))
}

/// Create `path` and every missing parent.
///
/// # Errors
///
/// [`Error::Io`], with the path in the message, when the directory cannot be created — including
/// the case where something that is not a directory is already sitting there.
pub fn create_dir(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path).map_err(|source| Error::Io {
        action: "create directory",
        path: path.to_path_buf(),
        source,
    })
}

/// Whether a folder's metadata read says it is gone — roadmap task **T206d**, D1.
///
/// **Only `NotFound`.** No access, a drive not mounted, a timeout: each is a folder out of reach, and
/// nothing here may treat one as deleted. A pure function of the read so every `ErrorKind` can be
/// tested, which no real path does the same way on three systems.
#[must_use]
pub fn gone(read: &std::io::Result<std::fs::Metadata>) -> bool {
    matches!(read, Err(error) if error.kind() == std::io::ErrorKind::NotFound)
}

/// [`gone`] for a path, read without following a final link.
pub async fn is_gone(path: &Path) -> bool {
    gone(&tokio::fs::symlink_metadata(path).await)
}

/// A relative path written with `/`, joined onto `base` one part at a time — roadmap task **T191**.
///
/// A manifest's `provides` value, a site's doc root and a route's root are all stored with `/`,
/// because they are written once for every system or rendered into a web server's configuration.
/// `Path::join` would keep that `/` inside a Windows path (`…\8.3.33\bin/php`), which is the
/// spelling a person then reads in an error or a log. Splitting on both separators also accepts a
/// value that already arrived native; on Unix the two are one character.
#[must_use]
pub fn join_stored(base: &Path, relative: &str) -> PathBuf {
    relative
        .split(['/', std::path::MAIN_SEPARATOR])
        .filter(|part| !part.is_empty())
        .fold(base.to_path_buf(), |path, part| path.join(part))
}

/// `path` rebuilt from its components, so every separator is this system's — roadmap task **T191**.
///
/// For a path a person wrote by hand, such as a `[paths]` value the configuration template tells
/// them to write with `/`. A trailing separator and `.` parts go; `..` stays, and so does a UNC
/// prefix, which `components` keeps whole.
#[must_use]
pub fn native(path: &Path) -> PathBuf {
    path.components().collect()
}

/// A relative path stored with `/`, spelled for a person on this system — roadmap task **T191**.
///
/// The wire form of a site's doc root and a static route's root. The column keeps `/`; the input
/// side (`sites::relative_doc_root`) accepts either spelling, so what a client reads it can send
/// back unchanged.
#[must_use]
pub fn native_relative(stored: &str) -> String {
    stored
        .split('/')
        .collect::<Vec<_>>()
        .join(std::path::MAIN_SEPARATOR_STR)
}

/// Remove a directory tree that may not be there.
///
/// **A directory that is already gone is the answer this wants**, which is what makes an uninstall
/// resumable: one interrupted by a daemon restart has to be able to finish rather than refuse. Two
/// callers share it since roadmap task **T82a** — an extension's own directories and the generated
/// `etc/` and `logs/` of the pool that serves it — because two copies of "not found is fine" is one
/// copy that eventually is not.
///
/// # Errors
///
/// [`Error::Io`], with the path in the message, for anything that is not "it was not there".
pub(crate) async fn remove_dir(path: &Path) -> Result<()> {
    match tokio::fs::remove_dir_all(path).await {
        Ok(()) => Ok(()),
        Err(reason) if reason.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(Error::Io {
            action: "remove",
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Every directory and file MixEngine owns, resolved once at startup.
///
/// Built from the root plus the `[paths]` section of `config.toml`, so the rest of the code asks
/// this type where something goes instead of joining strings and re-deciding what "overridden"
/// means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    root: PathBuf,
    bin: PathBuf,
    runtimes: PathBuf,
    packages: PathBuf,
    data: PathBuf,
    etc: PathBuf,
    certs: PathBuf,
    logs: PathBuf,
    extensions: PathBuf,
    blueprints: PathBuf,
    run: PathBuf,
    cache: PathBuf,
    database_file: PathBuf,
    config_file: PathBuf,
    credentials_file: PathBuf,
    daemon_log_file: PathBuf,
    lock_file: PathBuf,
}

impl Paths {
    /// Lay out `root`, applying the user's `[paths]` overrides.
    ///
    /// An override may be absolute (`D:\mixengine\runtimes`, a second disk with room for it) or
    /// relative, in which case it is taken relative to the root rather than to the process's
    /// working directory — relative-to-cwd would mean the same config file describing a different
    /// machine depending on where the daemon happened to be started.
    ///
    /// Overrides arrive already validated by [`crate::config`], which is where "relative" is made
    /// to mean what it says. The Windows paths that are neither absolute nor relative to anything
    /// the config file names — `\bulk`, rooted without a drive, and `C:bulk`, a drive without its
    /// root — would both be resolved by `join` against the *current* drive rather than against the
    /// root, so they are refused when the file is read rather than quietly redirected here. So is
    /// an override that resolves back to the root or above it (`""`, `"."`, `".."`).
    #[must_use]
    pub fn new(root: PathBuf, overrides: &PathOverrides) -> Self {
        let under = |name: &str, override_: Option<&PathBuf>| match override_ {
            Some(path) if path.is_absolute() => path.clone(),
            Some(path) => root.join(path),
            None => root.join(name),
        };

        // The one path built on top of another rather than on the root: moving `logs/` to a second
        // disk has to take `daemon.log` with it, or the override would silently only apply to the
        // service logs.
        let logs = under("logs", overrides.logs.as_ref());

        // The other one, for the same reason in reverse: `run/` cannot be moved by `[paths]`, so the
        // lock is built on it rather than on the root to keep the two from ever disagreeing about
        // which directory the daemon's runtime scratch is.
        let run = under("run", None);

        Self {
            bin: under("bin", None),
            runtimes: under("runtimes", overrides.runtimes.as_ref()),
            packages: under("packages", overrides.packages.as_ref()),
            data: under("data", overrides.data.as_ref()),
            etc: under("etc", None),
            certs: under("certs", None),
            daemon_log_file: logs.join(DAEMON_LOG_FILE_NAME),
            logs,
            extensions: under("extensions", None),
            blueprints: under("blueprints", None),
            cache: under("cache", None),
            lock_file: run.join(LOCK_FILE_NAME),
            run,
            database_file: under(DATABASE_FILE_NAME, None),
            config_file: under(CONFIG_FILE_NAME, None),
            credentials_file: under(CREDENTIALS_FILE_NAME, None),
            root,
        }
    }

    /// `MIXENGINE_HOME` itself.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Version-resolving shims: `php`, `node`, `composer` … This is the directory that goes on
    /// `PATH`.
    #[must_use]
    pub fn bin(&self) -> &Path {
        &self.bin
    }

    /// Installed language runtimes, one directory per `kind/version`.
    #[must_use]
    pub fn runtimes(&self) -> &Path {
        &self.runtimes
    }

    /// Installed servers, databases and caches, one directory per `name/version`.
    #[must_use]
    pub fn packages(&self) -> &Path {
        &self.packages
    }

    /// Per-instance service data — the user's databases. Never regenerated, never deleted.
    #[must_use]
    pub fn data(&self) -> &Path {
        &self.data
    }

    /// Generated configuration. Disposable by design: it is a projection of the database and is
    /// never parsed back into state.
    #[must_use]
    pub fn etc(&self) -> &Path {
        &self.etc
    }

    /// The internal CA and the per-site certificates it issues.
    #[must_use]
    pub fn certs(&self) -> &Path {
        &self.certs
    }

    /// `daemon.log` plus a directory per supervised service.
    #[must_use]
    pub fn logs(&self) -> &Path {
        &self.logs
    }

    /// Installed extensions, one directory per extension id.
    #[must_use]
    pub fn extensions(&self) -> &Path {
        &self.extensions
    }

    /// Captured blueprints, one TOML file each.
    #[must_use]
    pub fn blueprints(&self) -> &Path {
        &self.blueprints
    }

    /// Runtime scratch: pid files, sockets, health markers. Safe to delete while nothing runs.
    #[must_use]
    pub fn run(&self) -> &Path {
        &self.run
    }

    /// Downloaded answers that can always be asked for again: the signed package index and, later,
    /// partial downloads.
    ///
    /// Not `run/`, although both are disposable: `run/` is scratch belonging to *this* daemon and is
    /// safe to empty between runs, while the whole value of a cached index is that it survives a
    /// reboot — an offline machine that lost its cache on restart would be an offline machine that
    /// can list nothing.
    ///
    /// Not private either. Everything in here is a document we publish to the world, and the
    /// signature is what makes it trustworthy rather than the file permissions; the index is
    /// re-verified on every read for exactly that reason.
    ///
    /// Not relocatable by `[paths]`, on the rule that a key arrives with the task that reads it: an
    /// index measured in kilobytes is not why anyone moves a directory to a second disk.
    #[must_use]
    pub fn cache(&self) -> &Path {
        &self.cache
    }

    /// The SQLite database.
    #[must_use]
    pub fn database_file(&self) -> &Path {
        &self.database_file
    }

    /// The user's `config.toml`.
    #[must_use]
    pub fn config_file(&self) -> &Path {
        &self.config_file
    }

    /// `credentials.json`: where a build that is not a release keeps its credentials — T184.
    #[must_use]
    pub fn credentials_file(&self) -> &Path {
        &self.credentials_file
    }

    /// The daemon's own log, inside [`logs`](Self::logs) and therefore moved by the same override.
    #[must_use]
    pub fn daemon_log_file(&self) -> &Path {
        &self.daemon_log_file
    }

    /// Where one service's output is written: `logs/services/<service-id>/`.
    ///
    /// Built rather than stored, because there is one of these per service and the set is not known
    /// until something starts one. The directory need not exist — the supervisor creates it when it
    /// opens the file, since it is the process that holds the handle.
    ///
    /// A [`ServiceId`] is checked to be a usable directory name when it is parsed (see
    /// `docs/architecture/process-supervision.md`), which is what makes this a join rather than
    /// an escaping problem.
    #[must_use]
    pub fn service_logs(&self, service: &ServiceId) -> PathBuf {
        self.logs.join(SERVICES_LOG_DIR_NAME).join(service.as_str())
    }

    /// Where this home's crash reports are written: `logs/crashes/` — roadmap task **T91**.
    ///
    /// **Built rather than stored, and deliberately not one of [`directories`](Self::directories)**,
    /// on [`service_logs`](Self::service_logs)' reasoning: the first crash creates it, so a home
    /// that has never crashed has no such directory — which is a more useful thing for somebody to
    /// find than an empty one.
    ///
    /// Under `logs/` so that a `[paths] logs` override onto a bigger disk takes the reports with it,
    /// and so that `mix uninstall` removes them with the rest of the log directory rather than
    /// needing a line of its own.
    #[must_use]
    pub fn crashes(&self) -> PathBuf {
        self.logs.join(CRASHES_DIR_NAME)
    }

    /// The lock that makes one daemon per home, inside [`run`](Self::run).
    ///
    /// Deliberately not moveable by `[paths]`: it decides which daemon owns this home, and a home
    /// whose lock could be redirected elsewhere would be a home two daemons could both hold.
    #[must_use]
    pub fn lock_file(&self) -> &Path {
        &self.lock_file
    }

    /// The directories no other account on this machine has any business reading.
    ///
    /// `certs/` holds the CA private key and `data/` the user's databases; `run/` holds the socket
    /// and the API token, which are what stands between a local process and the daemon. The root
    /// is here because it is the parent the rest inherit from on Windows, and because a `[paths]`
    /// override can move any of the other three out from under it.
    ///
    /// `bin/`, `etc/`, `logs/`, `runtimes/`, `packages/`, `extensions/` and `blueprints/` are
    /// deliberately absent: they hold downloaded software and generated configuration, and making
    /// them unreadable would break a user reading their own generated nginx config without
    /// protecting anything.
    #[must_use]
    pub fn private_directories(&self) -> [&Path; 4] {
        [&self.root, &self.certs, &self.data, &self.run]
    }

    /// Every directory MixEngine owns, root first.
    #[must_use]
    pub fn directories(&self) -> [&Path; 12] {
        [
            &self.root,
            &self.bin,
            &self.runtimes,
            &self.packages,
            &self.data,
            &self.etc,
            &self.certs,
            &self.logs,
            &self.extensions,
            &self.blueprints,
            &self.run,
            &self.cache,
        ]
    }

    /// Create every directory that does not exist yet, and shut other users out of the private
    /// ones.
    ///
    /// Idempotent: a complete home is walked, found intact, and left alone. Deleting `etc/` and
    /// starting the daemon is therefore a supported repair, not an accident. The permissions are
    /// re-applied on every start rather than only on the ones that create something — a home from
    /// an older version, or one copied off a USB stick, arrives with whatever the last filesystem
    /// thought and would otherwise keep it forever.
    ///
    /// Permissions are set immediately after each directory is created rather than in a second
    /// pass, so the window in which `certs/` exists and is world-readable is as short as the OS
    /// allows. It cannot be closed entirely from here: creating a directory with a mode is a
    /// platform detail, and [`create_dir`] is deliberately not one.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] naming the first directory that could not be created, and [`Error::Platform`]
    /// when one of them cannot be made private — which fails the start rather than continuing with
    /// the CA key readable by every account on the machine.
    pub fn bootstrap(&self, host: &dyn Host) -> Result<()> {
        let private = self.private_directories();
        let access = host.directory_access();

        for directory in self.directories() {
            create_dir(directory)?;

            // **Only where the restriction is not already in force** — roadmap task **T206e**. On
            // Windows, restricting the root rewrites the inherited permissions of every file under
            // it, and with an installed `msys2` that is 54,000 files: every start waited minutes on
            // `icacls` before the pipe opened. Asking is one listing of the directory itself. A
            // question that cannot be answered falls through to the restriction, which reports
            // what is wrong as it always has.
            if private.contains(&directory)
                && !access.is_restricted_to_owner(directory).unwrap_or(false)
            {
                access.restrict_to_owner(directory)?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    /// **Only "not found" is gone** — roadmap task **T206d**, D1. A folder that is there but out of
    /// reach is not something to offer a reinstall over.
    #[test]
    fn only_not_found_is_gone() {
        use std::io::{Error, ErrorKind};

        let failed =
            |kind: ErrorKind| -> std::io::Result<std::fs::Metadata> { Err(Error::from(kind)) };
        assert!(super::gone(&failed(ErrorKind::NotFound)));
        for kind in [
            ErrorKind::PermissionDenied,
            ErrorKind::TimedOut,
            ErrorKind::Other,
        ] {
            assert!(!super::gone(&failed(kind)), "{kind:?}");
        }
        assert!(!super::gone(&std::fs::symlink_metadata(
            std::env::temp_dir()
        )));
    }

    #[tokio::test]
    async fn a_deleted_folder_is_gone_and_one_that_is_there_is_not() {
        let root = tempfile::tempdir().expect("a temporary directory");
        let there = root.path().join("there");
        std::fs::create_dir(&there).expect("a folder");
        assert!(!super::is_gone(&there).await);
        assert!(super::is_gone(&root.path().join("never")).await);
    }

    use super::*;

    /// T184: the credentials belong to the home, so no `[paths]` override moves them.
    #[test]
    fn the_credentials_file_stays_at_the_root_whatever_moves() {
        let overrides = PathOverrides {
            data: Some(PathBuf::from("/elsewhere/data")),
            ..PathOverrides::default()
        };
        let paths = Paths::new(PathBuf::from("/home/me/MixEngine-dev"), &overrides);

        assert_eq!(
            paths.credentials_file(),
            Path::new("/home/me/MixEngine-dev/credentials.json")
        );
    }

    /// T191: a manifest's `bin/php` joined onto an install directory is spelled as this system
    /// spells one — no `/` left inside a Windows path.
    #[test]
    fn a_stored_relative_path_joins_in_this_systems_spelling() {
        let base = std::env::temp_dir().join("install");

        for relative in ["bin/php", "a/b/c", "bin//php", "bin/php/"] {
            let joined = join_stored(&base, relative);

            if cfg!(windows) {
                assert!(
                    !joined.display().to_string().contains('/'),
                    "{relative} joined as {}",
                    joined.display()
                );
            }
            assert!(joined.starts_with(&base), "{}", joined.display());
            assert_eq!(
                joined.file_name().and_then(|name| name.to_str()),
                relative.trim_end_matches('/').rsplit('/').next(),
                "{relative}"
            );
        }

        assert_eq!(join_stored(&base, ""), base);
    }

    /// A value that already arrived native is split too, not kept as one strange name.
    #[test]
    fn a_native_relative_path_joins_to_the_same_answer() {
        let base = std::env::temp_dir().join("install");
        let native_spelled = ["bin", "php"].join(std::path::MAIN_SEPARATOR_STR);

        assert_eq!(
            join_stored(&base, &native_spelled),
            join_stored(&base, "bin/php")
        );
    }

    /// T191 D2: a relocation written with `/` is respelled; on Unix nothing changes.
    #[test]
    fn a_path_written_with_slashes_is_respelled() {
        let cases: &[(&str, &str)] = if cfg!(windows) {
            &[
                ("D:/bulk/data", r"D:\bulk\data"),
                (r"D:\bulk/data", r"D:\bulk\data"),
                ("D:/bulk/data/", r"D:\bulk\data"),
                (r"\\server\share/bulk", r"\\server\share\bulk"),
                ("bulk/data", r"bulk\data"),
            ]
        } else {
            &[
                ("/mnt/bulk/data", "/mnt/bulk/data"),
                ("/mnt/bulk/data/", "/mnt/bulk/data"),
                ("bulk/data", "bulk/data"),
            ]
        };

        // Compared as text: `Path`'s own `==` goes by components, and on Windows it calls
        // `D:/bulk` and `D:\bulk` equal — the very difference this is about.
        for (written, expected) in cases {
            assert_eq!(
                native(Path::new(written)).as_os_str(),
                *expected,
                "{written}"
            );
        }
    }

    /// T191 D3: a stored doc root reaches the wire in this system's spelling.
    #[test]
    fn a_stored_relative_path_is_respelled_for_the_wire() {
        let expected = ["public", "assets"].join(std::path::MAIN_SEPARATOR_STR);

        assert_eq!(native_relative("public/assets"), expected);
        assert_eq!(native_relative("public"), "public");
        assert_eq!(native_relative(""), "");
    }
}
