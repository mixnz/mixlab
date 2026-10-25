//! A service's state moving: the transition, what it notes on the row, and the start and end of a
//! run.

use mixengine_proto::{
    PackageVersion, ServiceId, ServiceState, ServiceTransition, StateReason, Timestamp,
};

use crate::{Error, Result, Store};

use super::{ServiceError, StoppedBy, parse_state};

/// A stored failure note, or [`None`] where there is none or it is one this build cannot read. Said
/// in `daemon.log` and not refused: a note is an explanation, and a row whose explanation a newer
/// build wrote must still list.
pub(super) fn failure_note(
    service: &str,
    stored: Option<&str>,
) -> Option<mixengine_proto::ServiceFailureNote> {
    let stored = stored?;

    match serde_json::from_str(stored) {
        Ok(note) => Some(note),
        Err(error) => {
            tracing::warn!(service, %error, "a failure note this build cannot read is ignored");
            None
        }
    }
}

/// A stored pid, or [`None`] where the column holds something no pid could be.
///
/// [`started`] writes an `i64` widened from a `u32`, so the narrowing here cannot lose one this
/// build wrote. A value that does not fit is a hand-edited row, and "no process" is the safer of the
/// two readings available: the alternative is handing a number to something that will signal it.
pub(super) fn process_id(stored: Option<i64>) -> Option<u32> {
    stored.and_then(|pid| u32::try_from(pid).ok())
}

/// The same, for a port — [`create`] writes a `u16` and nothing else ever writes the column.
///
/// A row holding a number no port could be is answered as no port rather than as a failure, on
/// [`process_id`]'s reasoning: what a hand-edited database gets to do is describe a service with no
/// port, not take a listing down.
///
/// [`create`]: super::create
pub(super) fn listening_port(stored: Option<i64>) -> Option<u16> {
    stored.and_then(|port| u16::try_from(port).ok())
}

/// The same, for a version — a text this build cannot parse is a blank cell, not a failed listing.
pub(super) fn installed_version(stored: Option<String>) -> Option<PackageVersion> {
    stored.and_then(|version| PackageVersion::parse(version).ok())
}

/// The same, for an exit code — [`ended`] writes an `i32`.
pub(super) fn exit_code(stored: Option<i64>) -> Option<i32> {
    stored.and_then(|code| i32::try_from(code).ok())
}

/// Move a service to `to`, and hand back the transition that was written.
///
/// The return value is the whole point: it is a [`ServiceTransition`], which is also what
/// [`mixengine_proto::DaemonEvent::ServiceStateChanged`] carries. The caller publishes the value
/// this function persisted rather than describing the same event a second time, so the row and the
/// event cannot drift — and because it only comes back on success, an event is impossible to
/// publish for a transition that did not happen.
///
/// `at` is passed in rather than read from the clock here for the same reason
/// [`Timestamp::from_system_time`] takes a [`std::time::SystemTime`]: the caller already has a
/// reading, and a test needs to be able to say when.
///
/// **The read and the write are one `BEGIN IMMEDIATE` transaction.** Two supervisors racing — a
/// health check going `Degraded` while a user's `service.stop` arrives — must not both see `Running`
/// and both write, and the plain `BEGIN` sqlx issues by default would not stop them: it takes no
/// lock, so the `SELECT` only pins a read snapshot and the `UPDATE` that follows has to *upgrade* to
/// a writer. In WAL mode that upgrade fails with `SQLITE_BUSY_SNAPSHOT` the moment anybody else has
/// committed since the snapshot — and SQLite deliberately does **not** run the busy handler for it,
/// because no amount of waiting can resolve it while this transaction still holds its old read. The
/// [`crate::Store`]'s `busy_timeout` would be bypassed and the loser would get a bare database
/// error instead of an answer.
///
/// `BEGIN IMMEDIATE` takes the write lock up front, so the two supervisors serialise at the `BEGIN`
/// — where `busy_timeout` does apply — and the second one reads the state the first one committed
/// and re-judges its move against it. The `WHERE state = ?` on the `UPDATE` stays as a cheap
/// assertion that this is really so; [`ServiceError::StateRaced`] is what it reports if it ever is
/// not.
///
/// # Errors
///
/// [`Error::NotFound`] when there is no such service; [`ServiceError::IllegalTransition`] when the
/// machine has no such edge, which is a bug in the caller rather than a condition to handle;
/// [`ServiceError::StateRaced`] when something else changed the state in between;
/// [`ServiceError::UnknownServiceState`] when the row holds a word this build does not recognise;
/// and [`Error::Database`] when the file cannot be written — including when the write lock could
/// not be taken within the store's `busy_timeout`.
pub async fn transition(
    store: &Store,
    service: &ServiceId,
    to: ServiceState,
    reason: StateReason,
    at: Timestamp,
) -> Result<ServiceTransition> {
    transition_noting(store, service, to, reason, None, at).await
}

