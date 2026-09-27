//! Which commands `<root>/bin` fronts, and what each one runs.
//!
//! A shim is one binary, copied once per name it answers to, that reads its own file name to find
//! out which command was invoked ([runtime-versions.md](../../../docs/features/runtime-versions.md)).
//! This module is the table that name is looked up in, and it is here rather than in the shim binary
//! for the reason every table like it is in `core`: the process that *fills* `<root>/bin` needs the
//! same list, and two lists would be a `bin/` holding a name nothing dispatches — a program that
//! exists, runs, and refuses to be anything.
//!
//! # A command and an executable are two different names
//!
//! [`Command::name`] is what the user types and what the file in `bin/` is called. `executable` is
//! the key of the artifact's `provides` map, which is **ours rather than the publisher's** — the
//! path inside the archive belongs to whoever packed it, the name it is published under is a
//! convention this project sets, and the index is written to match. That is what lets `python3` and
//! `python` be one program, and `bundler` and `bundle` be one program, without the shim caring which
//! of them a given archive happened to call its file.
//!
//! # What is deliberately not in the table
//!
//! Every tool that is neither inside a language's archive nor published as an artifact of its own.
//! **`composer` used to be the example** — a `.phar` fetched separately, so a row would have been a
//! shim that resolved a PHP correctly and then failed to find a file no artifact contained. T27c
//! made it an artifact of its own kind, and its row is the one with a [`Command::via`]: the kind
//! names the file, `via` names the program that runs it.
//!
//! **Only PHP has artifacts today** (T20a), so the other three rows are unexercised until T27
//! publishes theirs. They are written now because the table is what a shim dispatches on: a row
//! missing when the artifact lands is a `node` in `bin/` that says it is nobody's, and the failure a
//! wrong row produces is one sentence naming what the runtime *does* publish, which is the same
//! sentence a missing row would need anyway.
//!
//! # Filling `bin/` — roadmap task T26
//!
//! [`refresh`] is the other half of the table's reason for being here: one copy of the shim binary
//! per row, under the row's name. It is what turns T25's binary into commands a person can type.
//!
//! **`bin/` is entirely MixEngine's**, which is what lets a refresh remove what it does not
//! recognise. `docs/architecture/overview.md` describes the directory as "version-resolving
//! shims" and nothing else, so a file in there answering to no command is a command that was
//! renamed or dropped between releases — a program that exists, runs, and refuses to be anything.
//! Somebody who wants a script of their own on the PATH has every other directory on the machine to
//! put it in.
//!
//! **It depends on what is installed** — roadmap task T185b, ADR 0057. It used not to: `bin/`
//! held `node` on a machine with no Node.js, on the argument that a shim saying which command to
//! type beats `node: command not found`. What that bought was a `which node` that said yes and a
//! `node` that then refused — and, `bin/` being first on the PATH, a Node the person had installed
//! themselves hidden behind it. A [`COMMANDS`] row is now written only while its kind has a version
//! installed, and the daemon refreshes after every install and uninstall.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use mixengine_platform::handover::RESOLVER_POINTER;
use mixengine_proto::RuntimeKind;

use crate::{Error, Result};

/// One command `<root>/bin` answers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command {
    /// What the user types, and what the shim file in `bin/` is named.
    pub name: &'static str,

    /// Which language's version resolution decides what this runs.
    pub kind: RuntimeKind,

    /// Which of the artifact's executables to run, by the name the index publishes it under.
    pub executable: &'static str,

    /// The kind whose program runs this one, when it is a file rather than a program.
    ///
    /// **`composer` and nothing else** — roadmap task **T27c**, its design's D4. A `.phar` is
    /// handed to a PHP: the command's own kind names the file, this kind names the program, and the
    /// shim resolves both for the directory it was run in. `None` is a program of its own.
    pub via: Option<RuntimeKind>,
}

