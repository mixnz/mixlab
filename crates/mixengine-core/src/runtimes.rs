//! The `runtime_installs` table, and the two facts about a language that are not the index's.
//!
//! This module owns every write to that table and nothing else — the same division
//! [`crate::jobs`] draws over the work a job does: what downloads eighty megabytes is
//! [`crate::install`], what decides to is the daemon, and what is written down afterwards is here.
//!
//! # A row exists only for a directory that does
//!
//! The order every caller follows is: install into place, *then* write the row; remove the
//! directory, *then* delete the row. Neither half is a transaction across the two, because there is
//! no such thing — SQLite cannot roll back a rename. What the ordering buys is that the failure
//! modes are the survivable ones: a directory with no row is invisible and costs disk, a row with no
//! directory is a runtime that fails the moment somebody uses it. The first is a repair, the second
//! is a bug report.
//!
//! # Two facts that belong to the language rather than to the index
//!
//! [`directory`] and [`smoke_test`]. Where a version lands is MixEngine's layout and not the
//! publisher's, and which flag prints a version is a fact about PHP and Node — which is exactly why
//! [`install::SmokeTest`](crate::install::SmokeTest) arrives as an argument rather than being
//! decided down there. Everything else about an artifact comes out of the signed index.

mod error;

pub use error::RuntimeError;

pub mod extensions;
pub mod globals;
pub mod java;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mixengine_proto::{
    PackageChannel, PackageVersion, RuntimeKind, RuntimeSummary, Timestamp, VersionError,
};

use crate::install::SmokeTest;
use crate::{Error, Paths, Result, Store};

/// Where one version of one runtime lives.
///
/// `runtimes/<kind>/<version>/`, which is the layout
/// [runtime-versions.md](../../../docs/features/runtime-versions.md) states and the reason a
/// version is a validated path component ([`PackageVersion`]) rather than a string: this is a `join`
/// and not an escaping problem.
#[must_use]
pub fn directory(paths: &Paths, kind: RuntimeKind, version: &PackageVersion) -> PathBuf {
    paths.runtimes().join(kind.as_str()).join(version.as_str())
}

/// What to run once a runtime is unpacked, to find out whether it runs *here* — or [`None`] for a
/// kind whose artifact starts nothing.
///
/// The executable is named as a key of the artifact's `provides` map rather than as a path, because
/// the path inside the archive is the publisher's and the name is ours.
///
/// **Every one of these exits zero quickly and touches the runtime's own machinery**, which is the
/// property that makes the check worth its second: `php -v` loads every extension the ini names, so
/// a build whose extension directory is wrong fails here rather than in the middle of somebody's
/// afternoon. `--version` is the same thing for the other three.
///
/// **Composer answers [`None`]** — roadmap task **T27c**, its design's D3. Its artifact is a `.phar`,
/// which a PHP runs; the PHP may not be installed yet, and asking for one here would make
/// `mix runtime install composer` fail on the machine a gallery blueprint is about to install PHP
/// on. What the download proved by hash is the whole of what can be proved without one.
#[must_use]
pub fn smoke_test(kind: RuntimeKind) -> Option<SmokeTest> {
    let (executable, flag, unset): (&str, &str, &'static [&'static str]) = match kind {
        // Not `php-win.exe`, which answers `-v` with nothing at all — one of the four bugs T20a
        // found, and the reason the index publishes both under names of ours.
        RuntimeKind::Php => ("php", "-v", &[]),
        RuntimeKind::Node => ("node", "--version", &[]),
        RuntimeKind::Python => ("python", "--version", &[]),
        RuntimeKind::Ruby => ("ruby", "--version", &[]),
        // `go` has no `--version` flag. `version` touches no module, and the install directory
        // holds no `go.mod`, so `go.env`'s `GOTOOLCHAIN=auto` has nothing to act on here (T27d).
        RuntimeKind::Go => ("go", "version", &[]),
        // `--version` exists from JDK 9 and prints to stdout; on Linux it is also what fails when
        // `libz.so.1` is missing, since the launcher imports it (T27e, D3).
        RuntimeKind::Java => ("java", "--version", java::UNSET),
        RuntimeKind::Composer => return None,
    };

    Some(SmokeTest {
        executable: executable.to_owned(),
        args: vec![flag.to_owned()],
        unset,
    })
}

