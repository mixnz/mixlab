//! MySQL and MariaDB as a [`SqlEngine`]: what each `sql_*` command asks of this engine, answered by
//! the functions beside this file.

use crate::error::AppError;
use crate::modules::db::drivers::mysql;
use crate::modules::db::drivers::sql::engine::{retry_read, SqlEngine};
use crate::modules::db::models::{ServerInfo, SqlProblem, StatementResult};
use async_trait::async_trait;
use serde_json::{Map, Value};

/// One MySQL or MariaDB connection, and which of the two it reached.
pub struct MysqlEngine {
    pub pool: sqlx::MySqlPool,
    pub mariadb: bool,
}

#[async_trait]
impl SqlEngine for MysqlEngine {
    async fn list_databases(&self) -> Result<Vec<String>, AppError> {
        retry_read!({
            let pool = self.pool.clone();
            mysql::list_databases(&pool).await
        })
    }

    async fn server_info(&self) -> Result<ServerInfo, AppError> {
        retry_read!({
            let pool = self.pool.clone();
            mysql::server_info(&pool).await
        })
    }

    async fn list_tables(&self, database: String) -> Result<Vec<String>, AppError> {
        retry_read!({
            let pool = self.pool.clone();
            mysql::list_tables(&pool, &database).await
        })
    }

    /// What every table in the database weighs, for the workspace's Statistics tab.
    async fn table_stats(
        &self,
        database: String,
    ) -> Result<Vec<mysql::structure::TableStats>, AppError> {
        retry_read!({
            let pool = self.pool.clone();
            mysql::structure::table_stats(&pool, &database).await
        })
    }

    async fn table_data(
        &self,
        database: String,
        table: String,
        query: mysql::PageQuery,
    ) -> Result<mysql::TablePage, AppError> {
        retry_read!({
            let (pool, mariadb) = (self.pool.clone(), self.mariadb);
            mysql::table_data(&pool, mariadb, &database, &table, &query).await
        })
    }

    async fn update_row(
        &self,
        database: String,
        table: String,
        updates: Map<String, Value>,
        key: Map<String, Value>,
    ) -> Result<(), AppError> {
        let pool = self.pool.clone();
        mysql::update_row(&pool, &database, &table, &updates, &key).await
    }

    async fn insert_rows(
        &self,
        database: String,
        table: String,
        rows: Vec<Map<String, Value>>,
    ) -> Result<(), AppError> {
        let pool = self.pool.clone();
        mysql::insert_rows(&pool, &database, &table, &rows).await
    }

    async fn delete_rows(
        &self,
        database: String,
        table: String,
        keys: Vec<Map<String, Value>>,
        all: bool,
        reset_auto_increment: bool,
    ) -> Result<(), AppError> {
        let pool = self.pool.clone();
        mysql::delete_rows(&pool, &database, &table, &keys, all, reset_auto_increment).await
    }

    async fn table_structure(
        &self,
        database: String,
        table: String,
    ) -> Result<mysql::structure::TableStructure, AppError> {
        retry_read!({
            let (pool, mariadb) = (self.pool.clone(), self.mariadb);
            mysql::structure::table_structure(&pool, mariadb, &database, &table).await
        })
    }

    /// Every table and column of one database, for the Query tab's completion. One read covers the
    /// whole database, so the editor never asks per table as the Structure tab does.
    async fn schema_outline(
        &self,
        database: String,
    ) -> Result<mysql::structure::SchemaOutline, AppError> {
        retry_read!({
            let pool = self.pool.clone();
            mysql::structure::schema_outline(&pool, &database).await
        })
    }

    /// The collations this server has, for the column editor's picker. A property of the server rather
    /// than of any one table, so the frontend reads it once per connection.
    async fn collations(&self) -> Result<Vec<mysql::structure::Collation>, AppError> {
        retry_read!({
            let pool = self.pool.clone();
            mysql::structure::collations(&pool).await
        })
    }

