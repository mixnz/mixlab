//! Tools somebody installed into a runtime.
//!
//! `npm install -g yarn`, `pip install poetry`, `gem install rails`. Each of the three package
//! managers has a *bindir* it writes programs into, that directory is **inside the runtime's own
//! install**, and no part of it is ever on anybody's PATH — only `<root>/bin` is, and until this
//! module that directory was a projection of a compile-time constant. So the tool existed, ran
//! perfectly well when named by its full path, and could not be typed.
//!
//! Measured rather than reasoned about, on the reference machine:
//!
//! ```text
//! $ node -e "console.log(process.execPath)"
//! …/runtimes/node/24.19.0/node.exe
//! $ npm config get prefix
//! …/runtimes/node/24.19.0
//! ```
//!
//! # Why this is not "put the runtime's directory on the PATH"
//!
//! A global tool belongs to **one version of one language**. `yarn` installed under Node 24 is not
//! installed under Node 22, and a PATH entry pointing at one install would make `cd`-ing into a
//! project that pins the other run the wrong program — silently, which is the exact failure the
//! whole shim exists to prevent. So a discovered tool becomes a *name* ([`crate::bin_commands`]),
//! and the file behind it is resolved at run time against whichever version the working directory
//! means, exactly as `npm` itself is.
//!
//! # What is not discovered
//!
//! [`scan`] refuses four families, and each of them for its own reason:
//!
//! - **A compiled command.** Somebody who runs `npm install -g npm` has an `npm` in their Node's
//!   bindir; fronting it would make `bin/npm` a shim that dispatches to a file found by a shim, and
//!   the first file it would find is itself.
//! - **The runtime's own programs**, by the artifact's `provides` map. `node` is already a command
//!   and `php-config` is already a command.
//! - **[`RESERVED`]** — MixEngine's own binaries, which a person may reasonably have copied
//!   anywhere and which must never be shadowed by a shim.
//! - **A file this operating system does not execute.** npm writes `yarn`, `yarn.cmd` and
//!   `yarn.ps1` for one tool; three files in `bin/` would be one command and two that do nothing.
//!
//! **What it deliberately does not refuse is a name that shadows a program already on the PATH.** A
//! globally installed package called `git` would put a `git` ahead of the machine's own, and every
//! version manager that fronts global tools has that property. `mix doctor` reports it; refusing it
//! would mean deciding on somebody's behalf that a tool they installed is not one they meant.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use mixengine_proto::RuntimeKind;

/// Names MixEngine never fronts on anybody else's behalf.
///
/// A shim called `mix` would answer every `mix` on this machine with "nothing installed here
/// answers to this command", including the one somebody would type to repair it.
pub const RESERVED: &[&str] = &["mix", "mixengined", "mixengine-shim", "mixengine-elevate"];

/// What a file has to be called to be a program here.
///
/// Windows resolves a bare `yarn` by appending each of these in turn, so a file with any other
/// extension is data whatever it contains — `yarn.ps1` is read by PowerShell and by nothing that
/// `hand_over` can start. On Unix there is no such list and none is used: a bindir holds programs
/// by construction, which is what *bindir* means to npm, pip and gem alike.
const RUNNABLE_ON_WINDOWS: &[&str] = &["exe", "cmd", "bat", "com"];

/// Where `npm install -g`, `pip install` and `gem install` put a program, inside one install.
///
/// A fact about each language's package manager, and the `cfg!` is a **path spelling** rather than a
/// call into the operating system — [`crate::shims::file_name`] sets that exception and
/// `docs/architecture/platform-abstraction.md` draws the line at behaviour a trait could be
/// written for. "Where does npm put a binary" is not something either side of `bin/` can be asked.
///
/// [`None`] for PHP: Composer's global bindir is `~/.composer/vendor/bin`, outside every install
/// directory and therefore a different question — see the spec's *Out of scope*. [`None`] for Go
/// too, for the same reason: `go install` writes into `GOBIN` or `GOPATH/bin`, which every
/// installed Go shares. And for Java: Maven and Gradle keep their programs outside every install.
#[must_use]
pub fn directory(kind: RuntimeKind, install_path: &Path) -> Option<PathBuf> {
    match kind {
        // npm's prefix is the install directory itself on Windows and its `bin` on Unix.
        RuntimeKind::Node => Some(match cfg!(windows) {
            true => install_path.to_path_buf(),
            false => install_path.join("bin"),
        }),

        // Python's `scripts` directory, which the installer and every `pip install` agree on.
        RuntimeKind::Python => Some(match cfg!(windows) {
            true => install_path.join("Scripts"),
            false => install_path.join("bin"),
        }),

        // RubyGems writes into the interpreter's own bindir on both systems.
        RuntimeKind::Ruby => Some(install_path.join("bin")),

        RuntimeKind::Php | RuntimeKind::Go | RuntimeKind::Java | RuntimeKind::Composer => None,
    }
}