/// Everything a finished install has to write down.
///
/// Taken as one value rather than as seven arguments, because five of them come straight off the
/// artifact that was installed and a caller assembling them in the wrong order would produce a row
/// that is wrong rather than one that fails to insert.
#[derive(Debug, Clone)]
pub struct Installation {
    /// Which language.
    pub kind: RuntimeKind,

    /// Which version.
    pub version: PackageVersion,

    /// Which channel the index published it on.
    pub channel: PackageChannel,

    /// Where it landed — [`directory`]'s answer, passed in rather than recomputed so the row names
    /// the directory that was actually renamed into place.
    pub path: PathBuf,

    /// How large the archive was, as the index declared it and the download proved it.
    pub bytes: u64,

    /// Which URL it came from, kept so a support conversation can ask "from where".
    pub url: String,

    /// The hash the index published for it, kept for the same reason.
    pub sha256: String,

    /// What this build calls its executables, and where each one is inside the directory.
    ///
    /// [`Artifact::provides`](crate::index::Artifact::provides), carried through rather than
    /// consulted and dropped. The install has always needed it — the smoke test names a key of it —
    /// and **the shim needs it afterwards**: a program called `php` has to become a path with no
    /// daemon to ask, and the layout is the publisher's rather than ours.
    pub provides: BTreeMap<String, String>,

    /// Where this build keeps its loadable extensions, relative to the install directory.
    ///
    /// [`Artifact::extension_dir`](crate::index::Artifact::extension_dir), and [`None`] for a
    /// runtime that can load none — which is every Node, Python and Ruby this build installs, and is
    /// what keeps the whole of [`extensions`](Self::extensions) from being a `match` on the kind.
    pub extension_dir: Option<String>,

    /// What this build offers, split by whether it can be turned off.
    ///
    /// Copied down for [`provides`](Self::provides)' reason: the answer has to survive a network
    /// that is not there and an index cache that has expired.
    pub extensions: crate::index::Extensions,
}

/// Write down a runtime that is now on disk.
///
/// **The first version of a kind becomes its default**, in the same transaction that inserts it.
/// That is not a convenience: a home whose only PHP is not the default is a home where `php`
/// resolves to nothing, and nobody installing their first PHP is asking for that. Every version
/// after it arrives beside the default rather than replacing it, because an install that silently
/// moved what `php` means would be an install that broke a project the user was not thinking about.
///
/// # Errors
///
/// [`RuntimeError::AlreadyRecorded`] when a row for this kind and version already exists, and
/// [`Error::Database`] when it cannot be written.
pub async fn remember(
    store: &Store,
    installation: &Installation,
    at: Timestamp,
) -> Result<RuntimeSummary> {
    // `BEGIN IMMEDIATE` for `jobs::progress`' reason: the count below decides what the insert
    // writes, and a deferred `BEGIN` would leave the write to upgrade a read snapshot.
    let mut tx = store
        .pool()
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|source| store.failure("write", source))?;

    let (kind, version) = (installation.kind.as_str(), installation.version.as_str());

    let has_default = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM runtime_installs WHERE kind = ? AND is_default = 1",
        kind
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|source| store.failure("read", source))?
        > 0;

    let default = !has_default;
    let (channel, installed_at) = (installation.channel.as_str(), at.to_rfc3339());
    let path = installation.path.display().to_string();
    // Serialising a `BTreeMap<String, String>` cannot fail — the failure modes of `to_string` are a
    // type that refuses to serialise and a map with non-string keys, and this is neither. Written as
    // a fallback rather than an `expect` because nothing in this crate panics, and an empty map is
    // what a row with no recorded executables already means.
    let provides =
        serde_json::to_string(&installation.provides).unwrap_or_else(|_| "{}".to_owned());
    // The column is INTEGER and the value is a count of bytes: a size that does not fit an `i64` is
    // an artifact of nine million terabytes, so the saturation is a formality rather than a case.
    let bytes = i64::try_from(installation.bytes).unwrap_or(i64::MAX);
    let is_default = i64::from(default);
    let extension_dir = installation.extension_dir.clone().unwrap_or_default();
    // The same fallback as `provides` above, and for the same reason: this cannot fail, and an
    // empty object is what a row with nothing recorded already means.
    let extensions =
        serde_json::to_string(&installation.extensions).unwrap_or_else(|_| "{}".to_owned());

    let inserted = sqlx::query!(
        "INSERT INTO runtime_installs
             (kind, version, channel, install_path, installed_at, size_bytes, source_url, sha256,
              is_default, provides_json, extension_dir, extensions_json)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (kind, version) DO NOTHING",
        kind,
        version,
        channel,
        path,
        installed_at,
        bytes,
        installation.url,
        installation.sha256,
        is_default,
        provides,
        extension_dir,
        extensions
    )
    .execute(&mut *tx)
    .await
    .map_err(|source| store.failure("write", source))?;

    // `DO NOTHING` rather than letting the unique index raise: the collision is a real case — two
    // clients asking for the same install at the same moment — and it deserves a sentence naming the
    // version rather than SQLite's.
    if inserted.rows_affected() == 0 {
        return Err(Error::Runtime(RuntimeError::AlreadyRecorded {
            kind: installation.kind,
            version: installation.version.clone(),
        }));
    }

    tx.commit()
        .await
        .map_err(|source| store.failure("write", source))?;

    tracing::info!(%kind, %version, default, "a runtime was recorded");

    Ok(RuntimeSummary {
        kind: installation.kind,
        version: installation.version.clone(),
        channel: installation.channel,
        path,
        installed_at: at,
        bytes: installation.bytes,
        default,
        // Read rather than assumed — T206d, D1: the row names the folder, and the folder decides.
        missing: Some(crate::paths::is_gone(&installation.path).await),
    })
}

