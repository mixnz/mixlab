//! Writing a service's row: declaring it, removing it, and the settings a person changes on it.

use mixengine_proto::{ResourceLimits, ServiceId};

use crate::{Error, Result, Store};

use super::{Declaration, Origin, Port, ServiceError, Written, data_dir, ports};

/// Write down a service somebody asked for.
///
/// **The row and nothing else.** Whether the package has a recipe, whether that version is
/// installed and whether the id's shape suits the recipe are decided by the caller, and rendering
/// the configuration happens after this returns — which is what makes [`delete`] the rollback for a
/// rendering that failed.
///
/// # Errors
///
/// [`ServiceError::ServiceAlreadyDeclared`] when a row with this id exists or the parent already
/// has an instance of this name, [`ServiceError::DataDirectoryTaken`] when another service is
/// already pointed at the `data_dir` this one names, [`Error::NotFound`] when the [`Origin`] names
/// a package or a runtime that is not installed, and [`Error::Database`] when the row cannot be
/// written.
pub async fn create(
    store: &Store,
    host: &dyn mixengine_platform::Host,
    declaration: &Declaration,
) -> Result<Written> {
    let Declaration {
        service,
        origin,
        instance_name,
        port,
        bind_addr,
        data_dir,
        autostart,
        overrides,
    } = declaration;

    let id = service.as_str();
    let autostart_column = i64::from(*autostart);

    // Held until this function returns, which is what makes the number below still free when the
    // row that claims it lands — see [`ports::hold`].
    let _allocating = ports::hold().await;

    // Inside that lock for the same reason the port search is: two calls naming one directory both
    // read a table neither of them has written to yet, and both are told it is free.
    if let Some(path) = data_dir
        && let Some(holder) = data_dir::held_by(store, path).await?
    {
        return Err(Error::Service(ServiceError::DataDirectoryTaken {
            path: path.clone(),
            holder,
        }));
    }

    let chosen = match *port {
        Port::None => Written {
            port: None,
            moved_from: None,
        },

        Port::Fixed(port) => Written {
            port: Some(port),
            moved_from: None,
        },

        Port::Allocate { preferred } => {
            let bind = ports::bind_address(bind_addr.as_deref());
            let allocation = ports::allocate(store, host, bind, preferred).await?;

            Written {
                port: Some(allocation.port),
                moved_from: allocation.moved_from,
            }
        }
    };

    let port_column = chosen.port.map(i64::from);

    // Checked here as well as by the caller, because the alternative is a constraint violation
    // whose message names a column: the row and the lookup are one statement otherwise, and a
    // subquery that found nothing is not a failure SQLite explains.
    let (package_id, runtime_install_id, extension_id) = match origin {
        Origin::Extension { id } => {
            let id_column = id.as_str();

            let found: Option<String> =
                sqlx::query_scalar!("SELECT id FROM extensions WHERE id = ?", id_column)
                    .fetch_optional(store.pool())
                    .await
                    .map_err(|source| store.failure("read", source))?;

            let found = found.ok_or_else(|| Error::NotFound {
                kind: "extension",
                id: id_column.to_owned(),
            })?;

            (None, None, Some(found))
        }

        Origin::Package { name, version } => {
            let version_column = version.as_str();

            let found: Option<i64> = sqlx::query_scalar!(
                "SELECT id FROM packages WHERE name = ? AND version = ?",
                name,
                version_column
            )
            .fetch_optional(store.pool())
            .await
            .map_err(|source| store.failure("read", source))?;

            let found = found.ok_or_else(|| Error::NotFound {
                kind: "package",
                id: format!("{name} {version}"),
            })?;

            (Some(found), None, None)
        }

        Origin::Runtime { kind, version } => {
            let kind_column = kind.as_str();
            let version_column = version.as_str();

            let found: Option<i64> = sqlx::query_scalar!(
                "SELECT id FROM runtime_installs WHERE kind = ? AND version = ?",
                kind_column,
                version_column
            )
            .fetch_optional(store.pool())
            .await
            .map_err(|source| store.failure("read", source))?;

            let found = found.ok_or_else(|| Error::NotFound {
                kind: "runtime",
                id: format!("{kind_column} {version_column}"),
            })?;

            (None, Some(found), None)
        }
    };

    let written = sqlx::query!(
        "INSERT INTO services
             (id, package_id, runtime_install_id, extension_id, instance_name, state, autostart,
              port, bind_addr, data_dir, config_overrides_json)
         VALUES (?, ?, ?, ?, ?, 'stopped', ?, ?, COALESCE(?, '127.0.0.1'), ?, ?)
         ON CONFLICT DO NOTHING",
        id,
        package_id,
        runtime_install_id,
        extension_id,
        instance_name,
        autostart_column,
        port_column,
        bind_addr,
        data_dir,
        overrides
    )
    .execute(store.pool())
    .await
    .map_err(|source| store.failure("write", source))?;

    // `DO NOTHING` over every unique constraint rather than letting one raise: the id, and one
    // `(parent, instance_name)` per kind of parent — and what a person did wrong is the same in all
    // of those cases, they asked for a service that is already here.
    if written.rows_affected() == 0 {
        return Err(Error::Service(ServiceError::ServiceAlreadyDeclared {
            service: service.clone(),
        }));
    }

    tracing::info!(%id, origin = ?origin, "a service was created");

    Ok(chosen)
}

