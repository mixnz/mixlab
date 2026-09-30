//! A connection handed to MixLab by another program — MixEngine's `mix database open` — as a
//! `mixlab://connect?…` URL, with the password in one environment variable rather than in the URL.
//!
//! This module understands the URL and keeps the result until the tab opened for it asks. It never
//! touches the environment: `crate::launch` reads the variable, and takes it out, on the first line
//! of `run()`, before this or anything else has started. The contract both sides implement is
//! written up in `docs/specs/2026-09-03-mixengine-connection-handoff-design.md`.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;

use super::models::{ConnectionConfig, DbKind};
use crate::error::AppError;

/// What the URL said, as a connection the form can hold and `connect_db` can dial.
///
/// `Debug` is derived because [`ConnectionConfig`]'s own is written by hand to redact the
/// password; the test at the bottom holds that line for this struct too.
#[derive(Debug, Clone, Serialize)]
pub struct Handoff {
    pub config: ConnectionConfig,
    /// The tab's name and the name pre-filled for saving — MixEngine's service id, `mariadb@main`.
    pub label: String,
    /// The key half of this account's address in MixEngine's keyring entry — `service="mixengine"`
    /// is a compile-time constant on the side that reads it and never travels on the wire (T84's
    /// D5). `Some` only when `secret` proved this process was actually started by MixEngine: a
    /// `mixlab://` link can name any `secret_key` it likes, but it cannot set an environment
    /// variable for the process it starts, so a value here without that proof would let a forged
    /// link get a saved connection pointed at an arbitrary MixEngine account.
    pub keyring_ref: Option<String>,
}

/// The environment variable `password_env` points at, when its name is one a launcher would use.
///
/// Trusted only inside the launcher's namespace — `MIX…_…PASSWORD`: `MIXENGINE_DB_PASSWORD`,
/// `MIXLAB_DB_PASSWORD`. Once the scheme is registered with the OS, any web page can produce a
/// `mixlab://` link naming any variable, and a name outside that namespace is how `$HOME` would
/// otherwise be sent to a stranger's server as a password. The check is on the *name*; whether the
/// variable exists is the caller's to find out. Nothing here reads the environment.
pub fn credential_name(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let name = first(&parsed, "password_env")?;
    if names_a_launcher_credential(&name) {
        Some(name)
    } else {
        eprintln!("mixlab: ignoring password_env={name}: not a launcher's credential variable");
        None
    }
}

