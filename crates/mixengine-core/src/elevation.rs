//! The queue of privileged operations, the document that carries them to `mixengine-elevate`, and
//! the report it leaves behind.
//!
//! **This crate owns the row and the document; the daemon owns the prompt.** The same cut
//! [`crate::jobs`] documents one table across: nothing here has a loop, a clock or a task, and
//! nothing here spawns a process. What that buys is the test the daemon cannot have —
//! `mixengine-daemon` is a binary crate with no library target, so with the document here a test in
//! `tests/elevation.rs` can build a request with the shipped code, run the **real** helper under an
//! ordinary token, and read the report back with the shipped code. See the T40b design, D3.

mod error;

pub use error::ElevationError;

use std::path::{Path, PathBuf};

use mixengine_proto::privileged::{
    OpOutcome, PrivilegedOp, PrivilegedRequest, PrivilegedResponse, RESPONSE_FILE_NAME,
};
use mixengine_proto::{PendingOp, PendingOpId, Timestamp};

use crate::{Error, Result, Store};

/// The operation as the `op` column holds it: its serialisation, and nothing else.
///
/// **Not the `dedupe_key` any more.** T40b wrote one value into both columns, which is right for an
/// operation carrying no state and wrong for a whole-state one: two `hosts-apply` rows disagreeing
/// about what the file should hold would both be valid. The key is now the operation's *identity*
/// and is [`PrivilegedOp::dedupe_key`]'s to answer — see the T41 design, D2.
///
/// # Errors
///
/// [`ElevationError::OpUnwritable`], which cannot happen — a [`PrivilegedOp`] is one of ours and holds
/// nothing serde can refuse. Mapped rather than unwrapped, because nothing in this crate panics.
fn canonical(op: &PrivilegedOp) -> Result<String> {
    serde_json::to_string(op)
        .map_err(|source| Error::Elevation(crate::ElevationError::OpUnwritable { source }))
}

/// Put an operation in the queue, and hand back the whole queue when that changed something.
///
/// [`None`] means the operation was already waiting: the machine's needs did not change, so there is
/// nothing to announce and the daemon publishes no
/// [`ElevationRequired`](mixengine_proto::DaemonEvent::ElevationRequired). See the T40b design, D8.
///
/// **A whole-state operation supersedes the one that was waiting** rather than queueing beside it —
/// the T41 design, D2. `requested_at` is deliberately not refreshed: the need started when it
/// started, and a queue that reset its own clock on every site creation would report a wait that
/// never got older. The `WHERE` clause is what preserves [`None`]'s meaning: re-enqueueing the same
/// state touches no row, so nothing is announced.
///
/// The list is read back **inside the transaction that inserted**, on
/// [`services::transition`](crate::services::transition)'s rule: what is announced is what survived
/// the write, so the row and the event cannot disagree.
///
/// `at` is passed in rather than read from the clock, as everywhere else in this crate: the caller
/// already has a reading, and a test needs to be able to say when.
///
/// # Errors
///
/// [`ElevationError::OpUnwritable`] when the operation cannot be encoded, and [`Error::Database`] when the row
/// cannot be written.
pub async fn enqueue(
    store: &Store,
    op: &PrivilegedOp,
    at: Timestamp,
) -> Result<Option<Vec<PendingOp>>> {
    let (encoded, key, requested) = (canonical(op)?, op.dedupe_key(), at.0);

    // `BEGIN IMMEDIATE` for `jobs::progress`' reason: the insert decides whether the read below
    // happens at all, and a deferred `BEGIN` would leave a write to upgrade a read snapshot, which
    // WAL refuses outright without even running the busy handler.
    let mut tx = store
        .pool()
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|source| store.failure("write", source))?;

    let written = sqlx::query!(
        "INSERT INTO pending_privileged_ops (op, dedupe_key, requested_at)
         VALUES (?, ?, ?)
         ON CONFLICT (dedupe_key) DO UPDATE SET op = excluded.op WHERE op <> excluded.op",
        encoded,
        key,
        requested
    )
    .execute(&mut *tx)
    .await
    .map_err(|source| store.failure("write", source))?;

    if written.rows_affected() == 0 {
        // Rolled back by being dropped. The `WHERE` clause is what the statement says, but it is the
        // `UNIQUE` index that makes a second writer unable to break the rule by forgetting it.
        return Ok(None);
    }

    let waiting = read(store, &mut tx).await?;

    tx.commit()
        .await
        .map_err(|source| store.failure("write", source))?;

    tracing::info!(
        op = op.name(),
        waiting = waiting.len(),
        "an operation is waiting for permission"
    );

    Ok(Some(waiting))
}