/// Remove a service's row, and say what its `data_dir` column held.
///
/// **The column verbatim, not the directory it resolves to.** [`None`] is a row that left the
/// placement to the home's layout, and only the generator knows what that layout made of it — so the
/// caller reconstructs it, and this stays a function about a table.
///
/// # Errors
///
/// [`Error::NotFound`] when there is no such row, and [`Error::Database`] when it cannot be written.
pub async fn delete(store: &Store, service: &ServiceId) -> Result<Option<String>> {
    let id = service.as_str();

    let removed = sqlx::query_scalar!("DELETE FROM services WHERE id = ? RETURNING data_dir", id)
        .fetch_optional(store.pool())
        .await
        .map_err(|source| store.failure("write", source))?
        .ok_or_else(|| Error::NotFound {
            kind: "service",
            id: id.to_owned(),
        })?;

    tracing::info!(%id, "a service was deleted");

    Ok(removed)
}

/// Replace how long a service may look idle before it is stopped.
///
/// **Three states, and the caller says which one it means.** `None` writes SQL `NULL`, "use
/// whatever the recipe wants"; `Some(0)` writes `0`, "never, whatever the recipe wants"; `Some(n)`
/// is minutes. They are different rows on purpose — a default arriving in a later release must
/// reach the first and must never reach the second, and once both have been stored as `NULL` no
/// migration can tell them apart again.
///
/// Writes the row and nothing else. The policy reaches a running service the way `limits_json`
/// does: through the next generation pass, which the registry makes at the top of every
/// `service.*` call.
///
/// # Errors
///
/// [`Error::NotFound`] when no such service is declared, and the store's own failure when the write
/// does not land.
pub async fn set_idle(store: &Store, service: &ServiceId, minutes: Option<u32>) -> Result<()> {
    let id = service.as_str();
    let minutes = minutes.map(i64::from);

    let changed = sqlx::query!(
        "UPDATE services SET idle_minutes = ? WHERE id = ?",
        minutes,
        id
    )
    .execute(store.pool())
    .await
    .map_err(|source| store.failure("write", source))?
    .rows_affected();

    if changed == 0 {
        return Err(Error::NotFound {
            kind: "service",
            id: id.to_owned(),
        });
    }

    tracing::info!(%id, ?minutes, "a service's idle policy was replaced");

    Ok(())
}