/// Write a reinstalled runtime over the row it already had — roadmap task **T206d**, D3.
///
/// **An `UPDATE`, never a second row**: the id is what a pool's service points at, and `is_default`
/// is left as it was, so a default that went missing comes back as the default. Called only after
/// the folder was renamed into place, which is [`remember`]'s ordering.
///
/// # Errors
///
/// [`Error::NotFound`] when there is no row to restore, and [`Error::Database`] when it cannot be
/// written.
pub async fn restore(
    store: &Store,
    installation: &Installation,
    at: Timestamp,
) -> Result<RuntimeSummary> {
    let (kind, version) = (installation.kind.as_str(), installation.version.as_str());
    let (channel, installed_at) = (installation.channel.as_str(), at.to_rfc3339());
    let path = installation.path.display().to_string();
    // As in `remember`: a size past `i64` is a formality, and neither map can fail to serialise.
    let bytes = i64::try_from(installation.bytes).unwrap_or(i64::MAX);
    let provides =
        serde_json::to_string(&installation.provides).unwrap_or_else(|_| "{}".to_owned());
    let extension_dir = installation.extension_dir.clone().unwrap_or_default();
    let extensions =
        serde_json::to_string(&installation.extensions).unwrap_or_else(|_| "{}".to_owned());

    let updated = sqlx::query!(
        "UPDATE runtime_installs
         SET channel = ?, install_path = ?, installed_at = ?, size_bytes = ?, source_url = ?,
             sha256 = ?, provides_json = ?, extension_dir = ?, extensions_json = ?
         WHERE kind = ? AND version = ?",
        channel,
        path,
        installed_at,
        bytes,
        installation.url,
        installation.sha256,
        provides,
        extension_dir,
        extensions,
        kind,
        version
    )
    .execute(store.pool())
    .await
    .map_err(|source| store.failure("write", source))?;

    if updated.rows_affected() == 0 {
        return Err(missing(installation.kind, &installation.version));
    }

    tracing::info!(%kind, %version, "a runtime was restored in place");

    record(store, installation.kind, &installation.version).await
}

/// Forget a runtime whose directory has already gone, and say whether its kind is left with none.
///
/// **Nothing is promoted in its place**, and that stayed true when T24 arrived with the grammar
/// that could pick a successor. Being *able* to name the newest remaining version was never the
/// argument: an uninstall that silently moved what `php` means would break a project nobody was
/// thinking about, which is the same reason [`remember`] does not move it either. A kind whose
/// default was removed is left with no default, out loud, and `mix runtime default` is one command.
///
/// # Errors
///
/// [`Error::NotFound`] when there is no such row, and [`Error::Database`] when it cannot be written.
pub async fn forget(store: &Store, kind: RuntimeKind, version: &PackageVersion) -> Result<bool> {
    let (kind_column, version_column) = (kind.as_str(), version.as_str());

    let removed = sqlx::query_scalar!(
        "DELETE FROM runtime_installs WHERE kind = ? AND version = ? RETURNING is_default",
        kind_column,
        version_column
    )
    .fetch_optional(store.pool())
    .await
    .map_err(|source| store.failure("write", source))?
    .ok_or_else(|| missing(kind, version))?;

    let was_default = removed == 1;
    tracing::info!(%kind_column, %version_column, was_default, "a runtime was forgotten");

    Ok(was_default)
}

