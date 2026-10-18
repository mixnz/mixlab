//! `[paths]`: where the directories that grow without bound are relocated to, and how a relocation
//! is written and checked — roadmap task **T143**.

use std::io::Write as _;
use std::path::{Component, Path, PathBuf, Prefix};

use serde::{Deserialize, Deserializer};

use crate::{Error, Result};

/// Relocations for the directories that grow without bound.
///
/// Only these four: runtimes and packages are re-downloadable, `data/` and `logs/` are the two that
/// get large. `bin/`, `etc/`, `certs/`, `extensions/`, `blueprints/`, `run/` and `mixengine.db`
/// stay together next to `config.toml`, because a home directory that can be split into eleven
/// pieces is a home directory no uninstaller can promise to remove.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PathOverrides {
    /// Where installed language runtimes go instead of `<root>/runtimes`.
    #[serde(default, deserialize_with = "relocation")]
    pub runtimes: Option<PathBuf>,
    /// Where installed servers and databases go instead of `<root>/packages`.
    #[serde(default, deserialize_with = "relocation")]
    pub packages: Option<PathBuf>,
    /// Where service data goes instead of `<root>/data`.
    #[serde(default, deserialize_with = "relocation")]
    pub data: Option<PathBuf>,
    /// Where logs go instead of `<root>/logs`.
    #[serde(default, deserialize_with = "relocation")]
    pub logs: Option<PathBuf>,
}

impl PathOverrides {
    /// The four keys and what each is set to, in the order the template lists them.
    ///
    /// **One list, walked by everything that has to treat the four alike** — roadmap task **T143**.
    /// The alternative is four copies of the same `match`, and the fifth key somebody adds later
    /// reaching three of them.
    #[must_use]
    pub fn entries(&self) -> [(&'static str, Option<&Path>); 4] {
        [
            ("runtimes", self.runtimes.as_deref()),
            ("packages", self.packages.as_deref()),
            ("data", self.data.as_deref()),
            ("logs", self.logs.as_deref()),
        ]
    }
}

/// What a caller asked to relocate — roadmap task **T143**.
///
/// [`PathOverrides`]'s shape without its serde, because the two are different statements: that one
/// is *what the file says*, and this is *what somebody just asked for*. A key left [`None`] here is
/// a key nobody mentioned, and [`set_paths`] leaves it exactly as the file has it — which is what
/// makes `--data` on its own a request about `data/` and about nothing else.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestedPaths {
    /// Where installed language runtimes should go.
    pub runtimes: Option<PathBuf>,
    /// Where installed servers and databases should go.
    pub packages: Option<PathBuf>,
    /// Where service data should go.
    pub data: Option<PathBuf>,
    /// Where logs should go.
    pub logs: Option<PathBuf>,
}

impl RequestedPaths {
    /// The four keys and what each was asked for. [`PathOverrides::entries`]'s shape.
    #[must_use]
    pub fn entries(&self) -> [(&'static str, Option<&Path>); 4] {
        [
            ("runtimes", self.runtimes.as_deref()),
            ("packages", self.packages.as_deref()),
            ("data", self.data.as_deref()),
            ("logs", self.logs.as_deref()),
        ]
    }

    /// Did anybody ask for anything?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries().iter().all(|(_, path)| path.is_none())
    }

    /// Which of the requested keys say something `current` does not already say.
    ///
    /// **An empty answer is the whole of the silent no-op.** A daemon started from a launchd plist
    /// or a shell alias carries the same four flags for the life of that plist; asking for what the
    /// file already says has to be a start that writes nothing and complains about nothing, or the
    /// flag becomes a thing that works once.
    ///
    /// Compared as written rather than as resolved: `bulk/data` and an absolute path to the same
    /// directory are two different answers to *"what should the file say"*, and this function is
    /// only ever asked in order to decide whether to write one.
    #[must_use]
    pub fn differing_from(&self, current: &PathOverrides) -> Vec<&'static str> {
        let held = current.entries();

        self.entries()
            .into_iter()
            .zip(held)
            .filter_map(|((key, asked), (_, have))| {
                let asked = asked?;

                (Some(asked) != have).then_some(key)
            })
            .collect()
    }
}

