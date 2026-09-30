//! Finding — and, when they are nowhere to be found, fetching — the command-line tools that dump
//! and restore a database: `mysqldump`/`mysql`, `pg_dump`/`psql` and `mongodump`/`mongorestore`.
//!
//! They are not bundled with the app. `mysqldump` is GPL, the PostgreSQL and MongoDB downloads are
//! hundreds of megabytes apiece, and most machines that talk to a database already have one set
//! installed — so the app looks for what is there first, and only downloads a copy of its own when
//! asked to.
//!
//! Not every suite can be downloaded on every platform, and the difference is the vendor's rather
//! than the app's: see {@link archive_source}. Where there is nothing to fetch the tools have to
//! come from the machine, and the settings screen asks for a path instead of offering a button.
//!
//! A downloaded copy lives under the app's data directory and is never put on `PATH`: it belongs
//! to MixLab rather than to the machine.

use crate::error::AppError;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use crate::platform::hide_console;
use std::time::Duration;

/// One program this module knows how to find.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    MysqlDump,
    MysqlClient,
    PgDump,
    PsqlClient,
    MongoDump,
    MongoRestore,
}

/// The download a tool comes from, when it has to be downloaded: the two tools of a suite arrive
/// in the same archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suite {
    Mysql,
    Postgres,
    Mongo,
}

impl Suite {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "mysql" => Ok(Self::Mysql),
            "postgres" => Ok(Self::Postgres),
            "mongo" => Ok(Self::Mongo),
            other => Err(err!("error.unknownToolSuite", suite = other)),
        }
    }

    fn slug(self) -> &'static str {
        match self {
            Self::Mysql => "mysql",
            Self::Postgres => "postgres",
            Self::Mongo => "mongo",
        }
    }

    /// The tools that have to be present for the suite to count as installed.
    pub fn tools(self) -> [Tool; 2] {
        match self {
            Self::Mysql => [Tool::MysqlDump, Tool::MysqlClient],
            Self::Postgres => [Tool::PgDump, Tool::PsqlClient],
            Self::Mongo => [Tool::MongoDump, Tool::MongoRestore],
        }
    }

    /// Where a downloaded copy of this suite lives. One directory each, so removing a suite is
    /// removing its directory and cannot take the other's files with it.
    fn dir(self, tools_dir: &Path) -> PathBuf {
        tools_dir.join(self.slug())
    }
}

impl Tool {
    /// Every tool, in the order the settings screen lists them.
    pub const ALL: [Tool; 6] = [
        Tool::MysqlDump,
        Tool::MysqlClient,
        Tool::PgDump,
        Tool::PsqlClient,
        Tool::MongoDump,
        Tool::MongoRestore,
    ];

    pub fn parse(value: &str) -> Result<Self, AppError> {
        Self::ALL
            .into_iter()
            .find(|tool| tool.stem() == value)
            .ok_or_else(|| err!("error.unknownTool", tool = value))
    }

    pub fn stem(self) -> &'static str {
        match self {
            Self::MysqlDump => "mysqldump",
            Self::MysqlClient => "mysql",
            Self::PgDump => "pg_dump",
            Self::PsqlClient => "psql",
            Self::MongoDump => "mongodump",
            Self::MongoRestore => "mongorestore",
        }
    }

    fn file_name(self) -> String {
        if cfg!(windows) {
            format!("{}.exe", self.stem())
        } else {
            self.stem().to_string()
        }
    }

    pub fn suite(self) -> Suite {
        match self {
            Self::MysqlDump | Self::MysqlClient => Suite::Mysql,
            Self::PgDump | Self::PsqlClient => Suite::Postgres,
            Self::MongoDump | Self::MongoRestore => Suite::Mongo,
        }
    }
}

/// Where a tool was found, which is what the settings screen offers to change.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// A path the user picked themselves, which wins over everything else.
    Custom,
    /// The copy MixLab downloaded.
    Downloaded,
    /// Something already on this machine — on PATH, or in a usual install directory.
    System,
}

/// One tool as the settings screen shows it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    /// `mysqldump`, `mysql`, `mongodump` or `mongorestore`.
    pub name: &'static str,
    pub suite: &'static str,
    /// Where the tool is, or `None` when it is nowhere to be found.
    pub path: Option<String>,
    pub source: Option<Source>,
    /// Whether MixLab can fetch this tool for itself on this platform. An answer about the suite
    /// rather than the tool, repeated on each of its members so the settings screen — which reads
    /// tools, not suites — has it without asking a second question.
    pub downloadable: bool,
}

/// The file the user's own choices of tool are remembered in.
fn overrides_file(tools_dir: &Path) -> PathBuf {
    tools_dir.join("paths.json")
}