/// Make one installed version the one its kind resolves to.
///
/// Two statements in one transaction, and the partial unique index on the table is what makes the
/// pair safe rather than the ordering: `runtime_installs_one_default_per_kind` refuses a second
/// default for a kind outright, so a bug that cleared nothing would fail loudly instead of leaving
/// two rows both claiming to be it.
///
/// Idempotent: making the current default the default again writes the same two rows and answers the
/// same summary, which is what `docs/architecture/daemon-and-ipc.md` asks of every verb it makes
/// sense for.
///
/// # Errors
///
/// [`Error::NotFound`] when that version is not installed, [`RuntimeError::UnreadableRuntimeRow`]
/// when the row cannot be read back, and [`Error::Database`] when it cannot be written.
pub async fn set_default(
    store: &Store,
    kind: RuntimeKind,
    version: &PackageVersion,
) -> Result<RuntimeSummary> {
    let mut tx = store
        .pool()
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|source| store.failure("write", source))?;

    let (kind_column, version_column) = (kind.as_str(), version.as_str());

    sqlx::query!(
        "UPDATE runtime_installs SET is_default = 0 WHERE kind = ? AND is_default = 1",
        kind_column
    )
    .execute(&mut *tx)
    .await
    .map_err(|source| store.failure("write", source))?;

    let promoted = sqlx::query!(
        "UPDATE runtime_installs SET is_default = 1
         WHERE kind = ? AND version = ?
         RETURNING kind, version, channel, install_path, installed_at, size_bytes, is_default",
        kind_column,
        version_column
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|source| store.failure("write", source))?
    .ok_or_else(|| missing(kind, version))?;

    let summary = summary(
        promoted.kind,
        promoted.version,
        promoted.channel,
        promoted.install_path,
        &promoted.installed_at,
        promoted.size_bytes,
        promoted.is_default,
    )?;

    tx.commit()
        .await
        .map_err(|source| store.failure("write", source))?;

    tracing::info!(%kind_column, %version_column, "a runtime became the default for its kind");

    Ok(summary)
}

/// Every installed runtime, optionally of one kind.
///
/// Ordered by kind and then by the version string, which is **not** newest-first — and still is not,
/// now that [`PackageVersion::cmp_precedence`] could make it so. A listing is a table somebody scans
/// for a row they already have in mind, and the order that makes a row findable is the one the eye
/// can predict. Choosing *between* versions is [`crate::resolve`]'s, and that is where precedence
/// belongs; a client that wants another order here has the whole list.
///
/// # Errors
///
/// [`RuntimeError::UnreadableRuntimeRow`] when a row holds something this build cannot read back,
/// and [`Error::Database`] when the table cannot be read.
pub async fn records(store: &Store, kind: Option<RuntimeKind>) -> Result<Vec<RuntimeSummary>> {
    let wanted = kind.map(RuntimeKind::as_str);

    // The same value bound twice rather than a numbered parameter, on `jobs::records`' precedent:
    // `?1` is valid SQLite and it is the sqlx macro that has an opinion about it.
    let rows = sqlx::query!(
        "SELECT kind, version, channel, install_path, installed_at, size_bytes, is_default
         FROM runtime_installs
         WHERE (? IS NULL OR kind = ?)
         ORDER BY kind, version",
        wanted,
        wanted
    )
    .fetch_all(store.pool())
    .await
    .map_err(|source| store.failure("read", source))?;

    let mut listed = rows
        .into_iter()
        .map(|row| {
            summary(
                row.kind,
                row.version,
                row.channel,
                row.install_path,
                &row.installed_at,
                row.size_bytes,
                row.is_default,
            )
        })
        .collect::<Result<Vec<_>>>()?;

    // **Stamped from the disk on every read** — roadmap task **T206d**, D1: no column, so the answer
    // is the folder's state now, at one `stat` a row.
    for summary in &mut listed {
        stamp(summary).await;
    }

    Ok(listed)
}