/// Set the named keys in `[paths]`, leaving every comment where it was — roadmap task **T143**.
///
/// **A `toml_edit` document and not a re-serialised [`Config`]**, for the reason
/// [`crate::manifest::write`] gives about a project manifest and which holds twice over here: this
/// file ships with sixty lines of comments explaining the very keys being written, [`TEMPLATE`]
/// puts them there on first run, and a writer that round-tripped the parsed value would throw all
/// of them away the first time anybody chose a disk.
///
/// **The commented examples are left alone.** `#runtimes = "bulk/runtimes"` stays where the
/// template put it, above the real key this writes. Removing it would mean matching on comment
/// text, which is a parser for prose.
///
/// The file has to be there: this is called after [`load_or_create`], and creating one here would
/// mean a `config.toml` with these four keys and none of the documentation.
///
/// # Errors
///
/// [`Error::Io`] when the file cannot be read or replaced, and [`Error::ConfigEdit`] when it cannot
/// be parsed or when a value is one [`load`] would refuse — the same sentence, from the same check.
///
/// [`Config`]: super::Config
/// [`TEMPLATE`]: super::TEMPLATE
/// [`load_or_create`]: super::load_or_create
/// [`load`]: super::load
pub fn set_paths(path: &Path, requested: &RequestedPaths) -> Result<Vec<&'static str>> {
    let text = std::fs::read_to_string(path).map_err(|source| Error::Io {
        action: "read",
        path: path.to_path_buf(),
        source,
    })?;

    let mut document: toml_edit::DocumentMut =
        text.parse()
            .map_err(|error: toml_edit::TomlError| Error::ConfigEdit {
                path: path.to_path_buf(),
                reason: error.to_string(),
            })?;

    // Every value checked before any of them is written: a batch that refused the fourth key after
    // writing three would leave the file saying something nobody asked for.
    let mut written = Vec::new();
    for (key, asked) in requested.entries() {
        let Some(asked) = asked else {
            continue;
        };

        check_relocation(asked).map_err(|reason| Error::ConfigEdit {
            path: path.to_path_buf(),
            reason: format!("{key}: {reason}"),
        })?;

        // TOML is UTF-8, so a path this cannot spell is a path that cannot be written down at all.
        // Refused rather than lossily converted: a `to_string_lossy` here would store a path with
        // a replacement character in it and report success.
        if asked.to_str().is_none() {
            return Err(Error::ConfigEdit {
                path: path.to_path_buf(),
                reason: format!(
                    "{key}: this path is not valid UTF-8, so it cannot be written into a TOML file"
                ),
            });
        }

        written.push(key);
    }

    if written.is_empty() {
        return Ok(written);
    }

    let table = document
        .entry("paths")
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()));

    let Some(table) = table.as_table_mut() else {
        return Err(Error::ConfigEdit {
            path: path.to_path_buf(),
            reason: "[paths] is there and is not a table".to_owned(),
        });
    };

    // Without this a table created here is *implicit* and renders as nothing at all, so the keys
    // would be written and the file would come back without them.
    table.set_implicit(false);

    // Written in this system's spelling (T191); `toml_edit` escapes the backslashes a person
    // writing the file by hand has to think about.
    for (key, asked) in requested.entries() {
        if let Some(asked) = asked {
            let spelled = crate::paths::native(asked);
            if let Some(text) = spelled.to_str() {
                table[key] = toml_edit::value(text);
            }
        }
    }

    replace_atomically(path, &document.to_string())?;

    Ok(written)
}

/// Write `contents` over `path` without ever leaving a torn file behind.
///
/// A temporary in the same directory, flushed, then renamed — the rule
/// `mixengine_platform`'s own replace states and which this cannot call: that one is
/// `pub(crate)` to its crate and compiled only for the elevated helper, deliberately, so that a
/// third caller cannot reach the mechanism that rewrites `/etc/hosts`. This writes a file inside
/// `MIXENGINE_HOME` and needs none of that one's ownership carrying.
fn replace_atomically(path: &Path, contents: &str) -> Result<()> {
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let temporary = directory.join(format!(
        "{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));

    let failed = |path: &Path, source: std::io::Error| Error::Io {
        action: "write",
        path: path.to_path_buf(),
        source,
    };

    let staged = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()
    })();

    if let Err(source) = staged {
        let _ = std::fs::remove_file(&temporary);
        return Err(failed(&temporary, source));
    }

    // The Windows case `crate::generate` already carries: a rename replaces an existing file
    // everywhere this runs, except while another process holds the target open without sharing
    // deletion. Removing it first opens a window where the file is absent, which is worse than a
    // rename and better than a start that fails because somebody had the file in an editor.
    if let Err(source) = std::fs::rename(&temporary, path) {
        if !path.exists() {
            let _ = std::fs::remove_file(&temporary);
            return Err(failed(path, source));
        }

        std::fs::remove_file(path).map_err(|source| failed(path, source))?;

        if let Err(source) = std::fs::rename(&temporary, path) {
            let _ = std::fs::remove_file(&temporary);
            return Err(failed(path, source));
        }
    }

    Ok(())
}

/// Refuse a relocation that does not name a directory of its own, or that names one ambiguously.
///
/// `Path::join("")` hands the original path straight back, so `data = ""` would not fail, it would
/// make `data/` *be* `MIXENGINE_HOME`. Everything downstream would then treat the whole install as
/// the service data directory — and "reset the data directory" is a thing MixEngine offers to do.
/// `"."`, `".."`, `"bulk/.."` and `"/"` all end up somewhere just as destructive: the home itself,
/// a directory containing it, or an entire filesystem. A user who wants the default deletes the
/// key.
fn relocation<'de, D>(deserializer: D) -> std::result::Result<Option<PathBuf>, D::Error>
where
    D: Deserializer<'de>,
{
    let path = Option::<PathBuf>::deserialize(deserializer)?;

    if let Some(candidate) = path.as_deref() {
        check_relocation(candidate).map_err(serde::de::Error::custom)?;
    }

    // Respelled once it has passed (T191): the template advises `/` for TOML's sake, and a path
    // built under `D:/bulk` would otherwise read `D:/bulk\mariadb` everywhere it is shown.
    Ok(path.map(|path| crate::paths::native(&path)))
}