/// Every command a shim answers to, grouped by language and in the order `bin/` is listed in.
///
/// The set per language is the tools that ship *inside* that language's archive and that a person
/// runs directly. `php-fpm` is absent although PHP ships one: it is a service the daemon supervises
/// with a generated pool config (T28), not a command anybody types in a project directory, and a
/// shim in front of it would be a second way to start one nothing was supervising.
pub const COMMANDS: &[Command] = &[
    // PHP. `pecl` and `pear` are scripts the Unix builds ship and the Windows ones do not, which is
    // not a special case here: an artifact that publishes neither answers the lookup with the list
    // of what it does publish, which is the honest message on a machine where they were never
    // packed.
    Command {
        name: "php",
        kind: RuntimeKind::Php,
        executable: "php",
        via: None,
    },
    Command {
        name: "php-config",
        kind: RuntimeKind::Php,
        executable: "php-config",
        via: None,
    },
    Command {
        name: "phpize",
        kind: RuntimeKind::Php,
        executable: "phpize",
        via: None,
    },
    Command {
        name: "pecl",
        kind: RuntimeKind::Php,
        executable: "pecl",
        via: None,
    },
    Command {
        name: "pear",
        kind: RuntimeKind::Php,
        executable: "pear",
        via: None,
    },
    // Node.
    Command {
        name: "node",
        kind: RuntimeKind::Node,
        executable: "node",
        via: None,
    },
    Command {
        name: "npm",
        kind: RuntimeKind::Node,
        executable: "npm",
        via: None,
    },
    Command {
        name: "npx",
        kind: RuntimeKind::Node,
        executable: "npx",
        via: None,
    },
    Command {
        name: "corepack",
        kind: RuntimeKind::Node,
        executable: "corepack",
        via: None,
    },
    // Python. `python3` and `pip3` are the same programs under the names most projects' scripts
    // actually call, which is the whole reason a command and an executable are separate fields.
    Command {
        name: "python",
        kind: RuntimeKind::Python,
        executable: "python",
        via: None,
    },
    Command {
        name: "python3",
        kind: RuntimeKind::Python,
        executable: "python",
        via: None,
    },
    Command {
        name: "pip",
        kind: RuntimeKind::Python,
        executable: "pip",
        via: None,
    },
    Command {
        name: "pip3",
        kind: RuntimeKind::Python,
        executable: "pip",
        via: None,
    },
    // Ruby.
    Command {
        name: "ruby",
        kind: RuntimeKind::Ruby,
        executable: "ruby",
        via: None,
    },
    Command {
        name: "gem",
        kind: RuntimeKind::Ruby,
        executable: "gem",
        via: None,
    },
    Command {
        name: "bundle",
        kind: RuntimeKind::Ruby,
        executable: "bundle",
        via: None,
    },
    Command {
        name: "bundler",
        kind: RuntimeKind::Ruby,
        executable: "bundle",
        via: None,
    },
    Command {
        name: "rake",
        kind: RuntimeKind::Ruby,
        executable: "rake",
        via: None,
    },
    Command {
        name: "irb",
        kind: RuntimeKind::Ruby,
        executable: "irb",
        via: None,
    },
    // Go (T27d). `gofmt` is typed, and called by editors, by name; the binaries under `pkg/tool/`
    // are not, and `go` finds them from its own `GOROOT`.
    Command {
        name: "go",
        kind: RuntimeKind::Go,
        executable: "go",
        via: None,
    },
    Command {
        name: "gofmt",
        kind: RuntimeKind::Go,
        executable: "gofmt",
        via: None,
    },
    // Java (T27e): exactly the keys of the artifact's `provides`. The JDK's other tools are reached
    // through PATH, whose first entry under a shim is that JDK's `bin/`.
    Command {
        name: "java",
        kind: RuntimeKind::Java,
        executable: "java",
        via: None,
    },
    Command {
        name: "javac",
        kind: RuntimeKind::Java,
        executable: "javac",
        via: None,
    },
    Command {
        name: "jar",
        kind: RuntimeKind::Java,
        executable: "jar",
        via: None,
    },
    Command {
        name: "jshell",
        kind: RuntimeKind::Java,
        executable: "jshell",
        via: None,
    },
    Command {
        name: "keytool",
        kind: RuntimeKind::Java,
        executable: "keytool",
        via: None,
    },
    Command {
        name: "jlink",
        kind: RuntimeKind::Java,
        executable: "jlink",
        via: None,
    },
    // Composer. A file and not a program: `composer.phar`, run by the PHP the directory resolves
    // to (T27c). Its own row so that a version of *Composer* is pinned and defaulted like any
    // runtime's, and `via` so that the shim knows whose program to start.
    Command {
        name: "composer",
        kind: RuntimeKind::Composer,
        executable: "composer",
        via: Some(RuntimeKind::Php),
    },
];

/// Which command a program invoked at this path is being asked to be.
///
/// `argv[0]` is the whole input, because a shim has no arguments of its own — every one of them
/// belongs to the program it fronts, and a `--home` flag here would be a flag `php` could never
/// receive. What is read off it is the file name with any executable suffix removed.
///
/// **The comparison is case-insensitive on Windows and not on Unix**, which is the filesystem's own
/// rule rather than a courtesy: `PHP.EXE` and `php.exe` are one file there and two files here, so
/// folding case on Unix would let a program genuinely called `PHP` be dispatched as `php`.
///
/// [`None`] for a name the table does not hold — including `mixengine-shim` itself, which is what
/// the binary is called before it is copied into `bin/` under a name that means something.
#[must_use]
pub fn dispatch(invoked_as: &Path) -> Option<&'static Command> {
    let stem = invoked_as.file_stem()?.to_str()?;

    COMMANDS.iter().find(|command| {
        if cfg!(windows) {
            command.name.eq_ignore_ascii_case(stem)
        } else {
            command.name == stem
        }
    })
}