/// One installed runtime.
///
/// # Errors
///
/// [`Error::NotFound`] when it is not installed, [`RuntimeError::UnreadableRuntimeRow`] when the
/// row cannot be read back, and [`Error::Database`] when the table cannot be read.
pub async fn record(
    store: &Store,
    kind: RuntimeKind,
    version: &PackageVersion,
) -> Result<RuntimeSummary> {
    let (kind_column, version_column) = (kind.as_str(), version.as_str());

    let row = sqlx::query!(
        "SELECT kind, version, channel, install_path, installed_at, size_bytes, is_default
         FROM runtime_installs WHERE kind = ? AND version = ?",
        kind_column,
        version_column
    )
    .fetch_optional(store.pool())
    .await
    .map_err(|source| store.failure("read", source))?
    .ok_or_else(|| missing(kind, version))?;

    let mut found = summary(
        row.kind,
        row.version,
        row.channel,
        row.install_path,
        &row.installed_at,
        row.size_bytes,
        row.is_default,
    )?;
    stamp(&mut found).await;

    Ok(found)
}

/// Whether the folder a row names is gone, read now — roadmap task **T206d**, D1.
async fn stamp(summary: &mut RuntimeSummary) {
    summary.missing = Some(crate::paths::is_gone(Path::new(&summary.path)).await);
}

/// The program an installed runtime publishes under `executable`, as a path that can be run.
///
/// **The shim's whole lookup**, and the reason `provides` is a column rather than something the
/// installer consulted and forgot: a process called `php` has resolved a version and now has to
/// become a program, with no daemon to ask and no right to guess. `php.exe` sits at the root of the
/// Windows archive and `bin/php` inside the Unix one, because a borrowed archive keeps the layout
/// its publisher shipped.
///
/// # What is checked, and why a signed index is not enough
///
/// The map arrives from a document we published and whose signature was verified before it was
/// parsed — and then it spends months in a file on the user's machine. So the relative path is held
/// to the same rule [`crate::install`] holds an archive entry to: no root, no drive, no `..`. The
/// value being trusted at the moment it is used is the one in the database, and what makes that
/// sound is this check rather than the signature that put it there.
///
/// # Errors
///
/// [`Error::NotFound`] when that version is not installed; [`RuntimeError::UnreadableRuntimeRow`]
/// when the row cannot be read back, which now includes a `provides_json` that is not a map of
/// strings or that names a path leading out of the install directory;
/// [`RuntimeError::RuntimeProvidesNothing`] when the runtime is installed and publishes no such
/// executable — which is also what an install from before this column existed answers, with an
/// empty list; and [`Error::Database`] when the table cannot be read.
pub async fn program(
    store: &Store,
    kind: RuntimeKind,
    version: &PackageVersion,
    executable: &str,
) -> Result<PathBuf> {
    let (kind_column, version_column) = (kind.as_str(), version.as_str());

    let row = sqlx::query!(
        "SELECT install_path, provides_json FROM runtime_installs WHERE kind = ? AND version = ?",
        kind_column,
        version_column
    )
    .fetch_optional(store.pool())
    .await
    .map_err(|source| store.failure("read", source))?
    .ok_or_else(|| missing(kind, version))?;

    let unreadable = |value: &str| {
        Error::Runtime(RuntimeError::UnreadableRuntimeRow {
            column: "provides_json",
            value: value.to_owned(),
        })
    };

    let provides: BTreeMap<String, String> =
        serde_json::from_str(&row.provides_json).map_err(|_| unreadable(&row.provides_json))?;

    let relative = provides.get(executable).ok_or_else(|| {
        Error::Runtime(RuntimeError::RuntimeProvidesNothing {
            kind,
            version: version.clone(),
            executable: executable.to_owned(),
            known: provides.keys().cloned().collect(),
        })
    })?;

    let checked = Path::new(relative);
    if !crate::install::archive::safe(checked) {
        return Err(unreadable(&format!("{executable} = {}", checked.display())));
    }

    Ok(crate::paths::join_stored(
        Path::new(&row.install_path),
        relative,
    ))
}