fn load_overrides(tools_dir: &Path) -> HashMap<String, String> {
    std::fs::read_to_string(overrides_file(tools_dir))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Remembers where the user said a tool is, or forgets it when given no path.
pub fn set_path(tool: Tool, path: Option<&str>, tools_dir: &Path) -> Result<(), AppError> {
    let mut overrides = load_overrides(tools_dir);
    match path.map(str::trim).filter(|path| !path.is_empty()) {
        Some(path) => {
            if !Path::new(path).is_file() {
                return Err(err!("error.noFileAt", path = path));
            }
            overrides.insert(tool.stem().to_string(), path.to_string());
        }
        None => {
            overrides.remove(tool.stem());
        }
    }
    std::fs::create_dir_all(tools_dir).map_err(|e| {
        err!(
            "error.cannotCreateDirectory",
            path = tools_dir.display(),
            message = e
        )
    })?;
    let text = serde_json::to_string_pretty(&overrides)
        .map_err(|e| err!("error.cannotSaveToolPath", message = e))?;
    std::fs::write(overrides_file(tools_dir), text)
        .map_err(|e| err!("error.cannotSaveToolPath", message = e))
}

/// The MySQL client version downloaded. An 8.0 client talks to 5.5 and up, so one build covers
/// every server the app is likely to meet — what a 5.x server needs beyond that is
/// `--column-statistics=0`, which the dump adds when it sees one.
const MYSQL_VERSION: &str = "8.0.40";

/// The PostgreSQL version downloaded, as EDB numbers its builds: the release, then their own build
/// number after the dash.
///
/// Unlike {@link MYSQL_VERSION} this constant has a shelf life, and the difference is worth
/// spelling out. `pg_dump` refuses outright to dump a server whose major version is newer than its
/// own — it will reach backwards to any older release, but not forwards by even one. So the pin has
/// to be the newest major there is, and it has to be raised each September when the next one lands,
/// or dumping stops working for whoever upgrades their server first.
const PG_VERSION: &str = "18.6-1";

/// Linux takes 8.4 instead, and not for its own sake: every 8.0 build published as a Linux tarball
/// is linked against `libncurses.so.5`, which the distributions this app runs on stopped shipping
/// years ago, so the client would not start. The 8.4 build is linked against ncurses 6 and reaches
/// back to 5.7 all the same.
const MYSQL_LINUX_VERSION: &str = "8.4.6";
const MONGO_TOOLS_VERSION: &str = "100.17.0";

/// Directories searched beyond `PATH`, since a database installed from an installer rather than a
/// package manager routinely leaves its `bin` off `PATH` entirely.
///
/// A `*` stands for one level of subdirectory, which is how the version-numbered install roots
/// (`MySQL Server 8.0`, `Tools/100`) are reached.
#[cfg(windows)]
const EXTRA_DIRS: &[&str] = &[
    r"C:\Program Files\MySQL\*\bin",
    r"C:\Program Files (x86)\MySQL\*\bin",
    r"C:\Program Files\MariaDB *\bin",
    r"C:\Program Files\PostgreSQL\*\bin",
    r"C:\Program Files\MongoDB\Tools\*\bin",
    r"C:\Program Files\MongoDB\Server\*\bin",
    r"C:\xampp\mysql\bin",
    r"C:\laragon\bin\mysql\*\bin",
    r"C:\laragon\bin\postgresql\*\bin",
    r"C:\wamp64\bin\mysql\*\bin",
];

#[cfg(not(windows))]
const EXTRA_DIRS: &[&str] = &[
    "/usr/bin",
    "/usr/local/bin",
    "/opt/homebrew/bin",
    "/usr/local/mysql/bin",
    "/opt/local/bin",
    // PostgreSQL's client programs are routinely off `PATH`: Debian and Ubuntu keep one set per
    // major version here and put only the wrappers on `PATH`, Red Hat uses `/usr/pgsql-16`, the
    // graphical installer uses `/Library/PostgreSQL`, and Homebrew keg-only `libpq` never links
    // `psql` into `/opt/homebrew/bin` at all.
    "/usr/lib/postgresql/*/bin",
    "/usr/pgsql-*/bin",
    "/Library/PostgreSQL/*/bin",
    "/opt/homebrew/opt/libpq/bin",
    "/usr/local/opt/libpq/bin",
];

/// Every directory an entry of {@link EXTRA_DIRS} stands for, with its `*` expanded against what
/// is actually on disk.
fn expand_dir(pattern: &str) -> Vec<PathBuf> {
    let Some((head, tail)) = pattern.split_once('*') else {
        return vec![PathBuf::from(pattern)];
    };
    // The `*` may be a whole path segment (`Tools\*\bin`) or only part of one (`MariaDB *`), so
    // the directory to list is the last complete segment before it, and what remains of that
    // segment is a prefix its children must start with.
    let head_path = Path::new(head);
    let (parent, prefix) = match head_path.file_name() {
        Some(name) if !head.ends_with(['/', '\\']) => (
            head_path.parent().unwrap_or(Path::new("")).to_path_buf(),
            name.to_string_lossy().into_owned(),
        ),
        _ => (head_path.to_path_buf(), String::new()),
    };
    let tail = tail.trim_start_matches(['/', '\\']);
    let Ok(entries) = std::fs::read_dir(&parent) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
        .map(|entry| {
            if tail.is_empty() {
                entry.path()
            } else {
                entry.path().join(tail)
            }
        })
        .collect()
}

/// Where `tool` is and how it got there: the path the user chose first, then the copy MixLab
/// downloaded, then whatever this machine already had. `None` when it is nowhere.
pub fn locate(tool: Tool, tools_dir: &Path) -> Option<(PathBuf, Source)> {
    if let Some(chosen) = load_overrides(tools_dir).get(tool.stem()) {
        let path = PathBuf::from(chosen);
        // A path that has since been moved or deleted falls through to the search rather than
        // making the tool unavailable — the choice is a preference, not a promise.
        if path.is_file() {
            return Some((path, Source::Custom));
        }
    }

    let name = tool.file_name();
    let suite_dir = tool.suite().dir(tools_dir);
    // `bin` is where a download puts them; the directory itself is where downloads before the
    // macOS build did, and a copy already on disk should not stop working over a change of layout.
    let downloaded = [suite_dir.join("bin").join(&name), suite_dir.join(&name)]
        .into_iter()
        .find(|path| path.is_file());
    if let Some(path) = downloaded {
        return Some((path, Source::Downloaded));
    }

    let path_dirs = std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .unwrap_or_default();
    path_dirs
        .into_iter()
        .chain(EXTRA_DIRS.iter().flat_map(|pattern| expand_dir(pattern)))
        .map(|dir| dir.join(&name))
        .find(|candidate| candidate.is_file())
        .map(|path| (path, Source::System))
}

pub fn find(tool: Tool, tools_dir: &Path) -> Option<PathBuf> {
    locate(tool, tools_dir).map(|(path, _)| path)
}

/// Every tool and where it stands, for the settings screen.
pub fn status(tools_dir: &Path) -> Vec<ToolStatus> {
    Tool::ALL
        .into_iter()
        .map(|tool| {
            let found = locate(tool, tools_dir);
            ToolStatus {
                name: tool.stem(),
                suite: tool.suite().slug(),
                path: found.as_ref().map(|(path, _)| path.display().to_string()),
                source: found.map(|(_, source)| source),
                downloadable: downloadable(tool.suite()),
            }
        })
        .collect()
}

/// Deletes the copy MixLab downloaded. Anything found on the machine itself is left alone — it was
/// never MixLab's to remove.
pub fn uninstall(suite: Suite, tools_dir: &Path) -> Result<(), AppError> {
    let dir = suite.dir(tools_dir);
    if !dir.exists() {
        return Ok(());
    }
    std::fs::remove_dir_all(&dir).map_err(|e| {
        err!(
            "error.cannotRemoveDirectory",
            path = dir.display(),
            message = e
        )
    })
}

/// Where `tool` is, or the error the caller shows: the frontend asks whether a suite is present
/// before running anything, so reaching this message means it went missing in between.
///
/// What the message offers depends on what this platform can actually do: telling someone on macOS
/// to let MixLab download the MySQL tools is telling them to press a button that fails.
pub fn require(tool: Tool, tools_dir: &Path) -> Result<PathBuf, AppError> {
    find(tool, tools_dir).ok_or_else(|| match (tool.suite(), downloadable(tool.suite())) {
        (Suite::Mysql, true) => err!("error.mysqlToolNotFound", tool = tool.stem()),
        (Suite::Mysql, false) => err!("error.mysqlToolNotInstalled", tool = tool.stem()),
        (Suite::Postgres, true) => err!("error.postgresToolNotFound", tool = tool.stem()),
        (Suite::Postgres, false) => err!("error.postgresToolNotInstalled", tool = tool.stem()),
        (Suite::Mongo, _) => err!("error.mongoToolNotFound", tool = tool.stem()),
    })
}

/// Whether every tool of the suite can be found.
pub fn installed(suite: Suite, tools_dir: &Path) -> bool {
    suite
        .tools()
        .iter()
        .all(|tool| find(*tool, tools_dir).is_some())
}

/// Where the suite's archive is downloaded from for this platform, and the SHA-256 it has to hash
/// to. `None` where the vendor publishes nothing worth fetching — the tools are still used there,
/// they just have to come from the package manager.
///
/// The checksums are pinned rather than fetched alongside the download: one taken from the same
/// server as the file it describes only says that the two arrived together, which is no answer to
/// the question worth asking about a binary that is about to be run with the user's database
/// credentials. They come from MongoDB's own release manifest
/// (<https://downloads.mongodb.org/tools/db/release.json>) and — MySQL publishing only an MD5 —
/// from the archive whose MD5 matched the one MySQL publishes for it.
fn archive_source(suite: Suite) -> Option<(String, &'static str)> {
    let windows = cfg!(windows);
    let macos = cfg!(target_os = "macos");
    let arm = cfg!(target_arch = "aarch64");
    match suite {
        Suite::Mysql if windows => Some((
            format!("https://dev.mysql.com/get/Downloads/MySQL-8.0/mysql-{MYSQL_VERSION}-winx64.zip"),
            "7c3f1c09ba1b4a82df32a8d889533fceaf2692383e386a04ee708a12de66f129",
        )),
        // macOS is published as a .dmg *and* as a plain tarball; it is the tarball that is taken,
        // since a .dmg has to be mounted before anything can be read out of it.
        Suite::Mysql if macos => {
            let (arch, sha256) = if arm {
                ("arm64", "a0b8449c19ef59ca688c93ffd89d42f5d78abe6cc136c0d754c6ccb3b202fb9a")
            } else {
                ("x86_64", "a416ee86e72f22089c41911bfee08be0d4dab3b816923be7b465f65df555b36d")
            };
            Some((
                format!(
                    "https://dev.mysql.com/get/Downloads/MySQL-8.0/mysql-{MYSQL_VERSION}-macos14-{arch}.tar.gz"
                ),
                sha256,
            ))
        }
        // Linux takes the "minimal" tarball — the same programs as the full one, without the debug
        // symbols and test suite that make it close to a gigabyte. It is published for x86-64 only,
        // so that is what is asked for by name rather than by "not ARM": every other architecture
        // this could be built for takes its tools from the package manager, which has them.
        Suite::Mysql if cfg!(target_arch = "x86_64") => Some((
            format!(
                "https://dev.mysql.com/get/Downloads/MySQL-8.4/mysql-{MYSQL_LINUX_VERSION}-linux-glibc2.28-x86_64-minimal.tar.xz"
            ),
            "f284b17b9e038adbe77f0dd5fb11ed30262286b23a390b8b4e367abc3574c42e",
        )),
        Suite::Mysql => None,
        // EnterpriseDB's "binaries" zip — the install tree without the installer around it, which
        // is the whole server at some hundreds of megabytes for the two programs wanted here. The
        // rest is thrown away after unpacking, the same as MySQL's.
        //
        // The download page hands out `sbp.enterprisedb.com/getfile.jsp?fileid=…` links, but every
        // one of them redirects here, to a URL that spells out its version — so the version is
        // pinned rather than an opaque file id that says nothing about what it fetches.
        Suite::Postgres if windows => Some((
            format!(
                "https://get.enterprisedb.com/postgresql/postgresql-{PG_VERSION}-windows-x64-binaries.zip"
            ),
            "fbe23da234ee31547bf8a36d29dfd81e82b849df2d2b78d2eecb43d360252f8c",
        )),
        // One download for both Macs, where MySQL needs two: EDB builds these as universal
        // binaries, so the same file carries the Intel and Apple Silicon halves and there is no
        // architecture to choose between.
        Suite::Postgres if macos => Some((
            format!(
                "https://get.enterprisedb.com/postgresql/postgresql-{PG_VERSION}-osx-binaries.zip"
            ),
            "2a6739fccbbc36474cb2446e4e7b4f377abb8471653d3a294e4e5092271e4796",
        )),
        // Linux: EDB stopped building it after PostgreSQL 10, and there is no other official
        // client-only download to pin a checksum against. So there `pg_dump` and `psql` are found
        // rather than fetched — which is why {@link EXTRA_DIRS} knows more places to look for them
        // than for anything else — and where they are nowhere to be found, the settings screen asks
        // for a path instead.
        Suite::Postgres => None,
        Suite::Mongo => {
            let (platform, extension, sha256) = match (windows, macos, arm) {
                (true, _, _) => (
                    "windows-x86_64",
                    "zip",
                    "07b8fca56272397490102051edad4aeadc79369365ffdcda4ff70b4549512c5b",
                ),
                (_, true, true) => (
                    "macos-arm64",
                    "zip",
                    "099691c9059b25504a1b318bc31b3b9bd965ff78ce6b9f629090f89b25539dac",
                ),
                (_, true, false) => (
                    "macos-x86_64",
                    "zip",
                    "b488e12a3e2399f8ee3ba0abf6da54dbac1bda678c230963edaa7c435887ae99",
                ),
                (_, _, true) => (
                    "ubuntu2204-arm64",
                    "tgz",
                    "59d4475a767c75d319d120189ecd853e017038874a01dc985610938b330391c1",
                ),
                _ => (
                    "ubuntu2204-x86_64",
                    "tgz",
                    "f30d0b3115cc31b1f360af2341a794d890c74ceb41e5a4931d3b945efeeb628e",
                ),
            };
            Some((
                format!(
                    "https://fastdl.mongodb.org/tools/db/mongodb-database-tools-{platform}-{MONGO_TOOLS_VERSION}.{extension}"
                ),
                sha256,
            ))
        }
    }
}

/// Whether MixLab has anywhere to download this suite from on this platform. `false` means the
/// tools have to come from the machine — from a package manager, or from a path chosen in
/// Settings — and no button should offer otherwise.
pub fn downloadable(suite: Suite) -> bool {
    archive_source(suite).is_some()
}

/// Checks a downloaded archive against the checksum pinned for it.
///
/// A mismatch means the file is not the one this build of MixLab was made to unpack: a release
/// withdrawn or rebuilt under the same name, a download that came back truncated, or something
/// between here and the vendor handing over a different file. None of those should be unpacked
/// and then run with the credentials of every database the user connects to.
fn verify_sha256(path: &Path, expected: &str) -> Result<(), AppError> {
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

/// How far an install has got, for the settings screen to show. Emitted often enough during the
/// download that a 60MB fetch looks like it is moving, and once at every change of stage — a
/// download that has finished is not an install that has finished, and the difference is minutes
/// of unpacking on a slow disk.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// Which suite this is about, so a screen showing both can tell the two apart.
    pub suite: &'static str,
    /// `downloading`, `verifying`, `unpacking` or `installing`. There is no `done`: the install
    /// command returning is what says it finished, and one fact should have one teller.
    pub stage: &'static str,
    /// Bytes fetched so far, and how many there are in all. A `total` of `0` means the server
    /// never said — the bar shows movement without a percentage rather than a wrong one.
    pub done: u64,
    pub total: u64,
}

impl Progress {
    fn stage(suite: Suite, stage: &'static str) -> Self {
        Self {
            suite: suite.slug(),
            stage,
            done: 0,
            total: 0,
        }
    }
}

/// A helper program, set up the way this module always wants one: nothing on stdin, output thrown
/// away, complaints kept back for the error message, and — on Windows — no console window flashing
/// up in the user's face.
fn helper(program: &str) -> Command {
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
fn finish(child: Child, what: &'static str) -> Result<(), AppError> {
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
fn run(program: &str, args: &[&str], what: &'static str) -> Result<(), AppError> {
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
fn content_length(url: &str) -> u64 {
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

/// Fetches the archive, reporting how far it has got as it goes.
///
/// The count comes from the size of the file being written rather than from curl's own progress
/// meter: curl draws that for a terminal, redrawing one line with carriage returns, and reading a
/// number back out of it is a great deal more fragile than asking the filesystem.
fn download(
    url: &str,
    archive: &Path,
    suite: Suite,
    report: &dyn Fn(Progress),
) -> Result<(), AppError> {
    let total = content_length(url);
    report(Progress {
        suite: suite.slug(),
        stage: "downloading",
        done: 0,
        total,
    });

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
                report(Progress {
                    suite: suite.slug(),
                    stage: "downloading",
                    done,
                    total,
                });
                std::thread::sleep(Duration::from_millis(250));
            }
            Err(e) => return Err(AppError::new("error.downloadFailed").with("message", e)),
        }
    }
    // `wait_with_output` after the process has already been reaped returns the status it kept, so
    // the exit code and stderr are still the ones curl left behind.
    finish(child, "error.downloadFailed")
}

/// Libraries in a PostgreSQL download that `pg_dump` and `psql` never load, matched on the start of
/// the name. ICU is the one that matters: the collation data belongs to the server, and on macOS —
/// where the archive stores each versioned alias as a copy of the library rather than as a link to
/// it — the three of them come to 229MB, against 60MB for everything the two programs actually
/// need. The rest are the ECPG precompiler's and the server's XML support, and on Windows the Stack
/// Builder's GUI.
///
/// Both spellings of ICU are here because the platforms name it differently — `icudt77.dll` beside
/// `libicudata.77.dylib` — and one list checked everywhere is less to keep in step than one per
/// platform.
///
/// Named as what to leave rather than what to take, so that a library a later release starts
/// linking against is copied by default: shipping a few megabytes too many is a waste, shipping one
/// file too few is a program that will not start.
const PG_SPARE_LIBRARIES: &[&str] = &[
    "icu",
    "libicu",
    "wx",
    "libxml2",
    "libxslt",
    "libecpg",
    "libpgtypes",
    "testplug",
];

/// Where a library the tools need has to be put, relative to the suite's own directory, or `None`
/// for a file that is not one of them. `parent` is the directory it was found in, inside the
/// unpacked archive.
///
/// Each platform looks for these in a fixed place relative to the program — beside it on Windows,
/// `@loader_path/../lib` on macOS, `$ORIGIN/../lib/private` on Linux. Put anywhere else they might
/// as well not have been downloaded.
///
/// What counts as one differs by suite. The MySQL clients need only OpenSSL beyond what the
/// operating system provides; `pg_dump` and `psql` arrive with their whole dependency tree in the
/// archive — libpq, OpenSSL, Kerberos, gettext and three compressors — so there it is easier to say
/// which of the libraries beside them are the ones they do *not* want, which is
/// {@link PG_SPARE_LIBRARIES}.
fn library_dir(suite: Suite, name: &str, parent: &Path) -> Option<PathBuf> {
    let in_dir = |dir: &str| parent.file_name().is_some_and(|found| found == dir);
    let lower = name.to_lowercase();
    let spare = || {
        PG_SPARE_LIBRARIES
            .iter()
            .any(|spare| lower.starts_with(spare))
    };
    let openssl = || name.starts_with("libssl") || name.starts_with("libcrypto");

    if cfg!(windows) {
        // Windows loads from the executable's own directory, and the archive keeps the DLLs there.
        if !in_dir("bin") || !lower.ends_with(".dll") {
            return None;
        }
        return (suite != Suite::Postgres || !spare()).then(|| PathBuf::from("bin"));
    }
    if cfg!(target_os = "macos") {
        if !in_dir("lib") || !name.ends_with(".dylib") {
            return None;
        }
        return match suite {
            Suite::Postgres => !spare(),
            _ => openssl(),
        }
        .then(|| PathBuf::from("lib"));
    }
    // Linux, where only MySQL is fetched: PostgreSQL has no download here, so nothing has had to
    // say which of its libraries to keep.
    (openssl() && in_dir("private") && name.contains(".so."))
        .then(|| PathBuf::from("lib").join("private"))
}

/// Copies out of `dir`, recursively, every file the suite needs, into the layout its tools expect
/// to be run from: the programs in `bin`, and with them, where {@link library_dir} says each one
/// belongs, the libraries they will not start without.
///
/// Symbolic links are passed over: the archives carry a versioned library and unversioned links to
/// it, and copying a link here would copy the whole library a second time under another name.
fn collect(dir: &Path, suite: Suite, target: &Path) -> Result<usize, AppError> {
    let wanted: Vec<String> = suite.tools().iter().map(|tool| tool.file_name()).collect();
    let mut found = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if entry.file_type().is_ok_and(|kind| kind.is_symlink()) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_tool = wanted.iter().any(|want| want == &name);
            let Some(relative) = (if is_tool {
                Some(PathBuf::from("bin"))
            } else {
                library_dir(suite, &name, &current)
            }) else {
                continue;
            };
            let into = target.join(relative);
            std::fs::create_dir_all(&into).map_err(|e| {
                err!(
                    "error.cannotCreateDirectory",
                    path = into.display(),
                    message = e
                )
            })?;
            std::fs::copy(&path, into.join(&name)).map_err(|e| {
                err!(
                    "error.cannotCopyTool",
                    tool = name,
                    path = into.display(),
                    message = e
                )
            })?;
            if is_tool {
                found += 1;
            }
        }
    }
    Ok(found)
}

