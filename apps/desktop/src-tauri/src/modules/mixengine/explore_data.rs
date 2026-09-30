//! *Explore data* on the dashboard and "Open" on the Services detail screen — opens a `db` tab
//! straight in the running process, going neither through `database.open` (which starts an external
//! process) nor through the OS.
//!
//! Repeats exactly the three steps Phase 0 used for `mixlab://connect` — build a `Handoff`, keep it
//! in `HandoffState`, call `crate::launch::request` — differing only in that the `Handoff` is built
//! from `database.client` called right here, not from a URL read off the command line. See
//! `docs/specs/2026-09-06-mixengine-runtimes-services-logs-design.md`, section 4 and
//! Decision D2.

use serde_json::{json, Value};
use tauri::{AppHandle, State};

use crate::error::AppError;
use crate::launch::{self, TabRequest};
use crate::modules::db::handoff::{mongo_uri, Handoff, HandoffState};
use crate::modules::db::models::{ConnectionConfig, DbKind};
use crate::secrets::secrets_resolve_mixengine;

use super::rpc;

/// MixEngine's `DatabaseProtocol` has four values; an unknown one is the daemon talking about a
/// protocol these bindings do not know yet, not a programming error — return an
/// `unsupported_platform`-shaped error instead of panicking.
fn db_kind_of(protocol: &str) -> Result<DbKind, AppError> {
    match protocol {
        "mysql" => Ok(DbKind::Mysql),
        "postgres" => Ok(DbKind::Postgres),
        "redis" => Ok(DbKind::Redis),
        "mongodb" => Ok(DbKind::Mongo),
        other => Err(err!(
            "error.mixengineProtocol",
            message = format!("database.client answered an unknown protocol `{other}`")
        )),
    }
}

/// Opens a database service as a new `db` tab in the same process.
///
/// `database` is the specific database to open into, or `None` to open at server level. Never
/// takes or forwards the password outside this function — it lives in the local variable
/// `password` and only goes into the `Handoff` waiting for `db` to pick it up.
#[tauri::command]
pub async fn mixengine_database_explore_data(
    app: AppHandle,
    handoffs: State<'_, HandoffState>,
    service: String,
    database: Option<String>,
) -> Result<(), AppError> {
    let report: Value = rpc::call("database.client", json!({ "service": service })).await?;

    let protocol = report
        .get("protocol")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            err!(
                "error.mixengineProtocol",
                message = "database.client answered with no protocol for a service this screen \
                           should not have offered Open for"
            )
        })?;
    let kind = db_kind_of(protocol)?;

    // `database.client` carries no port — that is what `ServiceSummary` (from `service.list`)
    // declares, the same report `mixengine_services()` uses, not a field of this report.
    let services: Value = rpc::call("service.list", json!({})).await?;
    let port = services
        .get("services")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("id").and_then(Value::as_str) == Some(service.as_str()))
        })
        .and_then(|row| row.get("port"))
        .and_then(Value::as_u64)
        .and_then(|p| u16::try_from(p).ok());
    let port = port.ok_or_else(|| {
        err!(
            "error.mixengineProtocol",
            message = "this service's row has no usable port"
        )
    })?;

    let secret = report.get("secret").cloned();
    let (password, keyring_ref, username) = match &secret {
        Some(address) => {
            let key = address
                .get("key")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| {
                    err!(
                        "error.mixengineProtocol",
                        message = "database.client answered a secret with no key"
                    )
                })?;
            let password = secrets_resolve_mixengine(key.clone()).await?;
            // `key` is `"<service-id>/<user>"` — the part after the last `/` is the account.
            let user = key.rsplit('/').next().map(str::to_string);
            (password, Some(key), user)
        }
        None => (None, None, None),
    };

    // Mongo reads its address and its database out of one string and ignores the fields: the same
    // string `mixlab://connect` builds, from the same function.
    let (uri, database) = match kind {
        DbKind::Mongo => (
            Some(mongo_uri("127.0.0.1", port, database.as_deref())?),
            None,
        ),
        _ => (None, database),
    };

    let config = ConnectionConfig {
        kind,
        host: "127.0.0.1".to_string(),
        port,
        username,
        password,
        database,
        uri,
        path: None,
        ssh: None,
        use_ssl: None,
    };

    let handoff = Handoff {
        config,
        label: service.clone(),
        keyring_ref,
    };
    let id = handoffs.keep(handoff);
    launch::request(
        &app,
        TabRequest {
            module_id: "db",
            state: json!({ "handoffId": id }),
        },
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each protocol the daemon returns opens exactly one workspace — `mongodb` is the Mongo tab
    /// (T155).
    #[test]
    fn every_protocol_the_daemon_answers_opens_a_workspace() {
        assert_eq!(db_kind_of("mysql").unwrap(), DbKind::Mysql);
        assert_eq!(db_kind_of("postgres").unwrap(), DbKind::Postgres);
        assert_eq!(db_kind_of("redis").unwrap(), DbKind::Redis);
        assert_eq!(db_kind_of("mongodb").unwrap(), DbKind::Mongo);
        assert!(db_kind_of("mongo").is_err());
    }
}