/// Everything waiting, oldest first.
///
/// **A writer as well as a reader**, for the one case D2 names: a row this build cannot decode is
/// deleted and logged rather than carried. Filtering it instead would leave it to be met, and
/// warned about, on every call for the life of the home — and a row no installed build can act on is
/// a degraded mode nobody can ever clear.
///
/// # Errors
///
/// [`Error::Database`] when the table cannot be read, or when an undecodable row cannot be removed.
pub async fn pending(store: &Store) -> Result<Vec<PendingOp>> {
    let mut tx = store
        .pool()
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|source| store.failure("write", source))?;

    let waiting = read(store, &mut tx).await?;

    tx.commit()
        .await
        .map_err(|source| store.failure("write", source))?;

    Ok(waiting)
}

/// Read the queue inside a transaction somebody else opened, dropping what cannot be decoded.
async fn read(
    store: &Store,
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<Vec<PendingOp>> {
    let rows = sqlx::query!("SELECT id, op, requested_at FROM pending_privileged_ops ORDER BY id")
        .fetch_all(&mut **tx)
        .await
        .map_err(|source| store.failure("read", source))?;

    let mut waiting = Vec::with_capacity(rows.len());
    let mut undecodable = Vec::new();

    for row in rows {
        match serde_json::from_str::<PrivilegedOp>(&row.op) {
            Ok(op) => waiting.push(PendingOp {
                id: PendingOpId(row.id),
                description: op.describe(),
                op,
                requested_at: Timestamp(row.requested_at),
            }),
            Err(error) => {
                tracing::warn!(
                    id = row.id,
                    op = row.op,
                    %error,
                    "a pending privileged operation this build cannot act on was removed"
                );
                undecodable.push(row.id);
            }
        }
    }

    for id in undecodable {
        sqlx::query!("DELETE FROM pending_privileged_ops WHERE id = ?", id)
            .execute(&mut **tx)
            .await
            .map_err(|source| store.failure("write", source))?;
    }

    Ok(waiting)
}

/// Forget one operation, or all of them, and say how many rows went.
///
/// `discard` and not `drop`: a free function called `drop` in a module every caller imports shadows
/// the one in the prelude, and the confusion is not worth the symmetry with the wire verb.
///
/// Forgetting something that is not there is **not** an error — the caller wanted it gone and it is.
///
/// # Errors
///
/// [`Error::Database`] when the rows cannot be removed.
pub async fn discard(store: &Store, which: Option<PendingOpId>) -> Result<usize> {
    let removed = match which {
        Some(PendingOpId(id)) => {
            sqlx::query!("DELETE FROM pending_privileged_ops WHERE id = ?", id)
                .execute(store.pool())
                .await
        }
        None => {
            sqlx::query!("DELETE FROM pending_privileged_ops")
                .execute(store.pool())
                .await
        }
    }
    .map_err(|source| store.failure("write", source))?;

    Ok(usize::try_from(removed.rows_affected()).unwrap_or(usize::MAX))
}

/// Forget the operation of one family, because this machine no longer needs it — roadmap task
/// **T179**.
///
/// **The opposite of [`enqueue`], keyed the same way.** `key` is [`PrivilegedOp::dedupe_key`]'s own
/// value, so *the same question* means one thing in both directions and a family is spelled in one
/// place. The answer is how many rows went, which is 0 or 1 because that key is unique.
///
/// **Withdrawing is not revoking.** This removes a request nobody has answered; what the machine
/// already holds is untouched, and taking that back is a producer's job elsewhere or the
/// uninstall's (T87).
///
/// Withdrawing what is not there is **not** an error, on [`discard`]'s rule: the caller wanted it
/// gone and it is.
///
/// # Errors
///
/// [`Error::Database`] when the row cannot be removed.
pub async fn withdraw(store: &Store, key: &str) -> Result<usize> {
    let removed = sqlx::query!(
        "DELETE FROM pending_privileged_ops WHERE dedupe_key = ?",
        key
    )
    .execute(store.pool())
    .await
    .map_err(|source| store.failure("write", source))?;

    Ok(usize::try_from(removed.rows_affected()).unwrap_or(usize::MAX))
}

/// What one grant did to the queue.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settled {
    /// How many operations came back done — [`OpOutcome::Applied`] or [`OpOutcome::AlreadyDone`].
    pub applied: usize,

    /// The ones that will never succeed as written, and why, so the job's result can say so once.
    ///
    /// [`OpOutcome::Refused`] and [`OpOutcome::Unsupported`] together: their rows go for the same
    /// reason and what a person needs from either is the sentence.
    pub refused: Vec<(PendingOpId, String)>,

    /// The rows that are still there, and what each said — the [`OpOutcome::Failed`] ones.
    ///
    /// **The sentence is carried and not only counted** — a retryable failure is still a failure a
    /// person has to be told about. It was a count until 2026-09-16, and the cost of that showed up
    /// the first time a failure was permanent in practice: the grant reported *0 applied, 1 still
    /// waiting*, the reason lived in the audit log alone, and the operation was granted eight times
    /// against a message nobody had been shown. `kept` is `failed.len()`, so there is one number
    /// rather than two that can disagree.
    pub failed: Vec<(PendingOpId, String)>,
}