/// The file name a shim answering to `command` has in `bin/` on this operating system.
///
/// The suffix is the loader's rule and not a convention: Windows resolves a bare `php` typed at a
/// prompt by appending `.exe`, so a copy without one is a file nothing can run.
#[must_use]
pub fn file_name(command: &Command) -> String {
    format!("{}{}", command.name, std::env::consts::EXE_SUFFIX)
}

/// The name a copy that could not be replaced is moved aside under.
///
/// Windows will not overwrite a running executable, and a `php -S` somebody left in another
/// terminal is exactly that. It *will* let the file be renamed out of the way while it runs, which
/// is the only way to put a new one in its place — and the moved copy keeps working for the process
/// that is holding it until that process exits.
pub const MOVED_ASIDE: &str = ".mixengine-replaced";

/// A command `bin/` fronts that [`COMMANDS`] does not name — roadmap tasks **T130** and **T131**.
///
/// **The caller decides these and this module only copies them**, which is the whole shape of the
/// change: an extra is read out of the database — an installed package's client commands, a tool
/// found inside an installed runtime — and `core` is where the tables are but the daemon is the only
/// thing that may open the file. A shim binary is a shim binary whatever name it wears, so what
/// arrives here is a list of names and a note about where each came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extra {
    /// The command as it is typed, without the platform's executable suffix.
    pub name: String,

    /// What put it there. Carried so that a listing can say why a name is on the PATH, and so that
    /// a copy left behind by an uninstall can be told from one this build simply does not know.
    pub origin: Origin,
}

/// Where an [`Extra`] came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// A client command of an installed service package — [`crate::generate::recipe::ClientCommand`].
    Client {
        /// The `packages.name` whose recipe declared it.
        package: String,
    },

    /// A tool somebody installed into a runtime, found in that runtime's global directory.
    Global {
        /// Which language's version resolution decides which copy of it runs.
        kind: RuntimeKind,
    },
}

/// A name more than one installed package claimed, and which of them `bin/` gave it to.
///
/// **Resolved by [`resolve_claims`] and reported rather than hidden**: somebody who typed `mysql`
/// and reached MariaDB's client has to be able to find that out without reading `--version`, and
/// `mix doctor` is where it is said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// The contested command.
    pub name: String,

    /// The package whose program `bin/` fronts under it.
    pub won: String,

    /// The packages that also claimed it, in the order the tie-break rejected them.
    pub lost: Vec<String>,
}

/// One package's claim on one name, as the daemon read it off the installed rows.
///
/// The two booleans are the middle terms of the order in [`resolve_claims`], computed once by the
/// caller because both of them cost a query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claimed {
    /// The contested command.
    pub name: String,

    /// The `packages.name` claiming it.
    pub package: String,

    /// How strongly.
    pub claim: crate::generate::recipe::Claim,

    /// Whether this package has any instance at all.
    pub has_instance: bool,

    /// Whether one of those instances holds the port the product is documented under.
    pub on_preferred_port: bool,
}

/// Give every contested name to exactly one package — roadmap task **T130**, the design's §A.4.
///
/// **A total order, so that the answer is the same on every machine and on every run.** Never "give
/// it to neither": a name a person expects and cannot type is the complaint this whole task opens
/// with, and an arbitrary-but-stable winner that `mix doctor` names is strictly better than a hole.
///
/// 1. [`Claim::Own`](crate::generate::recipe::Claim::Own) beats
///    [`Alias`](crate::generate::recipe::Claim::Alias). MariaDB's `mysql` disappears the moment the
///    MySQL package is installed, which is the only conflict this build can actually produce.
/// 2. A package with an instance beats one without — the product somebody is running is the one
///    they meant.
/// 3. Then the instance on the product's own documented port, for
///    [`crate::services::client`]'s reason.
/// 4. Then the package name ascending: arbitrary, and stable, which is the only property left that
///    matters.
#[must_use]
pub fn resolve_claims(claims: &[Claimed]) -> (Vec<Extra>, Vec<Conflict>) {
    let mut by_name: std::collections::BTreeMap<String, Vec<&Claimed>> =
        std::collections::BTreeMap::new();

    for claim in claims {
        by_name.entry(fold(&claim.name)).or_default().push(claim);
    }

    let mut extras = Vec::new();
    let mut conflicts = Vec::new();

    for contenders in by_name.into_values() {
        let mut ordered = contenders;

        // Descending: the first element is the winner. `Reverse` on the name so that a *smaller*
        // package name sorts first once every other term is equal.
        ordered.sort_by_key(|claimed| {
            (
                std::cmp::Reverse(claimed.claim),
                std::cmp::Reverse(claimed.has_instance),
                std::cmp::Reverse(claimed.on_preferred_port),
                claimed.package.clone(),
            )
        });

        let won = ordered.first().expect("a group is never empty");

        extras.push(Extra {
            name: won.name.clone(),
            origin: Origin::Client {
                package: won.package.clone(),
            },
        });

        if ordered.len() > 1 {
            conflicts.push(Conflict {
                name: won.name.clone(),
                won: won.package.clone(),
                lost: ordered[1..]
                    .iter()
                    .map(|claimed| claimed.package.clone())
                    .collect(),
            });
        }
    }

    (extras, conflicts)
}