/// [`transition`], carrying the sentence that explains a move into `failed`.
///
/// **In the transition's own transaction**, so a row cannot say `failed` without saying why: a note
/// written beside it could be the one write that did not land. `detail` is used only for a move into
/// `failed`, and the reason's own words stand in when it is [`None`]. A move into `running` clears
/// the note — a failure that has been recovered from is history.
///
/// # Errors
///
/// As [`transition`].
pub async fn transition_noting(
    store: &Store,
    service: &ServiceId,
    to: ServiceState,
    reason: StateReason,
    detail: Option<&str>,
    at: Timestamp,
) -> Result<ServiceTransition> {
    let id = service.as_str();

    // Not `begin()`: that is a deferred `BEGIN`, which would leave the `UPDATE` below to upgrade a
    // read snapshot into a write and fail unrecoverably against any concurrent writer. See above.
    let mut tx = store
        .pool()
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|source| store.failure("write", source))?;

    let stored = sqlx::query_scalar!("SELECT state FROM services WHERE id = ?", id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|source| store.failure("read", source))?
        .ok_or_else(|| Error::NotFound {
            kind: "service",
            id: id.to_owned(),
        })?;

    let from = parse_state(service, stored)?;

    if !from.can_become(to) {
        return Err(Error::Service(ServiceError::IllegalTransition {
            service: id.to_owned(),
            from,
            to,
        }));
    }

    let (next, current) = (to.as_str(), from.as_str());

    // **Written on the transition into `stopped`, never separately.**
    // On-demand activation may start a service a connection needed, and must not do it to one a
    // person stopped; a transition is not stored anywhere, so a daemon that restarts would
    // otherwise forget who had stopped which of its services. Set on every arrival at `stopped`, so
    // it can never be left over from an older stop, and reset on the way out so that a row which is
    // not stopped says what is true of it: nobody has stopped this.
    let stopped_by = match to {
        ServiceState::Stopped => StoppedBy::of(&reason),
        _ => StoppedBy::Never,
    }
    .as_str();

    let updated = sqlx::query!(
        "UPDATE services SET state = ?, stopped_by = ? WHERE id = ? AND state = ?",
        next,
        stopped_by,
        id,
        current
    )
    .execute(&mut *tx)
    .await
    .map_err(|source| store.failure("write", source))?;

    if updated.rows_affected() == 0 {
        return Err(Error::Service(ServiceError::StateRaced {
            service: id.to_owned(),
            expected: from,
        }));
    }

    // The note moves with the state, in this transaction.
    let note = match to {
        ServiceState::Failed => Some(
            serde_json::to_string(&mixengine_proto::ServiceFailureNote {
                at,
                reason: reason.clone(),
                detail: detail.map_or_else(|| reason.to_string(), str::to_owned),
            })
            .expect("a note is made of strings and a timestamp"),
        ),
        _ => None,
    };

    if note.is_some() || to == ServiceState::Running {
        sqlx::query!(
            "UPDATE services SET last_failure_json = ? WHERE id = ?",
            note,
            id
        )
        .execute(&mut *tx)
        .await
        .map_err(|source| store.failure("write", source))?;
    }

    tx.commit()
        .await
        .map_err(|source| store.failure("write", source))?;

    tracing::info!(service = id, %from, to = %to, ?reason, "service state changed");

    Ok(ServiceTransition {
        service: service.clone(),
        from,
        to,
        reason,
        at,
    })
}

/// Record the process this service is now running as.
///
/// **The pair is the point.** A pid on its own is not an identity — the OS reuses the number within
/// minutes — so what makes a row adoptable after a daemon restart is `pid` *and* the moment that
/// process began, read by
/// [`Supervised::started_at`](mixengine_platform::process::Supervised::started_at) while the child
/// is still held. `pid_start_time` is [`None`] where that reading could not be made — a process
/// that ended in its first milliseconds is the ordinary case — and a null column is the honest
/// answer for it: adoption refuses a row it cannot identify, where a zero would look like a
/// reading.
///
/// Separate from [`transition`] rather than folded into it, because these are facts about a process
/// and not an edge in the machine: the state was already written when the service reached
/// `Starting`, and a spawn is what happens after that.
///
/// # Errors
///
/// [`Error::NotFound`] when there is no such service, and [`Error::Database`] when the file cannot
/// be written.
pub async fn started(
    store: &Store,
    service: &ServiceId,
    pid: u32,
    pid_start_time: Option<i64>,
    at: Timestamp,
) -> Result<()> {
    let id = service.as_str();
    let (pid, at) = (i64::from(pid), at.0);

    let updated = sqlx::query!(
        "UPDATE services SET last_started_at = ?, pid = ?, pid_start_time = ? WHERE id = ?",
        at,
        pid,
        pid_start_time,
        id
    )
    .execute(store.pool())
    .await
    .map_err(|source| store.failure("write", source))?;

    if updated.rows_affected() == 0 {
        return Err(Error::NotFound {
            kind: "service",
            id: id.to_owned(),
        });
    }

    Ok(())
}

/// Record that the process is gone, and what it exited with.
///
/// Clearing `pid` and `pid_start_time` is the half that matters and is why this is not optional
/// bookkeeping: a row that keeps a dead pid is a row the next daemon adopts, and the number will by
/// then belong to something else. `code` is [`None`] where the OS reports none — a Unix process
/// killed by a signal — for the same reason [`mixengine_proto::StateReason::Exited`] carries an
/// option there: writing `0` for it would say "clean exit" about a crash.
///
/// # Errors
///
/// As [`started`].
pub async fn ended(store: &Store, service: &ServiceId, code: Option<i32>) -> Result<()> {
    let id = service.as_str();

    let updated = sqlx::query!(
        "UPDATE services SET pid = NULL, pid_start_time = NULL, last_exit_code = ? WHERE id = ?",
        code,
        id
    )
    .execute(store.pool())
    .await
    .map_err(|source| store.failure("write", source))?;

    if updated.rows_affected() == 0 {
        return Err(Error::NotFound {
            kind: "service",
            id: id.to_owned(),
        });
    }

    Ok(())
}