/// Apply a helper's report to the queue.
///
/// **Four outcomes delete the row and one keeps it** — the T40b design, D5:
///
/// | [`OpOutcome`] | The row | Why |
/// | --- | --- | --- |
/// | [`Applied`](OpOutcome::Applied) | deleted | done |
/// | [`AlreadyDone`](OpOutcome::AlreadyDone) | deleted | the machine is in the state that was asked for; that is the same outcome |
/// | [`Refused`](OpOutcome::Refused) | deleted | "the caller's fault, and the same request will be refused again". A row that cannot ever succeed and is never removed is a permanent degraded mode nobody can clear |
/// | [`Unsupported`](OpOutcome::Unsupported) | deleted | the installed helper does not know this operation, and it is excluded from auto-update, so it will not learn |
/// | [`Failed`](OpOutcome::Failed) | **kept** | "the OS refused. Trying again may work; nothing about the request is wrong" |
///
/// The distinction is `mixengine-proto`'s already; what this function contributes is not blurring
/// it. One transaction, so a report is applied whole or not at all.
///
/// # Errors
///
/// [`Error::Database`] when the rows cannot be removed.
pub async fn settle(store: &Store, results: &[(PendingOpId, OpOutcome)]) -> Result<Settled> {
    let mut tx = store
        .pool()
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|source| store.failure("write", source))?;

    let mut settled = Settled::default();

    for (id, outcome) in results {
        match outcome {
            // `Unmanaged` settles beside `Applied`, and that is a decision rather than a
            // shorthand — T74. The operation is finished: this machine has no mechanism for it, so
            // the row must not be kept for a retry that would answer the same thing forever. What
            // the user has to know instead travels back with the *share*, which renders the manual
            // command; the queue's job here is only to stop asking.
            OpOutcome::Applied { .. } | OpOutcome::AlreadyDone | OpOutcome::Unmanaged { .. } => {
                settled.applied += 1;
            }
            OpOutcome::Refused { reason } | OpOutcome::Unsupported { reason } => {
                settled.refused.push((*id, reason.clone()));
            }
            OpOutcome::Failed { message } => {
                settled.failed.push((*id, message.clone()));
                continue;
            }
        }

        let row = id.0;
        sqlx::query!("DELETE FROM pending_privileged_ops WHERE id = ?", row)
            .execute(&mut *tx)
            .await
            .map_err(|source| store.failure("write", source))?;
    }

    tx.commit()
        .await
        .map_err(|source| store.failure("write", source))?;

    Ok(settled)
}

