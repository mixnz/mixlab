//! The one trait every SQL engine answers through (ADR 0062). A `sql_*` command turns its
//! connection into a `Box<dyn SqlEngine>` with `DbHandle::sql` and asks it; no command matches on
//! the engine.
//!
//! The arguments are owned, exactly as the commands receive them, so an engine's method is the
//! body its command used to have.

use super::types::{
    Collation, ColumnSpec, IndexSpec, PageQuery, SchemaOutline, TablePage, TableStats,
    TableStructure,
};
use crate::error::AppError;
use crate::modules::db::drivers::clickhouse::ddl::SkipIndexSpec;
use crate::modules::db::models::{ServerInfo, SqlProblem, StatementResult};
use async_trait::async_trait;
use serde_json::{Map, Value};

/// Runs a **read** command again exactly once, if the first attempt died along with the connection.
///
/// Reads only. An `INSERT` run again after losing the connection may become two rows — the
/// statement may have reached the server and only the answer was lost — so write commands report
/// the error and leave the decision to the user.
///
/// A macro rather than a function taking a closure: an `Fn() -> impl Future` borrowing
/// `State<'_, DbState>` drags the call into exactly the kind of lifetime trouble not worth
/// fighting, while a macro just writes the command body twice.
///
/// Called as `retry_read!({ ... })` — parentheses around the block, because a macro call opening
/// with a brace in statement position is parsed as a *statement macro*, not an expression with a
/// value.
macro_rules! retry_read {
    ($body:block) => {{
        match async { $body }.await {
            // No sleep between the two attempts: the second one will wait by itself in the pool's
            // `acquire`, behind the session reopening that is under way.
            Err(e) if e.code == "error.connectionLost" => async { $body }.await,
            first => first,
        }
    }};
}

pub(crate) use retry_read;

/// One row as the window sends it: column name to value.
pub type Row = Map<String, Value>;

/// The error for an operation this engine does not have. The dialect hides every control that
/// would ask for one, so reaching this is a bug in the caller, but a typed one the window can show.
pub fn unsupported(operation: &str) -> AppError {
    err!("error.unsupportedOperation", operation = operation)
}

/// What the `sql_*` commands ask of a SQL engine. The first eighteen methods every engine has; the
/// rest default to [`unsupported`] and are written by the engines that have them.
#[async_trait]
pub trait SqlEngine: Send + Sync {
    async fn list_databases(&self) -> Result<Vec<String>, AppError>;
    async fn server_info(&self) -> Result<ServerInfo, AppError>;
    async fn list_tables(&self, database: String) -> Result<Vec<String>, AppError>;
    async fn table_stats(&self, database: String) -> Result<Vec<TableStats>, AppError>;
    async fn table_data(
        &self,
        database: String,
        table: String,
        query: PageQuery,
    ) -> Result<TablePage, AppError>;
    async fn table_structure(
        &self,
        database: String,
        table: String,
    ) -> Result<TableStructure, AppError>;
    async fn schema_outline(&self, database: String) -> Result<SchemaOutline, AppError>;
    async fn update_row(
        &self,
        database: String,
        table: String,
        updates: Row,
        key: Row,
    ) -> Result<(), AppError>;
    async fn insert_rows(
        &self,
        database: String,
        table: String,
        rows: Vec<Row>,
    ) -> Result<(), AppError>;
    async fn delete_rows(
        &self,
        database: String,
        table: String,
        keys: Vec<Row>,
        all: bool,
        reset_auto_increment: bool,
    ) -> Result<(), AppError>;
    /// `collation` is read by the engines that have one and `engine` by ClickHouse alone.
    async fn create_table(
        &self,
        database: String,
        table: String,
        collation: Option<String>,
        engine: Option<String>,
    ) -> Result<(), AppError>;
    async fn rename_table(
        &self,
        database: String,
        table: String,
        new_name: String,
    ) -> Result<(), AppError>;
    async fn drop_table(&self, database: String, table: String) -> Result<(), AppError>;
    async fn add_column(
        &self,
        database: String,
        table: String,
        spec: ColumnSpec,
    ) -> Result<(), AppError>;
    async fn modify_column(
        &self,
        database: String,
        table: String,
        name: String,
        spec: ColumnSpec,
    ) -> Result<(), AppError>;
    async fn drop_column(
        &self,
        database: String,
        table: String,
        name: String,
    ) -> Result<(), AppError>;
    /// `announce` is handed the server's id for the session the script runs on, once it has one,
    /// so that `cancel_query` can reach it. An engine with nothing to cancel never calls it.
    async fn run_script(
        &self,
        sql: String,
        database: Option<String>,
        announce: &(dyn Fn(u64) + Send + Sync),
    ) -> Result<Vec<StatementResult>, AppError>;
    async fn validate_sql(
        &self,
        sql: String,
        database: Option<String>,
    ) -> Result<Option<SqlProblem>, AppError>;

    async fn create_database(
        &self,
        _name: String,
        _collation: Option<String>,
    ) -> Result<(), AppError> {
        Err(unsupported("create_database"))
    }
    async fn drop_database(&self, _database: String) -> Result<(), AppError> {
        Err(unsupported("drop_database"))
    }
    async fn collations(&self) -> Result<Vec<Collation>, AppError> {
        Err(unsupported("collations"))
    }
    async fn add_index(
        &self,
        _database: String,
        _table: String,
        _spec: IndexSpec,
    ) -> Result<(), AppError> {
        Err(unsupported("add_index"))
    }
    async fn modify_index(
        &self,
        _database: String,
        _table: String,
        _name: String,
        _spec: IndexSpec,
    ) -> Result<(), AppError> {
        Err(unsupported("modify_index"))
    }
    async fn drop_index(
        &self,
        _database: String,
        _table: String,
        _name: String,
    ) -> Result<(), AppError> {
        Err(unsupported("drop_index"))
    }
    /// Stops the session `run_script` announced.
    async fn cancel_query(&self, _session: u64) -> Result<(), AppError> {
        Err(unsupported("cancel_query"))
    }
    async fn add_skip_index(
        &self,
        _database: String,
        _table: String,
        _spec: SkipIndexSpec,
    ) -> Result<(), AppError> {
        Err(unsupported("add_skip_index"))
    }
    async fn modify_skip_index(
        &self,
        _database: String,
        _table: String,
        _name: String,
        _spec: SkipIndexSpec,
    ) -> Result<(), AppError> {
        Err(unsupported("modify_skip_index"))
    }
    async fn drop_skip_index(
        &self,
        _database: String,
        _table: String,
        _name: String,
    ) -> Result<(), AppError> {
        Err(unsupported("drop_skip_index"))
    }
    async fn rebuild_order_by(
        &self,
        _database: String,
        _table: String,
        _columns: Vec<String>,
    ) -> Result<Option<String>, AppError> {
        Err(unsupported("rebuild_order_by"))
    }
    async fn row_count(&self, _database: String, _table: String) -> Result<u64, AppError> {
        Err(unsupported("row_count"))
    }
}