/// What a staging directory is named, and what [`sweep_staging`] goes looking for.
const STAGING_PREFIX: &str = "download-";

/// How long a staging directory has to have been untouched before it counts as abandoned.
///
/// Nothing stops a second copy of MixLab being open, and a sweep at startup must not delete the
/// download the other one is in the middle of. Six hours is far past any download that is still
/// going and far short of leaving the disk full until the next install.
const STALE_AFTER: Duration = Duration::from_secs(6 * 60 * 60);

/// The temporary directory one install unpacks into, removed however the install ends.
struct Staging {
    path: PathBuf,
}

impl Staging {
    fn new(tools_dir: &Path) -> Result<Self, AppError> {
        let path = tools_dir.join(format!("{STAGING_PREFIX}{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).map_err(|e| {
            err!(
                "error.cannotCreateDirectory",
                path = path.display(),
                message = e
            )
        })?;
        Ok(Self { path })
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Deletes the staging directories left behind by installs that never finished.
///
/// [`Staging`] takes its own away on every path out of `install`, including a failed download —
/// but not when the process never gets to run anything at all: a crash, a power cut, the app being
/// killed mid-download. What is left is an unpacked server distribution, several hundred megabytes
/// in MySQL's case, that nothing would otherwise ever come back for.
pub fn sweep_staging(tools_dir: &Path) {
    sweep_staging_older_than(tools_dir, STALE_AFTER);
}

fn sweep_staging_older_than(tools_dir: &Path, stale_after: Duration) {
    let Ok(entries) = std::fs::read_dir(tools_dir) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(STAGING_PREFIX)
        {
            continue;
        }
        if last_touched(&entry.path()).is_some_and(|idle| idle < stale_after) {
            continue;
        }
        let _ = std::fs::remove_dir_all(entry.path());
    }
}

/// How long ago anything in the staging directory was last written, as far as one level down.
///
/// One level is enough to tell a live download from an abandoned one: the archive is a single file
/// written the whole way through, and the unpack that follows starts within seconds of it. Walking
/// the whole tree would mean stat-ing thousands of files of an unpacked server to answer a
/// question that has already been answered. `None` — nothing readable in there — counts as stale.
fn last_touched(path: &Path) -> Option<Duration> {
    let times = std::iter::once(path.to_path_buf())
        .chain(
            std::fs::read_dir(path)
                .ok()?
                .flatten()
                .map(|entry| entry.path()),
        )
        .filter_map(|path| path.metadata().ok()?.modified().ok())
        .filter_map(|time| time.elapsed().ok());
    times.min()
}

/// Downloads the suite's own tools into `tools_dir`.
///
/// `curl` and `tar` do the fetching and unpacking: both ship with Windows 10 and up and with every
/// macOS and Linux this app runs on, which is a great deal less to carry than an HTTP client and a
/// zip reader for the sake of an operation most users run once.
///
/// The archive holds an entire server distribution in MySQL's case, so it is unpacked to a
/// temporary directory, the few files that matter are taken out of it, and the rest is deleted.
///
/// `report` is called as each stage begins and, while the archive is coming down, every quarter
/// second — this takes minutes on an ordinary connection, and the one question the user has all
/// the way through is whether it is still going.
pub fn install(suite: Suite, tools_dir: &Path, report: &dyn Fn(Progress)) -> Result<(), AppError> {
    let (url, sha256) = archive_source(suite).ok_or_else(|| match suite {
        Suite::Postgres => err!("error.noPostgresArchive"),
        // Mongo is published for every platform this builds for, so it is only ever MySQL that
        // arrives here beside PostgreSQL.
        Suite::Mysql | Suite::Mongo => err!("error.noMysqlArchive"),
    })?;

    let target = suite.dir(tools_dir);
    std::fs::create_dir_all(&target).map_err(|e| {
        err!(
            "error.cannotCreateDirectory",
            path = target.display(),
            message = e
        )
    })?;
    // Whatever happens next, the staging directory goes: it holds a whole unpacked server. A guard
    // rather than a call at the end, so that a panic takes it away too.
    let staging = Staging::new(tools_dir)?;
    let staging = &staging.path;

    let archive = staging.join("tools-archive");
    (|| {
        download(&url, &archive, suite, report)?;
        // Before anything is unpacked, let alone run.
        report(Progress::stage(suite, "verifying"));
        verify_sha256(&archive, sha256)?;
        report(Progress::stage(suite, "unpacking"));
        let unpacked = staging.join("unpacked");
        std::fs::create_dir_all(&unpacked).map_err(|e| {
            err!(
                "error.cannotCreateDirectory",
                path = unpacked.display(),
                message = e
            )
        })?;
        run(
            "tar",
            &[
                "-xf",
                &archive.to_string_lossy(),
                "-C",
                &unpacked.to_string_lossy(),
            ],
            "error.unpackFailed",
        )?;
        report(Progress::stage(suite, "installing"));
        let found = collect(&unpacked, suite, &target)?;
        if found < suite.tools().len() {
            return Err(err!("error.downloadIncomplete"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for tool in suite.tools() {
                let path = target.join("bin").join(tool.file_name());
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
            }
        }
        Ok(())
    })()
}

#[cfg(test)]
mod tests {
    use super::{
        collect, downloadable, expand_dir, install, locate, sweep_staging_older_than,
        verify_sha256, Source, Suite, Tool,
    };
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    /// What the sweep takes and what it leaves.
    ///
    /// The age is a parameter here and a constant in the app, because the alternative is a test
    /// that has to move a directory's modification time backwards — which is a different thing on
    /// each platform, and would be testing the clock rather than the rule.
    #[test]
    fn the_sweep_takes_abandoned_downloads_and_leaves_everything_else() {
        let root = std::env::temp_dir().join(format!("mixlab-test-{}", uuid::Uuid::new_v4()));
        let live = root.join("download-live");
        // An install that got as far as unpacking: what the sweep must be able to remove whole.
        std::fs::create_dir_all(root.join("download-dead/unpacked/bin")).unwrap();
        std::fs::write(root.join("download-dead/unpacked/bin/mysqldump"), b"x").unwrap();
        std::fs::create_dir_all(&live).unwrap();
        // The installed tools, which live in the same directory and must never be touched.
        std::fs::create_dir_all(root.join("mysql/bin")).unwrap();
        std::fs::write(root.join("mysql/bin/mysqldump"), b"x").unwrap();

        // Nothing is old enough yet — which is the case of a second copy of MixLab downloading.
        sweep_staging_older_than(&root, Duration::from_secs(6 * 60 * 60));
        assert!(root.join("download-dead").is_dir());
        assert!(live.is_dir());

        // Everything is, now.
        sweep_staging_older_than(&root, Duration::ZERO);
        assert!(!root.join("download-dead").exists());
        assert!(!live.exists());
        assert!(
            root.join("mysql/bin/mysqldump").is_file(),
            "the installed tools were swept"
        );

        // A tools directory that does not exist is not a failure: it is what a machine that has
        // never downloaded anything looks like.
        sweep_staging_older_than(&root.join("nowhere"), Duration::ZERO);

        std::fs::remove_dir_all(&root).unwrap();
    }

    /// Fetches a suite the way the app does and checks the result is usable: the pinned URL still
    /// serves a file, that file still hashes to what {@link archive_source} pins, the archive still
    /// holds the programs {@link collect} goes looking for, and — the part that cannot be answered
    /// from any other machine — those programs start here, with the libraries copied out beside
    /// them and nothing else.
    ///
    /// A platform with no archive for this suite passes without downloading: that is the vendor's
    /// answer, not a fault.
    fn check_pinned_download(suite: Suite) {
        if !downloadable(suite) {
            eprintln!(
                "{}: no archive for this platform, nothing to check",
                suite.slug()
            );
            return;
        }
        let tools_dir =
            std::env::temp_dir().join(format!("mixlab-download-{}", uuid::Uuid::new_v4()));
        let installed = install(suite, &tools_dir, &|_| {});
        // Every check runs before the cleanup, so that a failure still takes the download with it
        // rather than leaving hundreds of megabytes behind on the runner.
        let ran: Vec<(&str, String)> = suite
            .tools()
            .into_iter()
            .map(|tool| {
                // `locate` would happily return a copy already on the machine, which would make
                // this pass without the download having worked at all.
                let Some((path, Source::Downloaded)) = locate(tool, &tools_dir) else {
                    return (tool.stem(), "was not in the download".to_string());
                };
                match std::process::Command::new(&path).arg("--version").output() {
                    Ok(out) if out.status.success() => (tool.stem(), String::new()),
                    Ok(out) => (
                        tool.stem(),
                        format!(
                            "exited {}: {}",
                            out.status,
                            String::from_utf8_lossy(&out.stderr)
                        ),
                    ),
                    // A missing library reads as the program not starting, which is the whole
                    // reason this runs on a real machine of each kind.
                    Err(e) => (tool.stem(), format!("did not start: {e}")),
                }
            })
            .collect();
        let _ = std::fs::remove_dir_all(&tools_dir);

        if let Err(e) = installed {
            panic!("installing the {} tools failed: {e}", suite.slug());
        }
        for (tool, problem) in ran {
            assert!(problem.is_empty(), "{tool} {problem}");
        }
    }

    /// Ignored because each of these fetches tens to hundreds of megabytes from a vendor, which is
    /// no part of an ordinary test run. `.github/workflows/tool-downloads.yml` runs them on a real
    /// machine of each kind — see `docs/standards/desktop/bumping-tool-downloads.md`.
    #[test]
    #[ignore = "downloads from the vendor; run it through the tool-downloads workflow"]
    fn the_pinned_mysql_download_still_installs_and_runs() {
        check_pinned_download(Suite::Mysql);
    }

    #[test]
    #[ignore = "downloads from the vendor; run it through the tool-downloads workflow"]
    fn the_pinned_postgres_download_still_installs_and_runs() {
        check_pinned_download(Suite::Postgres);
    }

    #[test]
    #[ignore = "downloads from the vendor; run it through the tool-downloads workflow"]
    fn the_pinned_mongo_download_still_installs_and_runs() {
        check_pinned_download(Suite::Mongo);
    }

    /// The empty file's SHA-256, which is a fine stand-in for a real archive: what is under test is
    /// the comparison, not the hashing.
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

    /// A `*` in a search directory stands for one level of subdirectory, whether it is a whole
    /// path segment (`Tools\*\bin`) or only part of one (`MariaDB *`).
    #[test]
    fn a_star_expands_against_what_is_on_disk() {
        let root = std::env::temp_dir().join(format!("mixlab-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("MariaDB 11.4").join("bin")).unwrap();
        std::fs::create_dir_all(root.join("Postgres").join("bin")).unwrap();

        let whole_segment = expand_dir(&format!("{}/*/bin", root.display()));
        let partial_segment = expand_dir(&format!("{}/MariaDB */bin", root.display()));
        std::fs::remove_dir_all(&root).unwrap();

        assert_eq!(whole_segment.len(), 2);
        assert_eq!(partial_segment, vec![root.join("MariaDB 11.4").join("bin")]);
    }

    /// The programs go where the tools are looked for, and the library they are linked against
    /// goes where they will look for it — which is a different place on each platform, and the
    /// whole reason a download is unpacked rather than copied out flat.
    #[test]
    fn a_download_is_laid_out_the_way_the_tools_are_run_from() {
        let root = std::env::temp_dir().join(format!("mixlab-test-{}", uuid::Uuid::new_v4()));
        let unpacked = root.join("unpacked").join("mysql-8.0.40");
        // Where each platform's archive keeps its OpenSSL, and where the clients then look for it.
        let (from, into, library) = if cfg!(windows) {
            ("bin", "bin", "libssl-3-x64.dll")
        } else if cfg!(target_os = "macos") {
            ("lib", "lib", "libssl.3.dylib")
        } else {
            ("lib/private", "lib/private", "libssl.so.3")
        };
        std::fs::create_dir_all(unpacked.join("bin")).unwrap();
        std::fs::create_dir_all(unpacked.join(from)).unwrap();
        for tool in Suite::Mysql.tools() {
            std::fs::write(unpacked.join("bin").join(tool.file_name()), b"").unwrap();
        }
        std::fs::write(unpacked.join(from).join(library), b"").unwrap();
        // Something the tools have no use for, which should be left in the archive.
        std::fs::write(unpacked.join("bin").join("mysqladmin"), b"").unwrap();

        let tools_dir = root.join("tools");
        let found = collect(
            &root.join("unpacked"),
            Suite::Mysql,
            &tools_dir.join("mysql"),
        )
        .unwrap();
        let placed = tools_dir.join("mysql").join(into).join(library).is_file();
        let spare = tools_dir
            .join("mysql")
            .join("bin")
            .join("mysqladmin")
            .exists();
        // The layout is only worth anything if it is also the one the tools are then found in.
        let located = locate(Tool::MysqlDump, &tools_dir);
        std::fs::remove_dir_all(&root).unwrap();

        assert_eq!(found, 2);
        assert!(
            placed,
            "the library the clients load was not put where they look for it"
        );
        assert!(!spare, "a program the suite does not use was copied out");
        let (path, source) = located.expect("the downloaded copy was not found again");
        assert!(matches!(source, Source::Downloaded));
        assert_eq!(path.parent().and_then(Path::file_name).unwrap(), "bin");
    }

    /// A PostgreSQL archive carries the whole install's libraries, most of which belong to the
    /// server, the ECPG precompiler or the Stack Builder rather than to the two programs taken out
    /// of it. What the tools load has to come along; the collation data beside it must not.
    ///
    /// Only where there is a download to shape: on Linux the tools come from the machine, and
    /// nothing has had to decide which of their libraries to keep.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn a_postgres_download_leaves_the_libraries_its_tools_never_load() {
        // Where each platform's archive keeps its libraries, what they are called there, and where
        // the tools then look for them.
        let (from, into, kept, spare): (_, _, &[&str], &[&str]) = if cfg!(windows) {
            (
                "bin",
                "bin",
                &[
                    "libpq.dll",
                    "libssl-3-x64.dll",
                    "libintl-9.dll",
                    "libzstd.dll",
                ],
                &[
                    "icudt77.dll",
                    "wxbase3211u_vc_x64_custom.dll",
                    "libxml2.dll",
                ],
            )
        } else {
            (
                "lib",
                "lib",
                &[
                    "libpq.5.dylib",
                    "libssl.3.dylib",
                    "libintl.8.dylib",
                    "libzstd.1.dylib",
                ],
                &[
                    "libicudata.77.1.dylib",
                    "libxml2.16.dylib",
                    "libecpg.6.dylib",
                ],
            )
        };

        let root = std::env::temp_dir().join(format!("mixlab-test-{}", uuid::Uuid::new_v4()));
        let unpacked = root.join("unpacked").join("pgsql");
        std::fs::create_dir_all(unpacked.join("bin")).unwrap();
        std::fs::create_dir_all(unpacked.join(from)).unwrap();
        for tool in Suite::Postgres.tools() {
            std::fs::write(unpacked.join("bin").join(tool.file_name()), b"").unwrap();
        }
        for name in kept.iter().chain(spare.iter()) {
            std::fs::write(unpacked.join(from).join(name), b"").unwrap();
        }

        let target = root.join("tools").join("postgres");
        let found = collect(&root.join("unpacked"), Suite::Postgres, &target).unwrap();
        let landed = target.join(into);
        let all_kept = kept.iter().all(|name| landed.join(name).is_file());
        let none_spare = spare.iter().all(|name| !landed.join(name).exists());
        std::fs::remove_dir_all(&root).unwrap();

        assert_eq!(found, 2);
        assert!(all_kept, "a library the tools load was left behind");
        assert!(none_spare, "a library the tools never load was copied out");
    }

    #[test]
    fn a_directory_without_a_star_stands_for_itself() {
        assert_eq!(
            expand_dir("/usr/local/bin"),
            vec![PathBuf::from("/usr/local/bin")]
        );
    }
}