/// What one [`refresh`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Refreshed {
    /// Every command `bin/` now answers to, in [`COMMANDS`]' order.
    pub commands: Vec<String>,

    /// The ones this call put there, because they were missing or were a different build.
    ///
    /// Empty on the ordinary start, which is the point of comparing before writing: a daemon
    /// restarted twenty times a day must not rewrite nineteen megabytes each time. What the *first*
    /// start of a home costs is `place`'s question rather than this one's.
    pub written: Vec<String>,

    /// What was in `bin/` that no command answers to, and was removed.
    pub removed: Vec<String>,

    /// What was in `bin/` that no command answers to, and could **not** be removed.
    ///
    /// Reported rather than swallowed: a leftover here is a name on the user's PATH that runs
    /// something MixEngine no longer understands, and a refresh that said nothing about it would be
    /// claiming a `bin/` it had not achieved.
    pub refused: Vec<String>,

    /// Names more than one installed package claimed, and who got each — roadmap task **T130**.
    ///
    /// **Filled by the caller and carried here**, because the conflict is settled before the copy
    /// happens: [`resolve_claims`] answers it out of the database, and one value describing one pass
    /// is better than a caller having to keep two beside each other.
    pub conflicts: Vec<Conflict>,
}

/// The file name of the shim binary, without the platform's executable suffix.
///
/// A constant rather than a literal inside [`source`] because it is also **what every release
/// artifact has to contain**: a stage without this name in it is an install whose `bin/` is empty
/// and whose every runtime command is missing. `crates/mixengine-core/tests/packaging.rs` asserts
/// `packaging/common.sh` ships it, which is roadmap task T85c not happening twice.
pub const BINARY: &str = "mixengine-shim";

/// The file `<root>/bin` holds under every name on Windows — roadmap task **T185**. See [`Source`].
///
/// Shipped beside [`BINARY`] and held to `packaging/common.sh` by the same test: a Windows install
/// without it falls back to copying the shim itself into `bin/`, which works and weighs what T185
/// set out to remove.
pub const TRAMPOLINE: &str = "mixengine-trampoline";

/// What [`refresh`] fills `bin/` from.
#[derive(Debug, Clone)]
pub struct Source {
    /// `mixengine-shim`, which resolves.
    pub resolver: PathBuf,

    /// What goes into `bin/` under every name: [`TRAMPOLINE`] on Windows, where a shim outlives
    /// the program it starts and every name must be a file of its own (see `link`), and the
    /// resolver itself elsewhere, where it `exec`s away and one file linked under every name is
    /// enough.
    pub placed: PathBuf,
}

/// Where the shim binary is, given the program that is asking — and, on Windows, the trampoline
/// beside it (T185).
///
/// It sits beside whatever is running — `mixengined` in an install, and the same `target/debug` in
/// a development tree — because a release ships the two next to each other and there is nothing
/// else to look at: a `PATH` search would find the *copy in `bin/`* on a machine where the PATH is
/// already set up, and copying a shim from `bin/` into `bin/` would make an upgrade a no-op.
///
/// # Errors
///
/// [`Error::ShimMissing`] when the shim is not there, which is a broken installation rather than
/// anything a user did. A missing trampoline is not an error: see the fallback below.
pub fn source(program: &Path) -> Result<Source> {
    let beside = program.parent().unwrap_or_else(|| Path::new("."));
    let resolver = present(beside, BINARY)?;

    // The same constant `link` reads, for the same reason: see [`Source::placed`].
    //
    // **Without a trampoline, the shim itself**, as before T185. An install that updated itself
    // onto this release has none: `updates::apply::swap` never adds a binary the install lacked. A
    // `bin/` of full-size shim copies is heavy and works; refusing to fill it would leave every
    // command missing. The next full install brings the trampoline and the next start moves to it.
    let placed = match cfg!(windows) {
        true => present(beside, TRAMPOLINE).unwrap_or_else(|_| resolver.clone()),
        false => resolver.clone(),
    };

    Ok(Source { resolver, placed })
}

/// `name` in `directory`, with this platform's executable suffix, if it is a file.
fn present(directory: &Path, name: &str) -> Result<PathBuf> {
    let file = directory.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));

    match file.is_file() {
        true => Ok(file),
        false => Err(Error::ShimMissing { path: file }),
    }
}

