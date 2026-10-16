//! SQLite as a [`SqlEngine`]: what each `sql_*` command asks of this engine, answered by
//! the functions beside this file.

use crate::error::AppError;
use crate::modules::db::drivers::sql::engine::SqlEngine;
use crate::modules::db::drivers::sqlite;
use crate::modules::db::models::{ServerInfo, SqlProblem, StatementResult};
use async_trait::async_trait;
use serde_json::{Map, Value};

/// One SQLite file.
pub struct SqliteEngine(pub sqlx::SqlitePool);

#[async_trait]
impl SqlEngine for SqliteEngine {
    async fn server_info(&self) -> Result<ServerInfo, AppError> {
        let pool = self.0.clone();
        sqlite::server_info(&pool).await
    }

    /// The one database a file holds. Answered without touching it — see `sqlite::list_databases`.
    /// A connection that has been closed still reports itself here, as for any other read: the
    /// command looked it up before asking.
    async fn list_databases(&self) -> Result<Vec<String>, AppError> {
        Ok(sqlite::list_databases())
    }

    async fn list_tables(&self, _database: String) -> Result<Vec<String>, AppError> {
        let pool = self.0.clone();
        sqlite::list_tables(&pool).await
    }

    async fn table_data(
        &self,
        _database: String,
        table: String,
        query: sqlite::PageQuery,
    ) -> Result<sqlite::TablePage, AppError> {
        let pool = self.0.clone();
        sqlite::table_data(&pool, &table, &query).await
    }

    async fn table_structure(
        &self,
        _database: String,
        table: String,
    ) -> Result<sqlite::structure::TableStructure, AppError> {
        let pool = self.0.clone();
        sqlite::structure::table_structure(&pool, &table).await
    }

    async fn schema_outline(
        &self,
        database: String,
    ) -> Result<sqlite::structure::SchemaOutline, AppError> {
        let pool = self.0.clone();
        sqlite::structure::schema_outline(&pool, &database).await
    }

    async fn table_stats(
        &self,
        _database: String,
    ) -> Result<Vec<sqlite::structure::TableStats>, AppError> {
        let pool = self.0.clone();
        sqlite::structure::table_stats(&pool).await
    }

    async fn collations(&self) -> Result<Vec<sqlite::structure::Collation>, AppError> {
        let pool = self.0.clone();
        sqlite::structure::collations(&pool).await
    }

    async fn update_row(
        &self,
        _database: String,
        table: String,
        updates: Map<String, Value>,
        key: Map<String, Value>,
    ) -> Result<(), AppError> {
        let pool = self.0.clone();
        sqlite::update_row(&pool, &table, &updates, &key).await
    }

    async fn insert_rows(
        &self,
        _database: String,
        table: String,
        rows: Vec<Map<String, Value>>,
    ) -> Result<(), AppError> {
        let pool = self.0.clone();
        sqlite::insert_rows(&pool, &table, &rows).await
    }

    async fn delete_rows(
        &self,
        _database: String,
        table: String,
        keys: Vec<Map<String, Value>>,
        all: bool,
        reset_auto_increment: bool,
    ) -> Result<(), AppError> {
        let pool = self.0.clone();
        sqlite::delete_rows(&pool, &table, &keys, all, reset_auto_increment).await
    }

    async fn rename_table(
        &self,
        _database: String,
        table: String,
        new_name: String,
    ) -> Result<(), AppError> {
        let pool = self.0.clone();
        sqlite::ddl::rename_table(&pool, &table, &new_name).await
    }

    async fn drop_table(&self, _database: String, table: String) -> Result<(), AppError> {
        let pool = self.0.clone();
        sqlite::ddl::drop_table(&pool, &table).await
    }

    async fn add_column(
        &self,
        _database: String,
        table: String,
        spec: crate::modules::db::drivers::sql::types::ColumnSpec,
    ) -> Result<(), AppError> {
        let pool = self.0.clone();
        sqlite::ddl::add_column(&pool, &table, &spec.into()).await
    }

    async fn modify_column(
        &self,
        _database: String,
        table: String,
        name: String,
        spec: crate::modules::db::drivers::sql::types::ColumnSpec,
    ) -> Result<(), AppError> {
        let pool = self.0.clone();
        sqlite::ddl::modify_column(&pool, &table, &name, &spec.into()).await
    }

    async fn drop_column(
        &self,
        _database: String,
        table: String,
        name: String,
    ) -> Result<(), AppError> {
        let pool = self.0.clone();
        sqlite::ddl::drop_column(&pool, &table, &name).await
    }

    async fn add_index(
        &self,
        _database: String,
        table: String,
        spec: crate::modules::db::drivers::sql::types::IndexSpec,
    ) -> Result<(), AppError> {
        let pool = self.0.clone();
        sqlite::ddl::add_index(&pool, &table, &spec.into()).await
    }

    async fn modify_index(
        &self,
        _database: String,
        table: String,
        name: String,
        spec: crate::modules::db::drivers::sql::types::IndexSpec,
    ) -> Result<(), AppError> {
        let pool = self.0.clone();
        sqlite::ddl::modify_index(&pool, &table, &name, &spec.into()).await
    }

    async fn drop_index(
        &self,
        _database: String,
        table: String,
        name: String,
    ) -> Result<(), AppError> {
        let pool = self.0.clone();
        sqlite::ddl::drop_index(&pool, &table, &name).await
    }

    async fn validate_sql(
        &self,
        sql: String,
        _database: Option<String>,
    ) -> Result<Option<SqlProblem>, AppError> {
        let pool = self.0.clone();
        sqlite::script::validate(&pool, &sql).await
    }

    async fn create_table(
        &self,
        _database: String,
        table: String,
        _collation: Option<String>,
        _engine: Option<String>,
    ) -> Result<(), AppError> {
        let pool = self.0.clone();
        sqlite::ddl::create_table(&pool, &table).await
    }

    /// Runs in this process against the file, so there is no session to announce or cancel.
    async fn run_script(
        &self,
        sql: String,
        _database: Option<String>,
        _announce: &(dyn Fn(u64) + Send + Sync),
    ) -> Result<Vec<StatementResult>, AppError> {
        let pool = self.0.clone();
        sqlite::script::run(&pool, &sql).await
    }
}