/// `^MIX[A-Z0-9]*_[A-Z0-9_]*PASSWORD$`, spelled out rather than pulled in as a regex crate.
fn names_a_launcher_credential(name: &str) -> bool {
    let Some(middle) = name
        .strip_prefix("MIX")
        .and_then(|rest| rest.strip_suffix("PASSWORD"))
    else {
        return false;
    };
    middle.contains('_')
        && middle
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// The URL, and the password read for it elsewhere, as a connection.
///
/// Pure: the environment and the app are both somebody else's. `secret` is whatever the caller
/// found under the variable [`credential_name`] named — `None` for a server with no accounts, and
/// for a URL that arrived any way other than on the command line of a fresh process.
pub fn parse(url: &str, secret: Option<String>) -> Result<Handoff, AppError> {
    let parsed = url::Url::parse(url).map_err(|e| invalid(format!("not a URL: {e}")))?;
    if parsed.scheme() != "mixlab" {
        return Err(invalid(format!(
            "the scheme is {}, not mixlab",
            parsed.scheme()
        )));
    }
    if parsed.host_str() != Some("connect") {
        return Err(invalid("only mixlab://connect is understood"));
    }

    let kind = match first(&parsed, "kind").as_deref() {
        Some("mysql") => DbKind::Mysql,
        Some("postgres") => DbKind::Postgres,
        Some("redis") => DbKind::Redis,
        // Same shape as the three above — host/port/user/database, no `uri` of its own.
        Some("clickhouse") => DbKind::Clickhouse,
        /* MixEngine's word for it, which is the protocol's and the URI scheme's; `mongo` is this
        application's own and stays refused below. A Mongo connection is one connection string,
        so the fields are turned into one by `mongo_uri` after they have been read. */
        Some("mongodb") => DbKind::Mongo,
        /* Refused by name rather than by falling through, because the reason is not "not supported
        yet". `mixlab://` is registered with the operating system, so any web page can hand this
        process a URL; a `kind=sqlite&path=…` would be that page choosing which file on the
        user's disk MixLab opens. Nothing else here names a local path, which is what makes this
        kind the exception. */
        Some("sqlite") => {
            return Err(invalid(
                "kind `sqlite` names a file on this machine, and is not opened from a URL",
            ))
        }
        Some(other) => {
            return Err(invalid(format!(
                "kind `{other}` is not one MixLab opens this way"
            )))
        }
        None => return Err(invalid("kind is missing")),
    };
    let host = present(&parsed, "host").ok_or_else(|| invalid("host is missing"))?;
    let port = present(&parsed, "port").ok_or_else(|| invalid("port is missing"))?;
    let port = port
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(|| invalid(format!("port `{port}` is not a TCP port")))?;
    let label = present(&parsed, "label").unwrap_or_else(|| format!("{host}:{port}"));
    // Only trusted alongside a `secret` that came from this process's own environment — see
    // `Handoff::keyring_ref`. Read before `secret` is moved into the config below.
    let keyring_ref = secret
        .is_some()
        .then(|| present(&parsed, "secret_key"))
        .flatten();

    // Mongo reads its address and its database out of the string and ignores the fields, so both
    // go into it; a MongoDB MixEngine runs has no accounts, so no user goes anywhere.
    let (uri, username, database) = match kind {
        DbKind::Mongo => (
            Some(mongo_uri(
                &host,
                port,
                present(&parsed, "database").as_deref(),
            )?),
            None,
            None,
        ),
        _ => (None, present(&parsed, "user"), present(&parsed, "database")),
    };

    Ok(Handoff {
        config: ConnectionConfig {
            kind,
            host,
            port,
            username,
            password: secret,
            database,
            uri,
            path: None,
            ssh: None,
            // "Try TLS, fall back to plaintext": right for MixEngine's loopback servers, which
            // speak none, and not wrong for a server that does.
            use_ssl: None,
        },
        label,
        keyring_ref,
    })
}

/// The connection string a MongoDB handed over by MixEngine is dialled with — roadmap task T155.
///
/// `mongodb://<host>:<port>/<database>?directConnection=true`. Shared by this module's URL and by
/// the Services screen's Open (`explore_data.rs`), so the two doors cannot build two strings.
///
/// **The host is an address or a plain name, and nothing else.** A `mixlab://` link can come from
/// any web page, and a host like `a/?authSource=x` would otherwise write options into the string.
/// `directConnection=true` because a standalone development server is not a replica set, and a
/// driver told nothing tries to discover one.
pub fn mongo_uri(host: &str, port: u16, database: Option<&str>) -> Result<String, AppError> {
    let authority = match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V6(address)) => format!("[{address}]"),
        Ok(address) => address.to_string(),
        Err(_)
            if !host.is_empty()
                && host
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-') =>
        {
            host.to_string()
        }
        Err(_) => {
            return Err(invalid(format!(
                "host `{host}` is not an address a connection string can carry"
            )))
        }
    };
    let path = database.map(encode).unwrap_or_default();

    Ok(format!(
        "mongodb://{authority}:{port}/{path}?directConnection=true"
    ))
}

/// Percent-encode everything outside RFC 3986's unreserved set.
fn encode(value: &str) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            }
            _ => {
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

/// The first value under `key`, percent-decoded. A repeated key is the first one's.
fn first(url: &url::Url, key: &str) -> Option<String> {
    url.query_pairs()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.into_owned())
}

/// [`first`], with an empty value counting as absent.
fn present(url: &url::Url, key: &str) -> Option<String> {
    first(url, key).filter(|value| !value.is_empty())
}

fn invalid(message: impl std::fmt::Display) -> AppError {
    err!("error.handoffInvalid", message = message)
}

/// Handoffs accepted and not yet opened, by the id the tab was told.
///
/// The one place a handed-over password sits in memory on this side other than the
/// `ConnectionConfig` on its way through `connect_db`. Each entry leaves on the first `take`:
/// a tab restored from an old session with an old id finds nothing, and shows an empty form.
#[derive(Default)]
pub struct HandoffState {
    pending: Mutex<HashMap<String, Handoff>>,
}

impl HandoffState {
    /// Keeps `handoff` and answers the id a tab can take it back with.
    pub fn keep(&self, handoff: Handoff) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.clone(), handoff);
        id
    }

    /// The handoff under `id`, removed — or `None` when it was never there or already taken.
    pub fn take(&self, id: &str) -> Option<Handoff> {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id)
    }
}

