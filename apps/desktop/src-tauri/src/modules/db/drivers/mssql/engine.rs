//! SQL Server as a [`SqlEngine`]: what each `sql_*` command asks of this engine, answered by
//! the functions beside this file.

use crate::error::AppError;
use crate::modules::db::drivers::mssql;
use crate::modules::db::drivers::sql::engine::{retry_read, SqlEngine};
use crate::modules::db::models::{ServerInfo, SqlProblem, StatementResult};
use async_trait::async_trait;
use serde_json::{Map, Value};

/// One SQL Server connection.
pub struct MssqlEngine(pub super::Pool);

#[async_trait]
impl SqlEngine for MssqlEngine {
    async fn list_databases(&self) -> Result<Vec<String>, AppError> {
        retry_read!({
            let pool = self.0.clone();
            mssql::list_databases(&pool).await
        })
    }

    async fn server_info(&self) -> Result<ServerInfo, AppError> {
        retry_read!({
            let pool = self.0.clone();
            mssql::server_info(&pool).await
        })
    }

    async fn list_tables(&self, database: String) -> Result<Vec<String>, AppError> {
        retry_read!({
            let pool = self.0.clone();
            mssql::list_tables(&pool, &database).await
        })
    }

    /// Not `retry_read!`: these three write, and `retry_read!` only bothers to bound because a SELECT
    /// tried again after failing halfway is harmless — an INSERT/UPDATE/DELETE tried again the same way
    /// could write twice. PostgreSQL's and MySQL's `update_row` skip it for the same reason.
    async fn update_row(
        &self,
        database: String,
        table: String,
        updates: Map<String, Value>,
        key: Map<String, Value>,
    ) -> Result<(), AppError> {
        let pool = self.0.clone();
        mssql::editing::update_row(&pool, &database, &table, &updates, &key).await
    }

    async fn insert_rows(
        &self,
        database: String,
        table: String,
        rows: Vec<Map<String, Value>>,
    ) -> Result<(), AppError> {
        let pool = self.0.clone();
        mssql::editing::insert_rows(&pool, &database, &table, &rows).await
    }

    /// `reset_auto_increment` keeps MySQL's name, the way `commands::postgres` does too — it is the
    /// frontend's name for the checkbox; what it resets here is the table's IDENTITY seed.
    async fn delete_rows(
        &self,
        database: String,
        table: String,
        keys: Vec<Map<String, Value>>,
        all: bool,
        reset_auto_increment: bool,
    ) -> Result<(), AppError> {
        let pool = self.0.clone();
        mssql::editing::delete_rows(&pool, &database, &table, &keys, all, reset_auto_increment)
            .await
    }

    async fn table_data(
        &self,
        database: String,
        table: String,
        query: mssql::PageQuery,
    ) -> Result<mssql::TablePage, AppError> {
        retry_read!({
            let pool = self.0.clone();
            mssql::table_data(&pool, &database, &table, &query).await
        })
    }

    async fn table_structure(
        &self,
        database: String,
        table: String,
    ) -> Result<mssql::structure::TableStructure, AppError> {
        retry_read!({
            let pool = self.0.clone();
            mssql::structure::table_structure(&pool, &database, &table).await
        })
    }

    async fn table_stats(
        &self,
        database: String,
    ) -> Result<Vec<mssql::structure::TableStats>, AppError> {
        retry_read!({
            let pool = self.0.clone();
            mssql::structure::table_stats(&pool, &database).await
        })
    }

    async fn collations(&self) -> Result<Vec<mssql::structure::Collation>, AppError> {
        retry_read!({
            let pool = self.0.clone();
            mssql::structure::collations(&pool).await
        })
    }

    async fn schema_outline(
        &self,
        database: String,
    ) -> Result<mssql::structure::SchemaOutline, AppError> {
        retry_read!({
            let pool = self.0.clone();
            mssql::structure::schema_outline(&pool, &database).await
        })
    }

    /// Asks SQL Server to parse one statement without running it, for the editor's error checking.
    async fn validate_sql(
        &self,
        sql: String,
        database: Option<String>,
    ) -> Result<Option<SqlProblem>, AppError> {
        let pool = self.0.clone();
        mssql::script::validate(&pool, &sql, database.as_deref()).await
    }

    async fn create_database(
        &self,
        name: String,
        collation: Option<String>,
    ) -> Result<(), AppError> {
        let pool = self.0.clone();
        mssql::ddl::create_database(&pool, &name, collation.as_deref()).await
    }

    async fn drop_database(&self, database: String) -> Result<(), AppError> {
        let pool = self.0.clone();
        mssql::ddl::drop_database(&pool, &database).await
    }

    async fn rename_table(
        &self,
        database: String,
        table: String,
        new_name: String,
    ) -> Result<(), AppError> {
        let pool = self.0.clone();
        mssql::ddl::rename_table(&pool, &database, &table, &new_name).await
    }

    async fn drop_table(&self, database: String, table: String) -> Result<(), AppError> {
        let pool = self.0.clone();
        mssql::ddl::drop_table(&pool, &database, &table).await
    }

    async fn add_column(
        &self,
        database: String,
        table: String,
        spec: crate::modules::db::drivers::sql::types::ColumnSpec,
    ) -> Result<(), AppError> {
        let pool = self.0.clone();
        mssql::ddl::add_column(&pool, &database, &table, &spec.into()).await
    }

    async fn drop_column(
        &self,
        database: String,
        table: String,
        name: String,
    ) -> Result<(), AppError> {
        let pool = self.0.clone();
        mssql::ddl::drop_column(&pool, &database, &table, &name).await
    }

    async fn modify_column(
        &self,
        database: String,
        table: String,
        name: String,
        spec: crate::modules::db::drivers::sql::types::ColumnSpec,
    ) -> Result<(), AppError> {
        let pool = self.0.clone();
        mssql::ddl::modify_column(&pool, &database, &table, &name, &spec.into()).await
    }

    async fn add_index(
        &self,
        database: String,
        table: String,
        spec: crate::modules::db::drivers::sql::types::IndexSpec,
    ) -> Result<(), AppError> {
        let pool = self.0.clone();
        mssql::ddl::add_index(&pool, &database, &table, &spec.into()).await
    }

    async fn modify_index(
        &self,
        database: String,
        table: String,
        name: String,
        spec: crate::modules::db::drivers::sql::types::IndexSpec,
    ) -> Result<(), AppError> {
        let pool = self.0.clone();
        mssql::ddl::modify_index(&pool, &database, &table, &name, &spec.into()).await
    }

    async fn drop_index(
        &self,
        database: String,
        table: String,
        name: String,
    ) -> Result<(), AppError> {
        let pool = self.0.clone();
        mssql::ddl::drop_index(&pool, &database, &table, &name).await
    }

    async fn create_table(
        &self,
        database: String,
        table: String,
        _collation: Option<String>,
        _engine: Option<String>,
    ) -> Result<(), AppError> {
        let pool = self.0.clone();
        mssql::ddl::create_table(&pool, &database, &table).await
    }

    async fn run_script(
        &self,
        sql: String,
        database: Option<String>,
        announce: &(dyn Fn(u64) + Send + Sync),
    ) -> Result<Vec<StatementResult>, AppError> {
        let pool = self.0.clone();
        mssql::script::run(&pool, &sql, database.as_deref(), announce).await
    }

    async fn cancel_query(&self, session: u64) -> Result<(), AppError> {
        mssql::script::cancel(&self.0, session).await
    }
}