/// The name the request takes inside its own directory.
///
/// Not in `mixengine-proto` beside [`RESPONSE_FILE_NAME`]: the helper is *given* this path as its one
/// argument and never composes it, so it is the writer's name for a file rather than part of the
/// protocol. The response's name is the protocol, because that one is agreed rather than passed.
const REQUEST_FILE_NAME: &str = "request.json";

/// A request lying on disk, and what it is an answer to.
///
/// Holds the nonce and the rows so that [`read_report`] checks the report against **this** request
/// rather than against something a caller remembered — and so that the daemon can zip outcomes back
/// onto rows without keeping a second list in step.
#[derive(Debug)]
pub struct Request {
    /// The single-use directory. Removed by the caller when the grant ends, on every branch.
    directory: PathBuf,

    /// The document, inside it.
    path: PathBuf,

    /// Echoed by the helper, and checked on the way back.
    nonce: String,

    /// The rows this batch was built from, in the order their outcomes will arrive.
    ids: Vec<PendingOpId>,
}

impl Request {
    /// The document's path — the helper's one argument.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The single-use directory holding it.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// What the helper will echo back.
    #[must_use]
    pub fn nonce(&self) -> &str {
        &self.nonce
    }

    /// The rows, in the order their outcomes arrive.
    #[must_use]
    pub fn ids(&self) -> &[PendingOpId] {
        &self.ids
    }
}

/// Write one batch into a fresh single-use directory.
///
/// **The directory is single-use by construction**, which is what makes `response.json`'s existence
/// a sufficient anti-replay check (T40/D10): nothing else is ever written beside a request, so a
/// request with an answer next to it has been processed and the helper refuses it.
///
/// The nonce comes from the OS's random source through
/// [`generate_secret`](mixengine_platform::generate_secret) rather than from a counter or a clock: a
/// daemon restarted twice in a second must not be able to produce two requests the helper cannot
/// tell apart.
///
/// `version` is the protocol to mark this request with, and it is the **caller's** decision —
/// roadmap task **T88a**. The daemon sends the lower of what it speaks and what the installed
/// helper answered a handshake with, because a fixed old binary cannot be taught a newer protocol
/// and the newer peer is therefore the one that speaks down. A function that stamped its own would
/// make that decision unreachable.
///
/// # Errors
///
/// [`ElevationError::ElevateRequestEmpty`] when `ops` is empty — the helper refuses an empty batch outright,
/// with no response file and exit 65, so it is refused here where the message can say why;
/// [`Error::Platform`] when the OS will not produce random bytes; [`ElevationError::OpUnwritable`] when an
/// operation cannot be encoded; and [`Error::Io`] naming the file that could not be written.
pub fn write_request(
    directory: &Path,
    home: &Path,
    ops: &[PendingOp],
    version: mixengine_proto::ProtocolVersion,
) -> Result<Request> {
    if ops.is_empty() {
        return Err(Error::Elevation(crate::ElevationError::ElevateRequestEmpty));
    }

    crate::paths::create_dir(directory)?;

    let nonce = mixengine_platform::generate_secret(32)?;

    let encoded = ops
        .iter()
        .map(|waiting| {
            serde_json::to_value(&waiting.op)
                .map_err(|source| Error::Elevation(crate::ElevationError::OpUnwritable { source }))
        })
        .collect::<Result<Vec<_>>>()?;

    let body = PrivilegedRequest {
        version,
        home: home.to_path_buf(),
        nonce: nonce.clone(),
        ops: encoded,
    };

    let path = directory.join(REQUEST_FILE_NAME);
    let text = serde_json::to_vec(&body)
        .map_err(|source| Error::Elevation(crate::ElevationError::OpUnwritable { source }))?;

    std::fs::write(&path, text).map_err(|source| Error::Io {
        action: "write",
        path: path.clone(),
        source,
    })?;

    Ok(Request {
        directory: directory.to_path_buf(),
        path,
        nonce,
        ids: ops.iter().map(|waiting| waiting.id).collect(),
    })
}