/// Whether anything of this kind is installed at all.
///
/// Its own read rather than `records(…).is_empty()`, because the caller asking it — the uninstall
/// that has just removed a default — does not want the rows and would be paying for a listing to
/// learn a boolean.
///
/// # Errors
///
/// [`Error::Database`] when the table cannot be read.
pub async fn any_installed(store: &Store, kind: RuntimeKind) -> Result<bool> {
    let kind_column = kind.as_str();

    let count = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM runtime_installs WHERE kind = ?",
        kind_column
    )
    .fetch_one(store.pool())
    .await
    .map_err(|source| store.failure("read", source))?;

    Ok(count > 0)
}

/// The failure of looking one up, named the way the wire mapping expects.
///
/// `kind: "runtime"` is the RPC namespace, which is what the daemon turns into the hint
/// `mix runtime list` — so the identifier has to read as one thing a person could have typed.
fn missing(kind: RuntimeKind, version: &PackageVersion) -> Error {
    Error::NotFound {
        kind: "runtime",
        id: format!("{kind} {version}"),
    }
}

/// One row, as the type a client renders.
///
/// Every column is checked rather than assumed, because this is where a hand-edited database or a
/// row written by a version that knew more than this one arrives. `kind` has a `CHECK` behind it and
/// is still parsed here: the constraint and the enum have to agree, and the reader is what says so
/// when they do not.
// Seven columns, listed once, rather than a struct that exists only to be destructured immediately
// — `jobs::summary`'s reasoning, one table across. It stays one under clippy's threshold, which is
// why there is no expectation here and one next door.
fn summary(
    kind: String,
    version: String,
    channel: String,
    path: String,
    installed_at: &str,
    bytes: i64,
    is_default: i64,
) -> Result<RuntimeSummary> {
    let unreadable = |column: &'static str, value: &str| {
        Error::Runtime(RuntimeError::UnreadableRuntimeRow {
            column,
            value: value.to_owned(),
        })
    };

    Ok(RuntimeSummary {
        kind: RuntimeKind::parse(&kind).ok_or_else(|| unreadable("kind", &kind))?,
        version: PackageVersion::parse(version.clone())
            .map_err(|VersionError { value, .. }| unreadable("version", &value))?,
        // The one column with no `CHECK` and no closed set behind it in SQL, which makes it the one
        // a row written by a newer build reaches this function through.
        channel: PackageChannel::parse(&channel).ok_or_else(|| unreadable("channel", &channel))?,
        path,
        installed_at: Timestamp::parse_rfc3339(installed_at)
            .ok_or_else(|| unreadable("installed_at", installed_at))?,
        // A negative size is not expressible through any write of ours and would be a hand-edited
        // row; read as zero rather than refused, because the number is a display value and refusing
        // the whole listing over it would hide the runtime it belongs to.
        bytes: u64::try_from(bytes).unwrap_or(0),
        default: is_default == 1,
        // Stamped from the disk by the reader — T206d, D1.
        missing: None,
    })
}

/// Remove an installed runtime's directory, if it is there at all.
///
/// **Before the row, never after**, which is the ordering the note at the top of this module
/// explains: a directory that could not be removed leaves a row that still describes it, and asking
/// again repeats exactly this.
///
/// # Errors
///
/// [`Error::Io`] naming the directory, which on Windows most often means a process is still running
/// out of it.
pub async fn discard(path: &Path) -> Result<()> {
    let owned = path.to_path_buf();

    let removed = tokio::task::spawn_blocking(move || match std::fs::remove_dir_all(&owned) {
        // Already gone is the outcome that was wanted. A row whose directory somebody deleted by
        // hand is exactly the repair this ordering leaves possible, and refusing it would make the
        // row unremovable through the API.
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    })
    .await;

    match removed {
        Ok(Ok(())) => Ok(()),
        Ok(Err(source)) => Err(Error::Io {
            action: "remove",
            path: path.to_path_buf(),
            source,
        }),
        // The blocking pool panicked, which nothing here does. Reported rather than unwrapped
        // because nothing in this crate panics.
        Err(source) => Err(Error::Io {
            action: "remove",
            path: path.to_path_buf(),
            source: std::io::Error::other(source),
        }),
    }
}

#[cfg(test)]
mod tests;
