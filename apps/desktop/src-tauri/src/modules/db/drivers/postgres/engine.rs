//! PostgreSQL as a [`SqlEngine`]: what each `sql_*` command asks of this engine, answered by
//! the functions beside this file.
//!
//! `database` means something different here than on MySQL: there it names a database to reach
//! into from the one connection, here it picks which pool a method runs on. An empty one is the
//! database the connection was opened on.

use crate::error::AppError;
use crate::modules::db::drivers::postgres;
use crate::modules::db::drivers::sql::engine::{retry_read, SqlEngine};
use crate::modules::db::models::{ServerInfo, SqlProblem, StatementResult};
use async_trait::async_trait;
use serde_json::{Map, Value};
use std::sync::Arc;

/// One PostgreSQL connection: a pool per database, opened as each is asked for.
pub struct PostgresEngine(pub Arc<super::Pools>);

#[async_trait]
impl SqlEngine for PostgresEngine {
    async fn list_databases(&self) -> Result<Vec<String>, AppError> {
        retry_read!({
            let pool = self.0.pool(Some("")).await?;
            postgres::list_databases(&pool).await
        })
    }

    async fn server_info(&self) -> Result<ServerInfo, AppError> {
        retry_read!({
            let pool = self.0.pool(Some("")).await?;
            postgres::server_info(&pool).await
        })
    }

    async fn list_tables(&self, database: String) -> Result<Vec<String>, AppError> {
        retry_read!({
            let pool = self.0.pool(Some(&database)).await?;
            postgres::list_tables(&pool).await
        })
    }

    async fn table_stats(
        &self,
        database: String,
    ) -> Result<Vec<postgres::structure::TableStats>, AppError> {
        retry_read!({
            let pool = self.0.pool(Some(&database)).await?;
            postgres::structure::table_stats(&pool).await
        })
    }

    async fn table_data(
        &self,
        database: String,
        table: String,
        query: postgres::PageQuery,
    ) -> Result<postgres::TablePage, AppError> {
        retry_read!({
            let pool = self.0.pool(Some(&database)).await?;
            postgres::table_data(&pool, &table, &query).await
        })
    }

    async fn update_row(
        &self,
        database: String,
        table: String,
        updates: Map<String, Value>,
        key: Map<String, Value>,
    ) -> Result<(), AppError> {
        let pool = self.0.pool(Some(&database)).await?;
        postgres::update_row(&pool, &table, &updates, &key).await
    }

    async fn insert_rows(
        &self,
        database: String,
        table: String,
        rows: Vec<Map<String, Value>>,
    ) -> Result<(), AppError> {
        let pool = self.0.pool(Some(&database)).await?;
        postgres::insert_rows(&pool, &table, &rows).await
    }

    /// `reset_auto_increment` keeps MySQL's name because it is the frontend's name for the same
    /// checkbox; what it resets here is the table's identity or `serial` sequence.
    async fn delete_rows(
        &self,
        database: String,
        table: String,
        keys: Vec<Map<String, Value>>,
        all: bool,
        reset_auto_increment: bool,
    ) -> Result<(), AppError> {
        let pool = self.0.pool(Some(&database)).await?;
        postgres::delete_rows(&pool, &table, &keys, all, reset_auto_increment).await
    }

    async fn table_structure(
        &self,
        database: String,
        table: String,
    ) -> Result<postgres::structure::TableStructure, AppError> {
        retry_read!({
            let pool = self.0.pool(Some(&database)).await?;
            postgres::structure::table_structure(&pool, &table).await
        })
    }

    async fn collations(&self) -> Result<Vec<postgres::structure::Collation>, AppError> {
        retry_read!({
            let pool = self.0.pool(Some("")).await?;
            postgres::structure::collations(&pool).await
        })
    }

    async fn schema_outline(
        &self,
        database: String,
    ) -> Result<postgres::structure::SchemaOutline, AppError> {
        retry_read!({
            let pool = self.0.pool(Some(&database)).await?;
            postgres::structure::schema_outline(&pool, &database).await
        })
    }