/// Read the report the helper left beside `request`, and check that it is one.
///
/// Three checks, and each of them is the reason a later step can be simple: the nonce, so an answer
/// to an earlier request cannot be read as the answer to this one; the protocol version; and one
/// outcome per operation, which is what lets the caller zip [`Request::ids`] against
/// [`PrivilegedResponse::results`] without wondering.
///
/// # Errors
///
/// [`ElevationError::ElevateReportMissing`] when there is nothing beside the request — **a real state and not
/// an impossibility**: `Completed` means the helper ran, not that it left a report, because a crash
/// is not a per-OS event. [`ElevationError::ElevateReportUnreadable`] when it is not JSON this build can read,
/// [`ElevationError::ElevateReportMismatched`] when it answers something else, and [`Error::Io`] when the file
/// is there and cannot be read.
pub fn read_report(request: &Request) -> Result<PrivilegedResponse> {
    let path = request.directory.join(RESPONSE_FILE_NAME);

    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            // What the helper said is the launcher's to know, not this reader's; the daemon adds it.
            return Err(Error::Elevation(
                crate::ElevationError::ElevateReportMissing { path, said: None },
            ));
        }
        Err(source) => {
            return Err(Error::Io {
                action: "read",
                path,
                source,
            });
        }
    };

    let response: PrivilegedResponse = serde_json::from_str(&text).map_err(|source| {
        Error::Elevation(crate::ElevationError::ElevateReportUnreadable {
            path: path.clone(),
            source,
        })
    })?;

    if response.nonce != request.nonce {
        return Err(Error::Elevation(
            crate::ElevationError::ElevateReportMismatched {
                path,
                why: "it answers a different request".to_owned(),
            },
        ));
    }

    // **At or above the floor, rather than exactly this build's** — roadmap task T88a. The helper is
    // excluded from auto-update, so a report from one older than this daemon is the ordinary case;
    // and one *newer* than this daemon is why the response was written without
    // `deny_unknown_fields`. The answer is bound to *this* request by the nonce, checked just above,
    // so what the version says is which build answered rather than which request it answered — and
    // the caller reads it to learn the ceiling every later request to that helper is marked at.
    if response.version < mixengine_proto::PROTOCOL_MINIMUM {
        return Err(Error::Elevation(
            crate::ElevationError::ElevateReportMismatched {
                path,
                why: format!(
                    "it speaks protocol {} and this daemon no longer serves anything below {}",
                    response.version.0,
                    mixengine_proto::PROTOCOL_MINIMUM.0
                ),
            },
        ));
    }

    if response.results.len() != request.ids.len() {
        return Err(Error::Elevation(
            crate::ElevationError::ElevateReportMismatched {
                path,
                why: format!(
                    "{} operations were sent and {} outcomes came back",
                    request.ids.len(),
                    response.results.len()
                ),
            },
        ));
    }

    Ok(response)
}

/// What a read of the *installed* helper found.
///
/// Three answers rather than a boolean, because "could not tell" is not "it is fine" — the T85
/// design, D5. The two that are not [`Administrative`](Trust::Administrative) both refuse, and they
/// carry different sentences because a person can act on one of them and not on the other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Trust {
    /// The file **and** its directory belong to an administrative account, and no other account may
    /// write either.
    Administrative,

    /// One of them does not, or somebody else may write it. Carries which.
    Writable(String),

    /// The machine would not say. Carries what it said instead.
    Unknown(String),
}

