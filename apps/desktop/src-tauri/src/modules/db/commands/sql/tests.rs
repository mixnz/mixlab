use super::session_of;
use crate::modules::db::state::DbState;

/// Every command `sql.rs` defines.
const COMMANDS: [&str; 32] = [
    "sql_list_databases",
    "sql_server_info",
    "sql_list_tables",
    "sql_table_stats",
    "sql_table_data",
    "sql_table_structure",
    "sql_schema_outline",
    "sql_update_row",
    "sql_insert_rows",
    "sql_delete_rows",
    "sql_create_table",
    "sql_rename_table",
    "sql_drop_table",
    "sql_add_column",
    "sql_modify_column",
    "sql_drop_column",
    "sql_validate_sql",
    "sql_create_database",
    "sql_drop_database",
    "sql_collations",
    "sql_add_index",
    "sql_modify_index",
    "sql_drop_index",
    "sql_add_skip_index",
    "sql_modify_skip_index",
    "sql_drop_skip_index",
    "sql_rebuild_order_by",
    "sql_row_count",
    "sql_run_script",
    "sql_cancel_query",
    "sql_dump",
    "sql_restore",
];

/// `invoke` of a command nobody registered fails only when it is called, and nothing at build time
/// says so. Every `sql_*` command is in `modules/mod.rs`, and nothing else under that name is.
#[test]
fn every_sql_command_is_registered() {
    let handler = include_str!("../../../mod.rs");
    for name in COMMANDS {
        assert!(
            handler.contains(&format!("db::commands::sql::{name},")),
            "{name} is not registered in modules/mod.rs"
        );
    }
    assert_eq!(
        handler.matches("db::commands::sql::").count(),
        COMMANDS.len(),
        "modules/mod.rs registers a sql_ command that COMMANDS does not list"
    );
}

/// The frontend's `sqlApi`, which is the only caller of these commands.
const SQL_API: &str = include_str!("../../../../../../src/modules/db/sql/api.ts");

/// The keys of the object `sqlApi` hands to `invoke("<name>", …)`, sorted.
fn invoked_keys(name: &str) -> Vec<String> {
    let call = format!("(\"{name}\", {{");
    let start = SQL_API
        .find(&call)
        .unwrap_or_else(|| panic!("sqlApi never invokes {name}"))
        + call.len();
    let end = start + SQL_API[start..].find('}').unwrap();
    let mut keys: Vec<String> = SQL_API[start..end]
        .split(',')
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
        .collect();
    keys.sort();
    keys
}

/// The arguments the Rust command takes from the payload, in camelCase as Tauri reads them, sorted.
fn command_keys(name: &str) -> Vec<String> {
    let source = include_str!("../sql.rs");
    let head = format!("pub async fn {name}(");
    let start = source.find(&head).unwrap() + head.len();
    let end = start + source[start..].find(") -> ").unwrap();
    // One parameter per line, as rustfmt lays a signature out; `State<'_, DbState>` has a comma.
    let mut keys: Vec<String> = source[start..end]
        .lines()
        .filter_map(|param| param.split(':').next())
        .map(str::trim)
        .filter(|name| !name.is_empty() && *name != "app" && *name != "state")
        .map(|name| {
            let mut parts = name.split('_');
            let first = parts.next().unwrap().to_string();
            parts.fold(first, |camel, part| {
                camel + &part[..1].to_uppercase() + &part[1..]
            })
        })
        .collect();
    keys.sort();
    keys
}

/// Every command `sqlApi` calls is one of these, every one of these is called, and each call sends
/// exactly the keys its command reads. A key spelled differently on the two sides arrives as a
/// missing argument, which Tauri reports only at the moment of the call.
#[test]
fn sql_api_calls_every_command_with_the_arguments_it_reads() {
    let mut invoked: Vec<&str> = SQL_API
        .match_indices("(\"sql_")
        .map(|(at, _)| {
            let name = &SQL_API[at + 2..];
            &name[..name.find('"').unwrap()]
        })
        .collect();
    invoked.sort();
    let mut expected = COMMANDS.to_vec();
    expected.sort();
    assert_eq!(invoked, expected);
    for name in COMMANDS {
        assert_eq!(invoked_keys(name), command_keys(name), "{name}");
    }
}

/// A cancel for a run that registered no session — finished already, or on SQLite or ClickHouse,
/// which have none — is answered without reaching for the connection at all.
#[test]
fn a_run_with_no_session_has_nothing_to_cancel() {
    let state = DbState::default();
    assert_eq!(session_of(&state, "run-1"), None);
    state
        .running_queries
        .lock()
        .unwrap()
        .insert("run-1".into(), 42);
    assert_eq!(session_of(&state, "run-1"), Some(42));
}