    /// Creates a database, for the header's database picker. `collation` is accepted and ignored: see
    /// `postgres::ddl::create_database`.
    async fn create_database(
        &self,
        name: String,
        _collation: Option<String>,
    ) -> Result<(), AppError> {
        let pool = self.0.pool(Some("")).await?;
        postgres::ddl::create_database(&pool, &name).await
    }

    /// Drops a database and every table in it. Takes the whole connection rather than a pool, since the
    /// pool on the database being dropped is what has to be closed first.
    async fn drop_database(&self, database: String) -> Result<(), AppError> {
        let pools = self.0.clone();
        postgres::ddl::drop_database(&pools, &database).await
    }

    async fn rename_table(
        &self,
        database: String,
        table: String,
        new_name: String,
    ) -> Result<(), AppError> {
        let pool = self.0.pool(Some(&database)).await?;
        postgres::ddl::rename_table(&pool, &table, &new_name).await
    }

    async fn drop_table(&self, database: String, table: String) -> Result<(), AppError> {
        let pool = self.0.pool(Some(&database)).await?;
        postgres::ddl::drop_table(&pool, &table).await
    }

    async fn add_column(
        &self,
        database: String,
        table: String,
        spec: crate::modules::db::drivers::sql::types::ColumnSpec,
    ) -> Result<(), AppError> {
        let pool = self.0.pool(Some(&database)).await?;
        postgres::ddl::add_column(&pool, &table, &spec.into()).await
    }

    async fn modify_column(
        &self,
        database: String,
        table: String,
        name: String,
        spec: crate::modules::db::drivers::sql::types::ColumnSpec,
    ) -> Result<(), AppError> {
        let pool = self.0.pool(Some(&database)).await?;
        postgres::ddl::modify_column(&pool, &table, &name, &spec.into()).await
    }

    async fn drop_column(
        &self,
        database: String,
        table: String,
        name: String,
    ) -> Result<(), AppError> {
        let pool = self.0.pool(Some(&database)).await?;
        postgres::ddl::drop_column(&pool, &table, &name).await
    }

    async fn add_index(
        &self,
        database: String,
        table: String,
        spec: crate::modules::db::drivers::sql::types::IndexSpec,
    ) -> Result<(), AppError> {
        let pool = self.0.pool(Some(&database)).await?;
        postgres::ddl::add_index(&pool, &table, &spec.into()).await
    }

    async fn modify_index(
        &self,
        database: String,
        table: String,
        name: String,
        spec: crate::modules::db::drivers::sql::types::IndexSpec,
    ) -> Result<(), AppError> {
        let pool = self.0.pool(Some(&database)).await?;
        postgres::ddl::modify_index(&pool, &table, &name, &spec.into()).await
    }

    async fn drop_index(
        &self,
        database: String,
        table: String,
        name: String,
    ) -> Result<(), AppError> {
        let pool = self.0.pool(Some(&database)).await?;
        postgres::ddl::drop_index(&pool, &table, &name).await
    }

    /// Asks PostgreSQL to parse one statement without running it, for the editor's error checking.
    async fn validate_sql(
        &self,
        sql: String,
        database: Option<String>,
    ) -> Result<Option<SqlProblem>, AppError> {
        let pool = self.0.pool(Some(database.as_deref().unwrap_or(""))).await?;
        postgres::script::validate(&pool, &sql).await
    }

    async fn create_table(
        &self,
        database: String,
        table: String,
        _collation: Option<String>,
        _engine: Option<String>,
    ) -> Result<(), AppError> {
        let pool = self.0.pool(Some(&database)).await?;
        postgres::ddl::create_table(&pool, &table).await
    }

    async fn run_script(
        &self,
        sql: String,
        database: Option<String>,
        announce: &(dyn Fn(u64) + Send + Sync),
    ) -> Result<Vec<StatementResult>, AppError> {
        let pool = self.0.pool(Some(database.as_deref().unwrap_or(""))).await?;
        postgres::script::run(&pool, &sql, announce).await
    }

    /// The cancel goes out on a connection of its own, since the one being cancelled is busy. A
    /// backend pid belongs to the server, so the default database's pool reaches it.
    async fn cancel_query(&self, session: u64) -> Result<(), AppError> {
        let pool = self.0.pool(Some("")).await?;
        postgres::script::cancel(&pool, session).await
    }
}