/// Where `mixengine-elevate` is, given the program that is asking and where this OS installs one.
///
/// **The installed copy first, the copy beside the program second, and a refusal in between** — the
/// T85 design, D5.
///
/// Until T85 this was *"beside whatever is running, and there is no override"* — the T40b design,
/// D9 — which was true for as long as a release put every binary in one directory. It stopped being
/// true the moment an installer put `mixengined` in the user's own directory and the one file this
/// product runs as root somewhere only an administrator can write, which is what T85 is for.
///
/// **There is still no override**, and D9's reasoning is untouched: a setting that chooses which
/// file is run as root is a setting that chooses which file is run as root. `installed` is not one
/// — it is where *this operating system* keeps a privileged helper, a constant compiled into
/// [`mixengine_platform::install`], threaded through as an argument so that a test can state which
/// machine it is describing rather than inheriting whichever one it happens to run on.
///
/// **The second candidate is a list rather than a path** — roadmap task **T88d**. Where a copy of
/// the helper ships is a fact about install formats, so it is
/// [`mixengine_platform::install::helper_sources`]'s to answer; what is decided here is the
/// preference between an installed helper and a shipped one, which has not changed. Every system's
/// list ends with the directory D9 already accepted, which is what a `cargo build` and a machine
/// before its first elevation prompt both use — and on the three formats an installer writes as
/// root it now holds one more entry, so that `mix uninstall` is no longer a one-way door.
///
/// The list is built **only when there is no installed helper to prefer**, because this function is
/// on the path of every `mix status`.
///
/// # Errors
///
/// [`ElevationError::ElevateMissing`] when there is no helper anywhere, which the daemon answers as
/// `dependency_missing`: nothing can be granted, and the fix is a reinstall rather than a retry.
/// [`ElevationError::ElevateUntrusted`] when the installed one is not an administrator's — the one case that
/// is **not** answered by falling back.
pub fn helper(program: &Path, installed: Option<&Path>) -> Result<PathBuf> {
    let installed = installed
        .filter(|path| path.is_file())
        .map(|path| (path.to_path_buf(), trust_of(path)));

    choose(installed, || {
        mixengine_platform::install::helper_sources(program, crate::window::BUNDLE)
            .into_iter()
            .map(|path| {
                let exists = path.is_file();

                (path, exists)
            })
            .collect()
    })
}

/// Whether a batch goes past the installed helper to the one this release ships — roadmap task
/// **T182b**, D3.
///
/// **Only when the installed helper cannot do the work**: it does not list an operation in the
/// batch among its `supported_ops`, *and* the shipped copy reports this release's
/// [`HELPER_VERSION`](mixengine_proto::privileged::HELPER_VERSION). Never merely because the
/// installed one is older: an older helper that knows `helper-replace` is replaced through the
/// signed path instead. The trust the shipped copy gets here is exactly what a first grant on a new
/// machine already gives it, which `docs/architecture/security-model.md` states as a residual.
#[derive(Debug, Clone, Copy)]
pub struct Bypass<'a> {
    /// What the installed helper answered its probe with.
    pub installed_ops: &'a [String],

    /// What the shipped copy answered its probe with, when it answered.
    pub shipped_version: Option<&'a str>,

    /// The wire names of the operations in the batch.
    pub batch_ops: &'a [&'a str],
}

impl Bypass<'_> {
    /// Does this batch go through the shipped copy?
    #[must_use]
    pub fn applies(&self) -> bool {
        let knows = |op: &str| self.installed_ops.iter().any(|known| known == op);

        let current = self.shipped_version == Some(mixengine_proto::privileged::HELPER_VERSION);
        let unreadable = self.batch_ops.iter().any(|op| !knows(op));

        // **And an install over a helper too old to replace itself** (D2's last row). Such a helper
        // does know `helper-install`, but run by it the operation copies *its own* image onto
        // itself and answers `AlreadyDone`: only the shipped copy can install anything newer.
        let install_over_old =
            self.batch_ops.contains(&"helper-install") && !knows("helper-replace");

        current && (unreadable || install_over_old)
    }
}

/// The copy of the helper this release ships, where it is on this machine — the first of the
/// platform's sources that exists, as [`helper`] would pick with nothing installed.
#[must_use]
pub fn shipped(program: &Path) -> Option<PathBuf> {
    mixengine_platform::install::helper_sources(program, crate::window::BUNDLE)
        .into_iter()
        .find(|path| path.is_file())
}

/// [`helper`], and T182b's D3: the shipped copy instead of the installed one when [`Bypass`]
/// applies.
///
/// **A refusal stays a refusal.** An installed helper that is not an administrator's is refused
/// before any bypass is considered: going around it would run the weaker arrangement at exactly the
/// moment somebody made it.
///
/// # Errors
///
/// As [`helper`].
pub fn helper_for(
    program: &Path,
    installed: Option<&Path>,
    bypass: Option<&Bypass<'_>>,
) -> Result<PathBuf> {
    let chosen = helper(program, installed)?;

    match bypass.filter(|bypass| bypass.applies()) {
        Some(_) => Ok(shipped(program).unwrap_or(chosen)),
        None => Ok(chosen),
    }
}