/// Replace whether this service starts when the daemon does.
///
/// [`set_idle`]'s shape, and for its reason: the column is the whole of the setting, and what reads
/// it is a walk that has not happened yet. **Nothing is started or stopped by this call.**
///
/// **A `bool` and not an `Option<bool>`.** `set_idle` takes three states because an absent value
/// restores the recipe's own default; no recipe declares an autostart, so there is no third state
/// for one to restore.
///
/// # Errors
///
/// [`Error::NotFound`] when no row has that id — the same refusal [`set_idle`] gives, and for the
/// same reason: a setting accepted for a name nobody has is a row nobody can read back.
/// [`Error::Database`] when the file cannot be written.
pub async fn set_autostart(store: &Store, service: &ServiceId, autostart: bool) -> Result<()> {
    let id = service.as_str();
    let column = i64::from(autostart);

    let changed = sqlx::query!("UPDATE services SET autostart = ? WHERE id = ?", column, id)
        .execute(store.pool())
        .await
        .map_err(|source| store.failure("write", source))?
        .rows_affected();

    if changed == 0 {
        return Err(Error::NotFound {
            kind: "service",
            id: id.to_owned(),
        });
    }

    tracing::info!(%id, autostart, "a service's autostart setting was replaced");

    Ok(())
}

/// The services whose `autostart` column is set.
///
/// **The ids and not the rows**, because what the caller does with them is build a
/// [`ServiceGraph::start_plan`](crate::services::graph::ServiceGraph::start_plan): the plan pulls in
/// what each one depends on, so a walk built from a filtered *listing* would be a walk that starts a
/// pool before its database.
///
/// A row holding an id this build cannot parse is passed over rather than failing the read, on
/// [`records`]'s reasoning: a hand-edited row gets to describe a service nobody starts, not to stop
/// every other service on the machine from starting.
///
/// # Errors
///
/// [`Error::Database`] when the table cannot be read.
///
/// [`records`]: super::records
pub async fn autostart_ids(store: &Store) -> Result<Vec<ServiceId>> {
    let rows = sqlx::query_scalar!("SELECT id FROM services WHERE autostart != 0 ORDER BY id")
        .fetch_all(store.pool())
        .await
        .map_err(|source| store.failure("read", source))?;

    Ok(rows
        .iter()
        .filter_map(|id| ServiceId::parse(id).ok())
        .collect())
}

/// What `services.idle_minutes` holds for this service.
///
/// The raw column rather than a resolved duration, because resolving it needs the recipe's default
/// and this crate's callers hold the catalogue that has it. What turns the two into an answer a
/// person reads is [`IdleSource::of`](mixengine_proto::IdleSource::of).
///
/// # Errors
///
/// [`Error::NotFound`] when no such service is declared, and the store's own failure when the read
/// does not land.
pub async fn idle_minutes(store: &Store, service: &ServiceId) -> Result<Option<i64>> {
    let id = service.as_str();

    let row = sqlx::query!(
        r#"SELECT idle_minutes AS "idle_minutes: i64" FROM services WHERE id = ?"#,
        id
    )
    .fetch_optional(store.pool())
    .await
    .map_err(|source| store.failure("read", source))?
    .ok_or_else(|| Error::NotFound {
        kind: "service",
        id: id.to_owned(),
    })?;

    Ok(row.idle_minutes)
}

/// Replace what a service may take.
///
/// **The whole value, never a delta.** `limits_json` holds a complete `ResourceLimits`, so a write
/// that merged would need to read first, and two clients capping one service would then race over
/// which read won. Writing the document the caller handed over is one statement about one row.
///
/// Writes the row and nothing else: applying the ceilings to a *running* process is the daemon's, in
/// `Registry::set_limits`, because this crate has no supervisor. A service that is stopped needs
/// nothing else — the next spawn reads this row.
///
/// # Errors
///
/// [`Error::NotFound`] when no such service is declared, and the store's own failure when the write
/// does not land.
pub async fn set_limits(store: &Store, service: &ServiceId, limits: ResourceLimits) -> Result<()> {
    let id = service.as_str();
    // Three scalar fields and an enum: this cannot fail, and the alternative to saying so is an
    // error variant no caller could ever act on. `recipe.rs` makes the same claim the same way.
    let document = serde_json::to_string(&limits).expect("a set of resource limits serialises");

    let changed = sqlx::query!(
        "UPDATE services SET limits_json = ? WHERE id = ?",
        document,
        id
    )
    .execute(store.pool())
    .await
    .map_err(|source| store.failure("write", source))?
    .rows_affected();

    if changed == 0 {
        return Err(Error::NotFound {
            kind: "service",
            id: id.to_owned(),
        });
    }

    tracing::info!(%id, %document, "a service's resource limits were replaced");

    Ok(())
}
