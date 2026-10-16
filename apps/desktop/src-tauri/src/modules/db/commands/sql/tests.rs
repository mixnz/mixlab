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