/// Put one copy of [`Source::placed`] in `bin` for every command in [`COMMANDS`], and remove what
/// is not one.
///
/// Idempotent, and cheap when there is nothing to do: a copy whose length matches the source's and
/// whose modification time is not older is left alone, so the common case is a stat per command and
/// no bytes moved. An upgrade replaces the source binary with one that is a different length or
/// newer than the copies, and every copy is rewritten.
///
/// **The pass that is *not* idempotent — the first one, on a home that has never had a daemon — is
/// the expensive one, and `place` is where that cost is paid or avoided.** Nineteen names is a hard
/// link apiece where the filesystem gives one file a second name, and nineteen copies where it does
/// not — which on Windows is always, for the reason stated there. Since T185 what is copied there is
/// the trampoline, a few hundred KB, and `bin/` also holds [`RESOLVER_POINTER`]: the one line that
/// tells each trampoline where the resolver is.
///
/// **Not a transaction, and it cannot be one**: nineteen files cannot be renamed into place at
/// once. What that costs is bounded by the fact that every copy is the *same program* — a `bin/`
/// half written by an upgrade holds some new shims and some old ones, and both dispatch on their own
/// file name and resolve against the same database.
///
/// # Errors
///
/// [`Error::Io`] naming the file that could not be written. Failing to *remove* a stranger is not
/// one — it lands in [`Refreshed::refused`] — because a directory that has what it should have is
/// working, and refusing to start over a file nobody can delete would be worse than saying so.
pub fn refresh(
    bin: &Path,
    source: &Source,
    installed: &std::collections::BTreeSet<RuntimeKind>,
    extra: &[Extra],
) -> Result<Refreshed> {
    crate::paths::create_dir(bin)?;

    let mut refreshed = Refreshed::default();

    // **[`COMMANDS`] first and the extras after it**, which is the one rule that keeps this
    // composable: a discovered `npm` — an `npm` somebody installed globally into a Node, which does
    // happen — must not displace the compiled row, or `bin/npm` would be a shim that dispatches to
    // a file found by a shim that dispatches to a file. A name already spoken for is dropped here
    // rather than refused, because the caller's list is a description of a disk and not a request.
    // **Only what is installed** — roadmap task T185b. A `node` in `bin/` on a machine with no
    // Node.js made `which node` say yes and then refused, and — `bin/` being first on the PATH —
    // hid a Node the person had installed themselves. A row fronted by another kind (`composer`,
    // run by PHP) needs both.
    let mut names: Vec<String> = COMMANDS
        .iter()
        .filter(|command| {
            installed.contains(&command.kind)
                && command.via.is_none_or(|via| installed.contains(&via))
        })
        .map(file_name)
        .collect();
    let mut expected: HashSet<String> = names.iter().map(|name| fold(name)).collect();

    // T185: the trampolines' way to the resolver is not a stranger to sweep.
    if cfg!(windows) {
        expected.insert(fold(RESOLVER_POINTER));
    }

    for extra in extra {
        let name = format!("{}{}", extra.name, std::env::consts::EXE_SUFFIX);

        if expected.insert(fold(&name)) {
            names.push(name);
        }
    }

    // Swept **before** the copies rather than after, so that a name which moved from one command to
    // another — a row renamed between releases, a runtime uninstalled since the last pass — is
    // removed and then written afresh rather than removed a moment after being put there.
    sweep(bin, &expected, &mut refreshed);

    for name in names {
        let target = bin.join(&name);

        if place(&source.placed, &target)? {
            refreshed.written.push(name.clone());
        }

        refreshed.commands.push(name);
    }

    // T185: only on Windows is there a trampoline to read it.
    if cfg!(windows) {
        point_at(bin, &source.resolver)?;
    }

    Ok(refreshed)
}

/// Every runtime kind — what a caller passes to [`refresh`] to front every row, as a refresh did
/// before roadmap task **T185b**.
#[must_use]
pub fn every_kind() -> std::collections::BTreeSet<RuntimeKind> {
    RuntimeKind::ALL.into_iter().collect()
}

/// Write `bin/mixengine-shim.path` when it does not already say `resolver`.
///
/// Written beside and renamed over, so a trampoline starting at that moment reads the old line or
/// the new one and never half of either. UTF-8 and nothing else, because the trampoline reads it
/// with nothing but the standard library: an install directory whose path is not Unicode is refused
/// here, by name, rather than turned into a line that points nowhere.
fn point_at(bin: &Path, resolver: &Path) -> Result<()> {
    let pointer = bin.join(RESOLVER_POINTER);
    let io = |source| Error::Io {
        action: "write where the shim is to",
        path: pointer.clone(),
        source,
    };

    let said = resolver.to_str().ok_or_else(|| {
        io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "the install directory's path is not Unicode",
        ))
    })?;

    if std::fs::read_to_string(&pointer).is_ok_and(|there| there == said) {
        return Ok(());
    }

    let staged = bin.join(format!("{RESOLVER_POINTER}.new"));
    std::fs::write(&staged, said).map_err(io)?;
    std::fs::rename(&staged, &pointer).map_err(io)
}