/// D5's table, over facts rather than over a filesystem.
///
/// Separated so that the table is a unit test: the row that matters most — an installed helper that
/// is **not** an administrator's — cannot be produced on a machine where the person running the
/// tests is not root, and a rule nobody can exercise is a rule nobody can trust.
///
/// **The sources arrive as a closure** — roadmap task **T88d** — so that *an installed helper is
/// preferred and nothing else is even looked at* is a property a test can hold by passing one that
/// panics, rather than a sentence in a comment. Each entry is a path and whether it is there.
pub(crate) fn choose(
    installed: Option<(PathBuf, Trust)>,
    sources: impl FnOnce() -> Vec<(PathBuf, bool)>,
) -> Result<PathBuf> {
    match installed {
        Some((path, Trust::Administrative)) => return Ok(path),

        // **Refused rather than downgraded.** Falling back here would quietly run the weaker
        // configuration at exactly the moment somebody has arranged for it — and that arrangement
        // is the one the root-owned directory exists to prevent. `elevation.status` reports this
        // through its `reason`, so it is on the screen before anybody clicks Allow.
        Some((path, Trust::Writable(why))) => {
            return Err(Error::Elevation(crate::ElevationError::ElevateUntrusted {
                path,
                why,
            }));
        }

        // Not knowing is not knowing it is safe. A daemon that cannot find out whether the file it
        // is about to run as root belongs to root has not learned that it does.
        Some((path, Trust::Unknown(why))) => {
            return Err(Error::Elevation(crate::ElevationError::ElevateUntrusted {
                path,
                why: format!("this machine would not say who owns it: {why}"),
            }));
        }

        None => {}
    }

    let sources = sources();

    // Named before the search consumes the list, because this message's whole job is to say where
    // it looked. The platform's **first** entry rather than the last one tried: that is the place
    // this system's installer is supposed to have left a copy.
    let looked = sources.first().map_or_else(
        || PathBuf::from("mixengine-elevate"),
        |(path, _)| path.clone(),
    );

    sources
        .into_iter()
        .find_map(|(path, exists)| exists.then_some(path))
        .ok_or(Error::Elevation(crate::ElevationError::ElevateMissing {
            path: looked,
        }))
}

/// Why a file about to be handed to an elevation prompt is not an administrator's, or [`None`] when
/// it is — roadmap task **T88d**.
///
/// **For the one caller that asks before spending a prompt**, which is the daemon deciding to
/// enqueue `HelperInstall {}`. What it reports is the residual
/// `docs/architecture/security-model.md` states for a machine with nothing installed: on Windows,
/// on the portable archives and — since T88d — inside a macOS bundle, the copy MixEngine would
/// install from sits where an ordinary account can arrange it.
///
/// **Deliberately not called by [`helper`]**, which every `mix status` and every poll a window makes
/// goes through: four ownership reads a second, with a warning line for each, is what *a source is
/// used and said out loud* must not cost.
#[must_use]
pub fn source_trust(path: &Path) -> Option<String> {
    match trust_of(path) {
        Trust::Administrative => None,
        Trust::Writable(why) | Trust::Unknown(why) => Some(why),
    }
}

/// Read the installed helper and its directory: who owns each, and whether anybody else may write.
///
/// Four reads and no privilege at all, which is what makes asking on every `elevation.status`
/// affordable. **The directory as well as the file**, because a file nobody may write inside a
/// directory anybody may write is a file anybody may replace.
fn trust_of(path: &Path) -> Trust {
    let Some(directory) = path.parent() else {
        return Trust::Unknown("it has no parent directory".to_owned());
    };

    for each in [path, directory] {
        match mixengine_platform::elevated::owner_of(each) {
            Ok(owner) if owner.is_administrative() => {}
            Ok(owner) => return Trust::Writable(format!("{} belongs to {owner}", each.display())),
            Err(error) => return Trust::Unknown(error.to_string()),
        }

        match mixengine_platform::elevated::others_can_write(each) {
            Ok(false) => {}
            Ok(true) => {
                return Trust::Writable(format!(
                    "{} can be written by an account other than its owner",
                    each.display()
                ));
            }
            Err(error) => return Trust::Unknown(error.to_string()),
        }
    }

    Trust::Administrative
}

#[cfg(test)]
mod tests;