/// The commands that directory holds which `bin/` may front.
///
/// `provides` is the artifact's own map as [`crate::runtimes::remember`] recorded it, so the
/// runtime's own programs are not discovered a second time under names that already dispatch.
///
/// An empty set for a directory that is not there, which is the ordinary state of a `Scripts` that
/// nothing has ever installed into — and for a directory that cannot be read, because a scan is a
/// projection and the honest answer for a projection it could not take is "nothing", not a refusal
/// that would sweep `bin/` clean of every other language's tools.
#[must_use]
pub fn scan(
    kind: RuntimeKind,
    install_path: &Path,
    provides: &BTreeMap<String, String>,
) -> BTreeSet<String> {
    let Some(directory) = directory(kind, install_path) else {
        return BTreeSet::new();
    };

    let Ok(entries) = std::fs::read_dir(&directory) else {
        return BTreeSet::new();
    };

    let published = published_names(provides);
    let mut found = BTreeSet::new();

    for entry in entries.flatten() {
        // `metadata` rather than `file_type`, so that a symlink is judged by what it points at:
        // npm's Unix bindir is a directory of symlinks into `lib/node_modules`, and every one of
        // them is a program.
        if !entry.metadata().is_ok_and(|metadata| metadata.is_file()) {
            continue;
        }

        let file = PathBuf::from(entry.file_name());

        let Some(name) = command_name(&file) else {
            continue;
        };

        if is_spoken_for(&name, &published) {
            continue;
        }

        found.insert(name);
    }

    found
}

/// Every tool every installed runtime holds, with the language that decides which copy runs.
///
/// **The pass the daemon repeats and the shape `bin_commands` is recorded in.** Each installed
/// version is scanned against its own `provides`, and the union is taken by name.
///
/// A name two languages both hold goes to the one whose kind comes first in
/// [`RuntimeKind::ALL`](mixengine_proto::RuntimeKind::ALL). Arbitrary, and the same answer on every
/// machine and on every pass, which is the property that matters: `bin/` holds one file per name,
/// so something has to decide, and a decision that moved between scans would make a command mean
/// different things on two consecutive days.
///
/// # Errors
///
/// [`Error::Database`](crate::Error::Database) when the installs cannot be listed. A directory that
/// cannot be read is no tools rather than a failure — see [`scan`].
pub async fn everywhere(store: &crate::Store) -> crate::Result<BTreeMap<String, RuntimeKind>> {
    let rows = sqlx::query!("SELECT kind, install_path, provides_json FROM runtime_installs")
        .fetch_all(store.pool())
        .await
        .map_err(|source| store.failure("read", source))?;

    let mut found: BTreeMap<String, RuntimeKind> = BTreeMap::new();

    for row in rows {
        // A row naming a language this build does not front is skipped rather than refused: it is a
        // home that met a newer release, and the tools of a language this binary cannot resolve are
        // ones it could not run anyway.
        let Some(kind) = RuntimeKind::parse(&row.kind) else {
            continue;
        };

        let provides: BTreeMap<String, String> =
            serde_json::from_str(&row.provides_json).unwrap_or_default();

        for name in scan(kind, Path::new(&row.install_path), &provides) {
            match found.get(&name) {
                // Already held by a language that sorts earlier. Nothing to do, and nothing to
                // report: this is a coincidence between two ecosystems' package names.
                Some(held) if *held <= kind => {}
                _ => {
                    found.insert(name, kind);
                }
            }
        }
    }

    Ok(found)
}

/// The command a file in a bindir would be typed as, and [`None`] for a file that is not one.
///
/// On Windows the extension is the loader's and is stripped: `yarn.cmd` is typed `yarn`, and
/// `yarn.ps1` is not typed at all. On Unix the whole file name is the command — `python3.13` is
/// `python3.13` and stripping at the dot would make it `python3`, which is a different program.
fn command_name(file: &Path) -> Option<String> {
    if !cfg!(windows) {
        return file.to_str().map(str::to_owned);
    }

    let runnable = file
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            RUNNABLE_ON_WINDOWS
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        });

    match runnable {
        true => file.file_stem()?.to_str().map(str::to_owned),
        false => None,
    }
}