/// Take `bin` back to nothing, which is what an uninstall of the whole home does first.
///
/// Its own function rather than `refresh` against an empty table, because the two disagree about
/// what a failure means: a copy that cannot be removed here is reported and the rest still go, so
/// that one file held open by a shell somebody forgot about does not leave eighteen others behind.
///
/// # Errors
///
/// [`Error::Io`] only when `bin` itself cannot be listed. Everything else is in
/// [`Refreshed::refused`].
pub fn clear(bin: &Path) -> Result<Refreshed> {
    let mut refreshed = Refreshed::default();

    if bin.is_dir() {
        sweep(bin, &HashSet::new(), &mut refreshed);
    }

    Ok(refreshed)
}

/// Remove everything in `bin` that is not one of `expected`.
///
/// Best effort by design — see [`refresh`]'s own note — and silent about the copies it moved aside
/// itself: one of those is a shim a running process is still holding, and reporting it as a
/// stranger every start would make an ordinary Windows situation look like a fault.
fn sweep(bin: &Path, expected: &HashSet<String>, refreshed: &mut Refreshed) {
    let Ok(entries) = std::fs::read_dir(bin) else {
        return;
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();

        if expected.contains(&fold(&name)) {
            continue;
        }

        let removed = std::fs::remove_file(entry.path()).is_ok();

        // The pointer, and a staged one a crash left behind (T185), are MixEngine's own and not
        // commands: removing them is `clear` doing its job, not something to report.
        if name.ends_with(MOVED_ASIDE) || name.starts_with(RESOLVER_POINTER) {
            continue;
        }

        match removed {
            true => refreshed.removed.push(name),
            false => refreshed.refused.push(name),
        }
    }
}

/// Put `shim` at `target` unless what is there is already this build. `true` when it wrote.
///
/// **A second name for the shim wherever the filesystem will give it one, and a second set of bytes
/// only where it will not.** The bytes are the whole cost of filling `bin/`, and paying them
/// nineteen times per home is not a cost this has to meet: a hard link is one directory entry, and
/// every property `refresh` relies on survives it — [`is_current`] compares length and modification
/// time, which a link shares with the file it names, so the next start still writes nothing; a build
/// that *replaces* the shim binary leaves the links on the file they were made from, which is older
/// or a different length, and every one of them is replaced.
///
/// It is also what stops a debug build from being pathological. A `mixengine-shim` with its debug
/// info in it is tens of megabytes, so a start used to move most of a gigabyte before it bound its
/// endpoint — and a test suite that gives every test a home of its own paid that per test, which is
/// what made four daemons on one CI runner take thirty seconds each to answer.
fn place(shim: &Path, target: &Path) -> Result<bool> {
    if is_current(shim, target) {
        return Ok(false);
    }

    if link(shim, target) {
        return Ok(true);
    }

    let io = |source| Error::Io {
        action: "install the shim at",
        path: target.to_path_buf(),
        source,
    };

    match std::fs::copy(shim, target) {
        Ok(_) => Ok(true),

        // The Windows case: something is running this copy. Renaming it away is allowed while a
        // process holds it, and the process keeps running the file it opened — so the moved copy is
        // rubbish the next sweep collects rather than something anybody still needs.
        Err(_) if target.exists() => {
            let aside = target.with_file_name(format!(
                "{}{MOVED_ASIDE}",
                target.file_name().unwrap_or_default().to_string_lossy()
            ));

            // A copy moved aside by an earlier attempt and never collected. Removed first, because
            // a rename onto an existing file fails on Windows.
            let _ = std::fs::remove_file(&aside);
            std::fs::rename(target, &aside).map_err(io)?;

            match std::fs::copy(shim, target) {
                Ok(_) => Ok(true),

                // Put back what was there. The command keeps working as the build it was, which is
                // a great deal better than a name on the PATH with no file behind it.
                Err(source) => {
                    let _ = std::fs::rename(&aside, target);
                    Err(io(source))
                }
            }
        }

        Err(source) => Err(io(source)),
    }
}