/// A URL that arrived — on the command line, over the channel from a second copy, or from the OS —
/// turned into a pending handoff and a tab for it. Called by `crate::launch`, which is the one
/// place a URL is matched to a module.
pub fn accept<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    url: &str,
    secret: Option<String>,
) -> Result<(), AppError> {
    use tauri::Manager;

    let handoff = parse(url, secret)?;
    let id = app.state::<HandoffState>().keep(handoff);
    crate::launch::request(
        app,
        crate::launch::TabRequest {
            module_id: "db",
            state: serde_json::json!({ "handoffId": id }),
        },
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str =
        "mixlab://connect?kind=mysql&host=127.0.0.1&port=3306&user=blog&database=blog\
                        &label=mariadb%40main&password_env=MIXENGINE_DB_PASSWORD\
                        &secret_key=mariadb%40main%2Fblog";

    /// The whole shape, every optional part present, the label decoded, and — because `secret`
    /// proves this process was started by MixEngine — the keyring reference carried too.
    #[test]
    fn a_full_url_reads_as_a_connection() {
        let handoff = parse(FULL, Some("s3cret".to_string())).unwrap();
        assert_eq!(handoff.config.kind, DbKind::Mysql);
        assert_eq!(handoff.config.host, "127.0.0.1");
        assert_eq!(handoff.config.port, 3306);
        assert_eq!(handoff.config.username.as_deref(), Some("blog"));
        assert_eq!(handoff.config.password.as_deref(), Some("s3cret"));
        assert_eq!(handoff.config.database.as_deref(), Some("blog"));
        assert_eq!(handoff.config.uri, None);
        assert!(handoff.config.ssh.is_none());
        assert_eq!(handoff.config.use_ssl, None);
        assert_eq!(handoff.label, "mariadb@main");
        assert_eq!(handoff.keyring_ref.as_deref(), Some("mariadb@main/blog"));
    }

    /// A `secret_key` with no `secret` behind it names nothing: this is what a `mixlab://` link
    /// clicked from a browser looks like, and it must not be able to point a saved connection at
    /// an arbitrary MixEngine account just by naming one in the URL.
    #[test]
    fn a_secret_key_without_a_proven_secret_is_not_a_keyring_ref() {
        let handoff = parse(FULL, None).unwrap();
        assert_eq!(handoff.config.password, None);
        assert_eq!(handoff.keyring_ref, None);
    }

    /// A server with no accounts hands over an address and a label and nothing else.
    #[test]
    fn a_redis_url_names_no_account() {
        let handoff = parse(
            "mixlab://connect?kind=redis&host=127.0.0.1&port=6379&label=redis%40main",
            None,
        )
        .unwrap();
        assert_eq!(handoff.config.kind, DbKind::Redis);
        assert_eq!(handoff.config.username, None);
        assert_eq!(handoff.config.password, None);
        assert_eq!(handoff.config.database, None);
        assert_eq!(handoff.label, "redis@main");
    }

    /// ClickHouse is a server with an account, same as MySQL and PostgreSQL — unlike Redis, which
    /// carries no username at all.
    #[test]
    fn a_clickhouse_url_reads_as_a_connection() {
        let handoff = parse(
            "mixlab://connect?kind=clickhouse&host=127.0.0.1&port=8123&user=admin&database=analytics\
             &label=clickhouse%40main",
            Some("s3cret".to_string()),
        )
        .unwrap();
        assert_eq!(handoff.config.kind, DbKind::Clickhouse);
        assert_eq!(handoff.config.username.as_deref(), Some("admin"));
        assert_eq!(handoff.config.password.as_deref(), Some("s3cret"));
        assert_eq!(handoff.config.database.as_deref(), Some("analytics"));
        assert_eq!(handoff.label, "clickhouse@main");
    }

    /// A MongoDB MixEngine runs, as `mix database open` hands it over — roadmap task T155.
    #[test]
    fn a_mongodb_url_reads_as_a_connection_string() {
        let handoff = parse(
            "mixlab://connect?kind=mongodb&host=127.0.0.1&port=27017&database=blog\
             &label=mongodb%40main",
            None,
        )
        .unwrap();
        assert_eq!(handoff.config.kind, DbKind::Mongo);
        assert_eq!(
            handoff.config.uri.as_deref(),
            Some("mongodb://127.0.0.1:27017/blog?directConnection=true")
        );
        assert_eq!(handoff.config.username, None);
        assert_eq!(handoff.config.database, None);
        assert_eq!(handoff.label, "mongodb@main");
    }

    /// A URL can come from any web page, so the host is an address or a plain name and nothing
    /// that would write options into the string; a database name is escaped.
    #[test]
    fn a_mongo_uri_carries_an_address_and_nothing_that_writes_options() {
        assert_eq!(
            mongo_uri("::1", 27017, None).unwrap(),
            "mongodb://[::1]:27017/?directConnection=true"
        );
        assert_eq!(
            mongo_uri("db.local", 1, Some("a b/c")).unwrap(),
            "mongodb://db.local:1/a%20b%2Fc?directConnection=true"
        );
        for host in ["a/?authSource=x", "a@b", "", "a b"] {
            assert_eq!(
                mongo_uri(host, 1, None).unwrap_err().code,
                "error.handoffInvalid",
                "{host}"
            );
        }
    }

    #[test]
    fn a_missing_label_is_the_address() {
        let handoff = parse(
            "mixlab://connect?kind=postgres&host=db.local&port=5432",
            None,
        )
        .unwrap();
        assert_eq!(handoff.label, "db.local:5432");
    }

    /// Everything that is not a connection MixLab can open, each refused by name.
    #[test]
    fn what_cannot_be_opened_is_refused() {
        for url in [
            "not a url",
            "https://connect?kind=mysql&host=h&port=1",
            // Another program's scheme: only `mixlab://` is answered (ADR 0047).
            "otherapp://connect?kind=mysql&host=h&port=1",
            "mixlab://open?kind=mysql&host=h&port=1",
            "mixlab://connect?host=h&port=1",
            "mixlab://connect?kind=mongo&host=h&port=1",
            "mixlab://connect?kind=mysql&port=1",
            "mixlab://connect?kind=mysql&host=&port=1",
            "mixlab://connect?kind=mysql&host=h",
            "mixlab://connect?kind=mysql&host=h&port=0",
            "mixlab://connect?kind=mysql&host=h&port=70000",
            "mixlab://connect?kind=mysql&host=h&port=abc",
            // Refused whether or not it is well formed, and whether or not a path is offered: a
            // URL is not allowed to choose a file on this machine. See the arm in `parse`.
            "mixlab://connect?kind=sqlite&host=h&port=1",
            "mixlab://connect?kind=sqlite&path=C:%5CUsers%5Csomeone%5Cblog.db",
        ] {
            let error = parse(url, None).expect_err(url);
            assert_eq!(error.code, "error.handoffInvalid", "{url}");
            assert!(error.params.contains_key("message"), "{url}");
        }
    }

    /// The variable named by the URL is trusted only inside the launcher's own namespace: a link
    /// on a web page can name any variable it likes, and this is what keeps `$HOME` from being
    /// sent to a stranger's server as a password.
    #[test]
    fn only_a_launcher_credential_variable_is_named() {
        let named = |name: &str| {
            credential_name(&format!("mixlab://connect?kind=redis&password_env={name}"))
        };
        assert_eq!(
            named("MIXENGINE_DB_PASSWORD").as_deref(),
            Some("MIXENGINE_DB_PASSWORD")
        );
        assert_eq!(
            named("MIXLAB_DB_PASSWORD").as_deref(),
            Some("MIXLAB_DB_PASSWORD")
        );
        assert_eq!(named("MIX_PASSWORD").as_deref(), Some("MIX_PASSWORD"));
        for refused in [
            "PATH",
            "HOME",
            "DB_PASSWORD",
            "PGPASSWORD",
            "MIXPASSWORD",
            "mixengine_db_password",
            "MIXENGINE_DB_PASSWORD_",
            "",
        ] {
            assert_eq!(named(refused), None, "{refused}");
        }
        assert_eq!(credential_name("mixlab://connect?kind=redis"), None);
        assert_eq!(credential_name("not a url"), None);
    }

    /// The name is read off a URL that would not parse as a connection: the variable has to be
    /// taken out of the environment whether or not the rest of the URL is any good.
    #[test]
    fn the_credential_name_survives_a_broken_url() {
        assert_eq!(
            credential_name("mixlab://connect?password_env=MIXENGINE_DB_PASSWORD").as_deref(),
            Some("MIXENGINE_DB_PASSWORD")
        );
    }

    #[test]
    fn a_handoff_never_prints_its_password() {
        let handoff = parse(FULL, Some("hunter2".to_string())).unwrap();
        let printed = format!("{handoff:?}");
        assert!(!printed.contains("hunter2"), "{printed}");
        assert!(printed.contains("mariadb@main"));
    }

    /// The pending store hands each handoff out once.
    #[test]
    fn a_kept_handoff_is_taken_once() {
        let state = HandoffState::default();
        let id = state.keep(parse(FULL, None).unwrap());
        assert!(!id.is_empty());
        assert_eq!(
            state.take(&id).map(|h| h.label).as_deref(),
            Some("mariadb@main")
        );
        assert!(state.take(&id).is_none());
        assert!(state.take("nothing").is_none());
    }
}