/// Every name the artifact's own programs answer to, by key and by file name.
///
/// Both halves, because the two can differ: `provides` publishes `php` and the file inside the
/// archive is `bin/php.exe`, and either spelling turning up in a bindir is the runtime's own
/// program rather than somebody's global install.
fn published_names(provides: &BTreeMap<String, String>) -> BTreeSet<String> {
    let mut names = BTreeSet::new();

    for (key, at) in provides {
        names.insert(key.clone());

        let file = Path::new(at);

        if let Some(name) = file.file_name().and_then(|name| name.to_str()) {
            names.insert(name.to_owned());
        }

        if let Some(stem) = file.file_stem().and_then(|stem| stem.to_str()) {
            names.insert(stem.to_owned());
        }
    }

    names
}

/// Is this name already something else's?
fn is_spoken_for(name: &str, published: &BTreeSet<String>) -> bool {
    let same = |left: &str| match cfg!(windows) {
        true => left.eq_ignore_ascii_case(name),
        false => left == name,
    };

    crate::shims::COMMANDS
        .iter()
        .any(|command| same(command.name))
        || RESERVED.iter().any(|reserved| same(reserved))
        || published.iter().any(|known| same(known))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bindir with these files in it, and the install directory it sits inside.
    fn bindir(kind: RuntimeKind, files: &[&str]) -> (tempfile::TempDir, PathBuf) {
        let install = tempfile::tempdir().expect("a temporary directory");
        let directory = directory(kind, install.path()).expect("a language with a bindir");

        std::fs::create_dir_all(&directory).expect("a bindir");

        for file in files {
            std::fs::write(directory.join(file), b"a program").expect("a file");
        }

        let path = install.path().to_path_buf();
        (install, path)
    }

    fn provides(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(key, at)| ((*key).to_owned(), (*at).to_owned()))
            .collect()
    }

    /// Each language's bindir is where its own package manager puts a program — measured against a
    /// real install rather than reasoned about; see the module note.
    #[test]
    fn each_language_has_its_own_bindir() {
        let install = Path::new("/home/runtimes/node/24.19.0");

        let node = directory(RuntimeKind::Node, install).expect("node has one");
        assert_eq!(
            node,
            if cfg!(windows) {
                install.to_path_buf()
            } else {
                install.join("bin")
            }
        );

        let python = directory(RuntimeKind::Python, install).expect("python has one");
        assert!(python.ends_with(if cfg!(windows) { "Scripts" } else { "bin" }));

        assert_eq!(
            directory(RuntimeKind::Ruby, install).expect("ruby has one"),
            install.join("bin")
        );

        // Composer's global bindir is outside every install, which is a different question — and so
        // is Go's `GOBIN`, which every installed Go shares.
        assert_eq!(directory(RuntimeKind::Php, install), None);
        assert_eq!(directory(RuntimeKind::Go, install), None);
        assert_eq!(directory(RuntimeKind::Java, install), None);
        assert_eq!(directory(RuntimeKind::Composer, install), None);
    }

    /// **The complaint, as a unit.** A tool put in the bindir by `npm install -g` is a command.
    #[test]
    fn a_globally_installed_tool_is_found() {
        let files: &[&str] = match cfg!(windows) {
            true => &["yarn.cmd", "yarn.ps1", "yarn"],
            false => &["yarn"],
        };

        let (_install, path) = bindir(RuntimeKind::Node, files);

        assert_eq!(
            scan(RuntimeKind::Node, &path, &BTreeMap::new()),
            ["yarn".to_owned()].into_iter().collect::<BTreeSet<_>>(),
            "one tool is one command, whatever the package manager spelled it"
        );
    }

    /// A runtime's own programs are not discovered: `node` is a command already, and a second copy
    /// of the shim under that name would dispatch to itself.
    #[test]
    fn a_runtimes_own_programs_are_not_discovered() {
        let files: &[&str] = match cfg!(windows) {
            true => &["node.exe", "yarn.cmd"],
            false => &["node", "yarn"],
        };

        let (_install, path) = bindir(RuntimeKind::Node, files);
        let published = provides(&[(
            "node",
            match cfg!(windows) {
                true => "node.exe",
                false => "bin/node",
            },
        )]);

        assert_eq!(
            scan(RuntimeKind::Node, &path, &published),
            ["yarn".to_owned()].into_iter().collect::<BTreeSet<_>>()
        );
    }

    /// Nor a compiled command, whichever runtime the file turns up in — `npm install -g npm` is a
    /// thing people do.
    #[test]
    fn a_compiled_command_is_never_discovered() {
        let files: &[&str] = match cfg!(windows) {
            true => &["npm.cmd", "npx.cmd", "yarn.cmd"],
            false => &["npm", "npx", "yarn"],
        };

        let (_install, path) = bindir(RuntimeKind::Node, files);

        assert_eq!(
            scan(RuntimeKind::Node, &path, &BTreeMap::new()),
            ["yarn".to_owned()].into_iter().collect::<BTreeSet<_>>()
        );
    }

    /// Nor MixEngine's own binaries, which a person may reasonably have copied anywhere.
    #[test]
    fn mixengines_own_names_are_never_discovered() {
        let files: &[&str] = match cfg!(windows) {
            true => &["mix.exe", "mixengined.exe"],
            false => &["mix", "mixengined"],
        };

        let (_install, path) = bindir(RuntimeKind::Ruby, files);

        assert!(scan(RuntimeKind::Ruby, &path, &BTreeMap::new()).is_empty());
    }

    /// A bindir that is not there is no commands rather than an error: a `Scripts` nothing has ever
    /// installed into is the ordinary state of a fresh Python.
    #[test]
    fn a_bindir_that_is_not_there_is_empty() {
        let install = tempfile::tempdir().expect("a temporary directory");

        assert!(scan(RuntimeKind::Python, install.path(), &BTreeMap::new()).is_empty());
    }

    /// A directory is not a command, however it is spelled — npm's Unix prefix holds
    /// `lib/node_modules` beside the programs.
    #[test]
    fn a_directory_is_not_a_command() {
        let (_install, path) = bindir(RuntimeKind::Ruby, &[]);
        let directory = directory(RuntimeKind::Ruby, &path).expect("ruby has a bindir");
        std::fs::create_dir(directory.join("gems")).expect("a directory inside the bindir");

        assert!(scan(RuntimeKind::Ruby, &path, &BTreeMap::new()).is_empty());
    }

    /// **What counts as a program is this system's own rule.** Windows resolves a bare name by
    /// appending an extension from a fixed list, so a file with any other one is data whatever it
    /// holds — and `rails.ps1` beside `rails.bat` must not become a second command that does
    /// nothing. Unix has no such list and needs none: a bindir holds programs by construction,
    /// which is what *bindir* means to npm, pip and gem alike.
    ///
    /// One test over a `cfg!` rather than two behind `#[cfg]` — see
    /// `crates/mixengine-proto/tests/workspace_layering.rs`, which is what holds that rule.
    #[test]
    fn what_counts_as_a_program_is_this_systems_own_rule() {
        let (_install, path) = bindir(
            RuntimeKind::Ruby,
            &["rails.ps1", "LICENSE", "README.md", "rails.bat"],
        );

        let found = scan(RuntimeKind::Ruby, &path, &BTreeMap::new());

        let expected: BTreeSet<String> = match cfg!(windows) {
            true => ["rails".to_owned()].into_iter().collect(),
            false => [
                "LICENSE".to_owned(),
                "README.md".to_owned(),
                "rails.bat".to_owned(),
                "rails.ps1".to_owned(),
            ]
            .into_iter()
            .collect(),
        };

        assert_eq!(found, expected);
    }

    /// **A name keeps every dot in it.** `python3.13` is a program, and stripping at the dot would
    /// make it `python3`, which is a different one — so the Windows arm takes the *extension* off
    /// and the Unix arm takes nothing off at all.
    #[test]
    fn a_name_keeps_every_dot_that_is_not_an_extension() {
        let file = match cfg!(windows) {
            true => "python3.13.exe",
            false => "python3.13",
        };

        let (_install, path) = bindir(RuntimeKind::Python, &[file]);

        assert_eq!(
            scan(RuntimeKind::Python, &path, &BTreeMap::new()),
            ["python3.13".to_owned()]
                .into_iter()
                .collect::<BTreeSet<_>>()
        );
    }
}