/// Give the shim a second name at `target`, and say whether the system allowed it.
///
/// Never an error: every way of failing has the same answer, which is to copy the bytes instead —
/// a `bin/` on a different filesystem from the install, a filesystem with no links in it, a
/// permission a link needs and a write does not. The caller's next line is the copy, so a `false`
/// here costs one failed syscall and nothing else.
///
/// # Not on Windows, and it is the shim's own behaviour that decides it
///
/// A link is the same file under two names, so whatever holds one holds both — and a Windows shim
/// **outlives the program it starts**: it stays as the parent of a Job Object child (see
/// `mixengine-shim`) rather than `exec`ing away as it does on Unix. So a `php -S` somebody left
/// running would hold `mixengine-shim.exe` itself open for hours, and the next upgrade — or the next
/// `cargo build` in this tree — would meet a sharing violation on a file it has every right to
/// replace. [`place`]'s existing dance moves a *copy* aside for exactly that case and cannot move
/// aside a file that is the source. On Unix the same shim has `exec`ed into PHP microseconds after
/// it started, and the file it came from is nobody's any more.
///
/// The `cfg!` is the same one [`dispatch`] and [`fold`] use: a constant this module reads, not a
/// call into the operating system — `docs/architecture/platform-abstraction.md` draws that line
/// at behaviour a trait can be written for, and "does a running program hold its own file" is not
/// something either side of `bin/` can be asked.
fn link(shim: &Path, target: &Path) -> bool {
    if cfg!(windows) {
        return false;
    }

    if std::fs::hard_link(shim, target).is_ok() {
        return true;
    }

    // A link refuses an existing name, where a copy overwrites one — so the file that is there is
    // unlinked and the link tried once more. It is `place`'s own case: something is at `target` and
    // [`is_current`] has just said it is not this build. Unlinking it on Unix takes the name and
    // leaves the file to whatever process is still running it, which is the property this whole
    // function rests on.
    if !target.exists() || std::fs::remove_file(target).is_err() {
        return false;
    }

    std::fs::hard_link(shim, target).is_ok()
}

/// Is the copy at `target` the same build as `shim`?
///
/// Length and modification time rather than the bytes: this runs once per command on every daemon
/// start, and hashing nineteen copies of a multi-megabyte binary to discover that nothing changed
/// would be the most expensive thing a start does. `>=` and not `==` because a copy is stamped when
/// it is made, which is after the source it came from — and because a link ([`link`]) is the *same*
/// file, so the two times it compares are one time and the answer has to be yes.
fn is_current(shim: &Path, target: &Path) -> bool {
    let (Ok(source), Ok(copy)) = (shim.metadata(), target.metadata()) else {
        return false;
    };

    if source.len() != copy.len() {
        return false;
    }

    match (source.modified(), copy.modified()) {
        (Ok(source), Ok(copy)) => copy >= source,
        // A filesystem that will not say. Copying is the safe answer: the cost is one write per
        // start, and the alternative is a shim that is never upgraded.
        _ => false,
    }
}

