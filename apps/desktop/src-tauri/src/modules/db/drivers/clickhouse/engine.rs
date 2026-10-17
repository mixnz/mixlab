//! ClickHouse as a [`SqlEngine`]: what each `sql_*` command asks of this engine, answered by
//! the functions beside this file.

use crate::error::AppError;
use crate::modules::db::drivers::clickhouse;
use crate::modules::db::drivers::sql::engine::SqlEngine;
use crate::modules::db::models::{ServerInfo, SqlProblem, StatementResult};
use async_trait::async_trait;
use serde_json::{Map, Value};

/// One ClickHouse connection: a base URL and credentials, every call its own request.
pub struct ClickhouseEngine(pub super::Connection);

#[async_trait]
impl SqlEngine for ClickhouseEngine {
    async fn server_info(&self) -> Result<ServerInfo, AppError> {
        let conn = self.0.clone();
        clickhouse::server_info(&conn).await
    }

    async fn list_databases(&self) -> Result<Vec<String>, AppError> {
        let conn = self.0.clone();
        clickhouse::list_databases(&conn).await
    }

    async fn list_tables(&self, database: String) -> Result<Vec<String>, AppError> {
        let conn = self.0.clone();
        clickhouse::list_tables(&conn, &database).await
    }

    async fn table_data(
        &self,
        database: String,
        table: String,
        query: clickhouse::PageQuery,
    ) -> Result<clickhouse::TablePage, AppError> {
        let conn = self.0.clone();
        clickhouse::table_data(&conn, &database, &table, &query).await
    }

    async fn table_structure(
        &self,
        database: String,
        table: String,
    ) -> Result<clickhouse::structure::TableStructure, AppError> {
        let conn = self.0.clone();
        clickhouse::structure::table_structure(&conn, &database, &table).await
    }

    async fn table_stats(
        &self,
        database: String,
    ) -> Result<Vec<clickhouse::structure::TableStats>, AppError> {
        let conn = self.0.clone();
        clickhouse::structure::table_stats(&conn, &database).await
    }

    async fn schema_outline(
        &self,
        database: String,
    ) -> Result<clickhouse::structure::SchemaOutline, AppError> {
        let conn = self.0.clone();
        clickhouse::structure::schema_outline(&conn, &database).await
    }

    async fn validate_sql(
        &self,
        sql: String,
        database: Option<String>,
    ) -> Result<Option<SqlProblem>, AppError> {
        let conn = self.0.clone();
        clickhouse::script::validate(&conn, &sql, database.as_deref()).await
    }

    async fn update_row(
        &self,
        database: String,
        table: String,
        updates: Map<String, Value>,
        key: Map<String, Value>,
    ) -> Result<(), AppError> {
        let conn = self.0.clone();
        clickhouse::update_row(&conn, &database, &table, &updates, &key).await
    }

    async fn delete_rows(
        &self,
        database: String,
        table: String,
        keys: Vec<Map<String, Value>>,
        all: bool,
        reset_auto_increment: bool,
    ) -> Result<(), AppError> {
        let conn = self.0.clone();
        clickhouse::delete_rows(&conn, &database, &table, &keys, all, reset_auto_increment).await
    }

    async fn insert_rows(
        &self,
        database: String,
        table: String,
        rows: Vec<Map<String, Value>>,
    ) -> Result<(), AppError> {
        let conn = self.0.clone();
        clickhouse::insert_rows(&conn, &database, &table, &rows).await
    }

    async fn drop_database(&self, database: String) -> Result<(), AppError> {
        let conn = self.0.clone();
        clickhouse::ddl::drop_database(&conn, &database).await
    }

    async fn rename_table(
        &self,
        database: String,
        table: String,
        new_name: String,
    ) -> Result<(), AppError> {
        let conn = self.0.clone();
        clickhouse::ddl::rename_table(&conn, &database, &table, &new_name).await
    }

    async fn drop_table(&self, database: String, table: String) -> Result<(), AppError> {
        let conn = self.0.clone();
        clickhouse::ddl::drop_table(&conn, &database, &table).await
    }

    async fn add_column(
        &self,
        database: String,
        table: String,
        spec: crate::modules::db::drivers::sql::types::ColumnSpec,
    ) -> Result<(), AppError> {
        let conn = self.0.clone();
        clickhouse::ddl::add_column(&conn, &database, &table, &spec.into()).await
    }

    async fn drop_column(
        &self,
        database: String,
        table: String,
        name: String,
    ) -> Result<(), AppError> {
        let conn = self.0.clone();
        clickhouse::ddl::drop_column(&conn, &database, &table, &name).await
    }

    async fn modify_column(
        &self,
        database: String,
        table: String,
        name: String,
        spec: crate::modules::db::drivers::sql::types::ColumnSpec,
    ) -> Result<(), AppError> {
        let conn = self.0.clone();
        clickhouse::ddl::modify_column(&conn, &database, &table, &name, &spec.into()).await
    }

    async fn add_skip_index(
        &self,
        database: String,
        table: String,
        spec: clickhouse::ddl::SkipIndexSpec,
    ) -> Result<(), AppError> {
        let conn = self.0.clone();
        clickhouse::ddl::add_skip_index(&conn, &database, &table, &spec).await
    }

    async fn modify_skip_index(
        &self,
        database: String,
        table: String,
        name: String,
        spec: clickhouse::ddl::SkipIndexSpec,
    ) -> Result<(), AppError> {
        let conn = self.0.clone();
        clickhouse::ddl::modify_skip_index(&conn, &database, &table, &name, &spec).await
    }

    async fn drop_skip_index(
        &self,
        database: String,
        table: String,
        name: String,
    ) -> Result<(), AppError> {
        let conn = self.0.clone();
        clickhouse::ddl::drop_skip_index(&conn, &database, &table, &name).await
    }

    async fn rebuild_order_by(
        &self,
        database: String,
        table: String,
        columns: Vec<String>,
    ) -> Result<Option<String>, AppError> {
        let conn = self.0.clone();
        clickhouse::ddl::rebuild_order_by(&conn, &database, &table, &columns).await
    }

    async fn row_count(&self, database: String, table: String) -> Result<u64, AppError> {
        let conn = self.0.clone();
        clickhouse::ddl::row_count(&conn, &database, &table).await
    }

    /// Creates an empty table, one `id UInt64` column and an empty sorting key. `engine` is the
    /// only thing the dialog asks for beyond the name, and the only one of a table's properties
    /// ClickHouse cannot change afterwards, so a call without one is refused rather than defaulted.
    async fn create_table(
        &self,
        database: String,
        table: String,
        _collation: Option<String>,
        engine: Option<String>,
    ) -> Result<(), AppError> {
        let engine = engine.ok_or_else(|| err!("error.tableEngineRequired"))?;
        let conn = self.0.clone();
        clickhouse::ddl::create_table(&conn, &database, &table, &engine).await
    }

    /// No cancel exists for a ClickHouse run in v1, so `announce` is never called.
    async fn run_script(
        &self,
        sql: String,
        database: Option<String>,
        _announce: &(dyn Fn(u64) + Send + Sync),
    ) -> Result<Vec<StatementResult>, AppError> {
        let conn = self.0.clone();
        clickhouse::script::run(&conn, &sql, database.as_deref()).await
    }

    /// Creates a database. `collation` is taken and dropped: ClickHouse has none.
    async fn create_database(
        &self,
        name: String,
        _collation: Option<String>,
    ) -> Result<(), AppError> {
        let conn = self.0.clone();
        clickhouse::ddl::create_database(&conn, &name).await
    }
}