    /// Drops a database and every table in it.
    async fn drop_database(&self, database: String) -> Result<(), AppError> {
        let pool = self.pool.clone();
        mysql::structure::drop_database(&pool, &database).await
    }

    /// Creates a database, for the header's database picker.
    async fn create_database(
        &self,
        name: String,
        collation: Option<String>,
    ) -> Result<(), AppError> {
        let pool = self.pool.clone();
        mysql::structure::create_database(&pool, &name, collation.as_deref()).await
    }

    /// Renames a table, for the sidebar's context menu.
    async fn rename_table(
        &self,
        database: String,
        table: String,
        new_name: String,
    ) -> Result<(), AppError> {
        let pool = self.pool.clone();
        mysql::structure::rename_table(&pool, &database, &table, &new_name).await
    }

    /// Drops a table and everything in it, for the sidebar's context menu.
    async fn drop_table(&self, database: String, table: String) -> Result<(), AppError> {
        let pool = self.pool.clone();
        mysql::structure::drop_table(&pool, &database, &table).await
    }

    async fn add_column(
        &self,
        database: String,
        table: String,
        spec: mysql::structure::ColumnSpec,
    ) -> Result<(), AppError> {
        let pool = self.pool.clone();
        mysql::structure::add_column(&pool, &database, &table, &spec).await
    }

    async fn modify_column(
        &self,
        database: String,
        table: String,
        name: String,
        spec: mysql::structure::ColumnSpec,
    ) -> Result<(), AppError> {
        let pool = self.pool.clone();
        mysql::structure::modify_column(&pool, &database, &table, &name, &spec).await
    }

    async fn drop_column(
        &self,
        database: String,
        table: String,
        name: String,
    ) -> Result<(), AppError> {
        let pool = self.pool.clone();
        mysql::structure::drop_column(&pool, &database, &table, &name).await
    }

    async fn add_index(
        &self,
        database: String,
        table: String,
        spec: mysql::structure::IndexSpec,
    ) -> Result<(), AppError> {
        let pool = self.pool.clone();
        mysql::structure::add_index(&pool, &database, &table, &spec).await
    }

    async fn modify_index(
        &self,
        database: String,
        table: String,
        name: String,
        spec: mysql::structure::IndexSpec,
    ) -> Result<(), AppError> {
        let pool = self.pool.clone();
        mysql::structure::modify_index(&pool, &database, &table, &name, &spec).await
    }

    async fn drop_index(
        &self,
        database: String,
        table: String,
        name: String,
    ) -> Result<(), AppError> {
        let pool = self.pool.clone();
        mysql::structure::drop_index(&pool, &database, &table, &name).await
    }

    /// Asks MySQL to parse one statement without running it, for the editor's error checking.
    ///
    /// Read-only in the strongest sense available: the statement is prepared and the plan thrown away,
    /// so not even a `DELETE` handed to this does anything. What comes back is `null` when the server
    /// had nothing to say — see `mysql::script::validate` for why most of what it *does* say is a
    /// warning rather than an error.
    async fn validate_sql(
        &self,
        sql: String,
        database: Option<String>,
    ) -> Result<Option<SqlProblem>, AppError> {
        let pool = self.pool.clone();
        mysql::script::validate(&pool, &sql, database.as_deref()).await
    }

    async fn create_table(
        &self,
        database: String,
        table: String,
        collation: Option<String>,
        _engine: Option<String>,
    ) -> Result<(), AppError> {
        let pool = self.pool.clone();
        mysql::structure::create_table(&pool, &database, &table, collation.as_deref()).await
    }

    async fn run_script(
        &self,
        sql: String,
        database: Option<String>,
        announce: &(dyn Fn(u64) + Send + Sync),
    ) -> Result<Vec<StatementResult>, AppError> {
        let pool = self.pool.clone();
        mysql::script::run(&pool, &sql, database.as_deref(), announce).await
    }

    async fn cancel_query(&self, session: u64) -> Result<(), AppError> {
        mysql::kill_query(&self.pool, session).await
    }
}