/// A file name as this filesystem compares one — [`dispatch`]'s rule, applied to `bin/` itself.
fn fold(name: &str) -> String {
    match cfg!(windows) {
        true => name.to_ascii_lowercase(),
        false => name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shim_knows_which_command_it_was_copied_to_be() {
        let bin = Path::new(if cfg!(windows) {
            r"C:\Users\someone\AppData\Local\MixEngine\bin"
        } else {
            "/home/someone/.local/share/mixengine/bin"
        });

        let php = dispatch(&bin.join(format!("php{}", std::env::consts::EXE_SUFFIX)))
            .expect("php is a command");
        assert_eq!(php.kind, RuntimeKind::Php);
        assert_eq!(php.executable, "php");

        // A name of the user's world that is a different name in the artifact's, which is the pair
        // of fields' whole reason.
        let python3 = dispatch(&bin.join("python3")).expect("python3 is a command");
        assert_eq!(python3.executable, "python");
        assert_eq!(python3.kind, RuntimeKind::Python);

        // The binary before it is copied into `bin/` under a name that means something.
        assert_eq!(dispatch(Path::new("mixengine-shim")), None);
        assert_eq!(dispatch(Path::new("cargo")), None, "not in any artifact");
    }

    /// The filesystem's rule, not a courtesy — see [`dispatch`].
    #[test]
    fn case_is_folded_exactly_where_the_filesystem_folds_it() {
        assert_eq!(dispatch(Path::new("PHP")).is_some(), cfg!(windows));
    }

    /// **One row runs through another kind** — roadmap task **T27c**, its design's D4. `composer`
    /// is a file for a PHP, and every other row is a program of its own.
    #[test]
    fn only_composer_runs_through_another_kind() {
        for command in COMMANDS {
            match command.name {
                "composer" => {
                    assert_eq!(command.kind, RuntimeKind::Composer);
                    assert_eq!(command.executable, "composer");
                    assert_eq!(command.via, Some(RuntimeKind::Php));
                }
                _ => assert_eq!(command.via, None, "{}", command.name),
            }
        }
    }

    /// Go's two commands — roadmap task **T27d**, its design's D4.
    #[test]
    fn go_fronts_go_and_gofmt() {
        for name in ["go", "gofmt"] {
            let command =
                dispatch(Path::new(name)).unwrap_or_else(|| panic!("{name} is a command"));

            assert_eq!(command.kind, RuntimeKind::Go);
            assert_eq!(command.executable, name);
            assert_eq!(command.via, None);
        }
    }

    /// Java's six commands — roadmap task **T27e**, its design's D4.
    #[test]
    fn java_fronts_the_six_commands_its_artifact_provides() {
        for name in ["java", "javac", "jar", "jshell", "keytool", "jlink"] {
            let command =
                dispatch(Path::new(name)).unwrap_or_else(|| panic!("{name} is a command"));

            assert_eq!(command.kind, RuntimeKind::Java);
            assert_eq!(command.executable, name);
            assert_eq!(command.via, None);
        }
    }

    /// Two rows with one name would make `bin/` a directory whose entries are decided by the order
    /// of this table, which is not a thing anybody should have to know.
    #[test]
    fn no_two_commands_answer_to_the_same_name() {
        let mut names: Vec<&str> = COMMANDS.iter().map(|command| command.name).collect();
        names.sort_unstable();

        let mut unique = names.clone();
        unique.dedup();

        assert_eq!(names, unique, "a name is listed twice");
    }

    /// Every name has to be a filename on all three systems, since `bin/` is where it lands.
    #[test]
    fn every_command_is_a_name_a_file_can_have() {
        for command in COMMANDS {
            assert!(
                !command.name.is_empty()
                    && command
                        .name
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{} is not a name to put in bin/",
                command.name
            );
        }
    }
}

#[cfg(test)]
mod claim_tests {
    use super::*;

    use crate::generate::recipe::Claim;

    fn claimed(name: &str, package: &str, claim: Claim) -> Claimed {
        Claimed {
            name: name.to_owned(),
            package: package.to_owned(),
            claim,
            has_instance: false,
            on_preferred_port: false,
        }
    }

    fn running(name: &str, package: &str, claim: Claim, on_preferred_port: bool) -> Claimed {
        Claimed {
            has_instance: true,
            on_preferred_port,
            ..claimed(name, package, claim)
        }
    }

    fn won(claims: &[Claimed], name: &str) -> String {
        let (_, conflicts) = resolve_claims(claims);

        conflicts
            .iter()
            .find(|conflict| conflict.name == name)
            .map(|conflict| conflict.won.clone())
            .expect("a conflict over this name")
    }

    /// **A real name beats a compatibility one**, which is the only conflict this build can produce:
    /// MariaDB's `mysql` disappears the moment somebody installs the product that owns the name.
    #[test]
    fn a_real_name_beats_a_compatibility_one() {
        // MariaDB is running on 3306 and MySQL is not installed anywhere near a port, so every
        // other term of the order points the other way. The claim still decides.
        let claims = [
            running("mysql", "mariadb", Claim::Alias, true),
            claimed("mysql", "mysql", Claim::Own),
        ];

        assert_eq!(won(&claims, "mysql"), "mysql");
    }

    /// Two real claims — a MariaDB 10.x archive still ships a `mysql` of its own — are settled by
    /// the instance: the product somebody is actually running is the one they meant.
    #[test]
    fn two_real_claims_are_settled_by_the_instance() {
        let claims = [
            claimed("mysql", "mariadb", Claim::Own),
            running("mysql", "mysql", Claim::Own, false),
        ];

        assert_eq!(won(&claims, "mysql"), "mysql");
    }

    /// Both running, and the product's own documented port decides — the same rule
    /// [`crate::services::client`] uses, for the same reason.
    #[test]
    fn both_running_and_the_documented_port_decides() {
        let claims = [
            running("mysql", "mariadb", Claim::Own, true),
            running("mysql", "mysql", Claim::Own, false),
        ];

        assert_eq!(won(&claims, "mysql"), "mariadb");
    }

    /// Still tied, and the answer is the package name ascending: arbitrary, and the same on every
    /// machine and on every run, which is the only property left that matters.
    #[test]
    fn a_total_tie_ends_in_the_package_name() {
        let claims = [
            running("mysql", "mysql", Claim::Own, true),
            running("mysql", "mariadb", Claim::Own, true),
        ];

        assert_eq!(won(&claims, "mysql"), "mariadb");
    }

    /// **The losers are named rather than dropped.** Somebody who typed `mysql` and reached the
    /// other product's client has to be able to find that out without reading `--version`.
    #[test]
    fn the_losers_are_named() {
        let claims = [
            claimed("mysql", "mariadb", Claim::Alias),
            claimed("mysql", "mysql", Claim::Own),
        ];

        let (extras, conflicts) = resolve_claims(&claims);

        assert_eq!(
            extras,
            vec![Extra {
                name: "mysql".to_owned(),
                origin: Origin::Client {
                    package: "mysql".to_owned()
                },
            }]
        );
        assert_eq!(
            conflicts,
            vec![Conflict {
                name: "mysql".to_owned(),
                won: "mysql".to_owned(),
                lost: vec!["mariadb".to_owned()],
            }]
        );
    }

    /// A name only one package claims is not a conflict, and saying so would put every command in
    /// `bin/` into a report meant for the two that are contested.
    #[test]
    fn an_uncontested_name_is_not_a_conflict() {
        let claims = [
            claimed("psql", "postgres", Claim::Own),
            claimed("redis-cli", "redis", Claim::Own),
        ];

        let (extras, conflicts) = resolve_claims(&claims);

        assert_eq!(extras.len(), 2);
        assert!(conflicts.is_empty());
    }
}