/// The rule [`relocation`] enforces, as a function anything may call — roadmap task **T143**.
///
/// **Lifted out of the deserializer rather than copied beside it.** [`set_paths`] writes the same
/// four keys the file declares, and two lists of refused values are two lists that disagree the
/// first time one of them grows a case. This is the only place the rule exists; the deserializer
/// and the writer are both callers.
///
/// # Errors
///
/// The sentence to show whoever wrote the path — the same sentence either door produces.
fn check_relocation(candidate: &Path) -> std::result::Result<(), String> {
    // "Is it anchored?" before "does it name anything?": both questions are answered by refusing
    // the path, but a half-anchored Windows path has its own diagnosis and deserves to hear it
    // rather than the generic one.
    if is_drive_less_root(candidate) {
        return Err(
            "this path starts at the root of a drive without saying which one; write it \
                    in full (D:\\bulk\\data) or make it relative to the MixEngine home (bulk/data)"
                .to_owned(),
        );
    }

    if is_drive_relative(candidate) {
        return Err(
            "this path names a drive but does not start at its root, so it would land \
                    wherever that drive's current directory happens to be; write it in full \
                    (D:\\bulk\\data) or make it relative to the MixEngine home (bulk/data)"
                .to_owned(),
        );
    }

    if !names_a_directory(candidate) {
        return Err(
            "after resolving `.` and `..` this names no directory of its own; it points \
                    at the MixEngine home, at a directory containing it, or at the root of a \
                    filesystem; remove the key to use the default"
                .to_owned(),
        );
    }

    Ok(())
}

/// Does anything survive resolving `.` and `..` — is there a directory here at all?
///
/// Counts how deep the path ends up, purely lexically: `bulk/runtimes` is two down and fine,
/// `bulk/..` is back where it started, `..` is above it, `""`, `"."` and `"/"` never went anywhere.
/// A path that ends at depth zero names the MixEngine home, one of its ancestors, or a filesystem
/// root — and "reset this directory" would then take far more than the directory with it.
///
/// A `..` that climbs and then descends again is left alone: `../bulk` is a sibling of the home,
/// which is an ordinary thing to want and contains nothing dangerous.
fn names_a_directory(path: &Path) -> bool {
    let mut depth = 0usize;

    for part in path.components() {
        match part {
            Component::Normal(_) => depth += 1,
            // Climbing above where we started is a no-op here: the components that follow are what
            // decide whether the path names something, and `../bulk` names `bulk`.
            Component::ParentDir => depth = depth.saturating_sub(1),
            // A share *is* a directory — `\\server\share` is somewhere data can go, and Windows
            // folds the whole of it into the prefix, leaving nothing behind for the count. A drive
            // letter is not: `C:\` is a filesystem, and relocating `data/` onto one whole is the
            // case above.
            Component::Prefix(prefix) => {
                if matches!(prefix.kind(), Prefix::UNC(..) | Prefix::VerbatimUNC(..)) {
                    depth += 1;
                }
            }
            Component::CurDir | Component::RootDir => {}
        }
    }

    depth > 0
}

/// `\bulk` or `/bulk` on Windows: rooted, but with no drive to root it to.
///
/// `Path::join` resolves these against the *current* drive rather than against the path it is
/// joined to, so `C:\home\MixEngine`.join("/bulk") is `C:\bulk`. Treating that as "relative to the
/// home" — which is what the surrounding code promises — would be a lie, and treating it as
/// absolute would silently pick a drive the user never named. Refusing is the only honest answer.
///
/// On Unix `has_root()` and `is_absolute()` are the same question, so this is always `false` and
/// the check costs nothing.
fn is_drive_less_root(path: &Path) -> bool {
    path.has_root() && !path.is_absolute()
}

/// `C:bulk` on Windows: a drive named, but not the root of it.
///
/// The mirror image of [`is_drive_less_root`] and just as dishonest. A path carrying a prefix
/// replaces the whole of whatever it is joined to — `C:\home\MixEngine`.join("C:bulk") is plain
/// `C:bulk` — and the OS then resolves it against the *current directory of drive C*, which no
/// part of the configuration file mentions and which changes under the daemon's feet.
///
/// On Unix there are no prefixes, so `C:bulk` is an ordinary directory name and this is `false`.
fn is_drive_relative(path: &Path) -> bool {
    matches!(path.components().next(), Some(Component::Prefix(_))) && !path.is_absolute()
}
