//! Changing the shape of a SQL Server database: its databases, tables, columns and indexes.
//!
//! Closest to `postgres_ddl.rs` in shape — one dialog fills either `ColumnSpec` or `IndexSpec`, and
//! most statements run inside a transaction the way PostgreSQL's DDL does (unlike MySQL's, which
//! auto-commits each one). What differs is the grammar of nearly every statement, and one structural
//! difference reaches further than any single statement: `ALTER COLUMN` cannot carry a default, an
//! index, or the identity property, so [`modify_column`] reads what is in the column's way first and
//! clears it, rather than restating the column whole the way MySQL's `CHANGE COLUMN` or building one
//! `ALTER COLUMN TYPE ... USING ...` the way PostgreSQL's does — see D15 and this plan's Task 5.

use super::mssql::{
    end_transaction, map_error, quote_ident, resolve, three_part, Connection, Pool,
};
use super::mssql_structure::{IndexColumn, TableIndex};
use crate::error::AppError;
use serde::Deserialize;

/// What a column is to be declared as — the write-side counterpart of `mssql_structure::StructureColumn`.
///
/// `after`/`onUpdateCurrentTimestamp`, the two fields MySQL's dialog sends that mean nothing here,
/// are simply not declared: serde drops a field a struct has no place for, the way
/// `postgres_ddl::ColumnSpec` already relies on for the same two fields.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnSpec {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    /// `None` writes no DEFAULT at all — and, on an existing column, drops the one it has.
    #[serde(default)]
    pub default_value: Option<String>,
    #[serde(default)]
    pub default_is_expression: bool,
    /// An `IDENTITY` column: one the server numbers itself (D7). Cannot be turned on or off on an
    /// existing column — see [`modify_column`] and D15.
    #[serde(default)]
    pub auto_increment: bool,
    #[serde(default)]
    pub collation: Option<String>,
    #[serde(default)]
    pub comment: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexColumnSpec {
    pub name: String,
}

/// What an index is to be created as.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexSpec {
    /// Left empty on a plain or unique index, one is generated — see `generated_index_name`. A
    /// primary key needs no name at all: `ADD CONSTRAINT` may drop its `CONSTRAINT name` clause and
    /// let the server pick one, the same as PostgreSQL's unnamed primary key.
    #[serde(default)]
    pub name: String,
    /// `index`, `unique` or `primary`.
    pub kind: String,
    /// `CLUSTERED` or `NONCLUSTERED`, or `None` to let the server decide.
    #[serde(default)]
    pub index_type: Option<String>,
    pub columns: Vec<IndexColumnSpec>,
    // A `comment` field is deliberately not declared here: the frontend always sends one
    // (`SqlIndexSpec.comment`), but SQL Server keeps no comment on an index — see
    // `mssql_structure::TableIndex.comment` — so there is nothing to do with it, and serde drops an
    // undeclared field rather than erroring on it, the same way `postgres_ddl::ColumnSpec` relies on
    // for the fields it has no place for.
}

/// Wraps text as a SQL string literal, doubling the quote that would otherwise end it early. T-SQL
/// does not backslash-escape a plain literal, so a backslash is left alone — the same rule
/// `postgres_ddl::quote_string` follows and for the same reason.
pub(super) fn quote_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// A Unicode string literal — the extended-property stored procedures (`comment_statement`, Task 4)
/// take `sql_variant` parameters, and every value passed here is text.
fn quote_nstring(value: &str) -> String {
    format!("N'{}'", value.replace('\'', "''"))
}

/// Runs one statement on its own connection, outside any transaction. `CREATE DATABASE` and
/// `ALTER`/`DROP DATABASE` are the only statements this file sends that T-SQL refuses inside a
/// transaction at all (error 226) — every other statement goes through [`execute_all`] instead.
async fn execute_single(pool: &Pool, sql: String) -> Result<(), AppError> {
    let mut client = pool
        .get()
        .await
        .map_err(|e| err!("error.mssql", message = e))?;
    client.simple_query(sql).await.map_err(map_error)?;
    Ok(())
}

/// Runs statements in order, inside one transaction — `USE database` first, then `BEGIN
/// TRANSACTION`, then every statement, then [`end_transaction`] commits or rolls back.
///
/// `USE` runs unconditionally rather than only where a statement needs it, because two things this
/// file sends cannot be told apart from the SQL text alone: `sp_rename` and the extended-property
/// procedures behind column comments (Task 4) take no database-qualified form at all (unlike a plain
/// `ALTER TABLE`, which this file always three-part-qualifies through `mssql::three_part` and so
/// would work without it) — they act on whatever database the connection is currently sitting in. A
/// pooled connection may be sitting in any database when it is checked out, so `USE` is not optional
/// here the way it would be if every statement carried its own qualification.
async fn execute_all(pool: &Pool, database: &str, statements: Vec<String>) -> Result<(), AppError> {
    if statements.is_empty() {
        return Ok(());
    }
    let mut client = pool
        .get()
        .await
        .map_err(|e| err!("error.mssql", message = e))?;
    client
        .simple_query(format!("USE {}", quote_ident(database)))
        .await
        .map_err(map_error)?;
    client
        .simple_query("BEGIN TRANSACTION")
        .await
        .map_err(map_error)?;

    let result = execute_all_body(&mut client, statements).await;
    end_transaction(&mut client, result).await
}

async fn execute_all_body(
    client: &mut Connection,
    statements: Vec<String>,
) -> Result<(), AppError> {
    for sql in statements {
        client.simple_query(sql).await.map_err(map_error)?;
    }
    Ok(())
}

/// Checks a collation name against the one rule every SQL Server collation actually follows
/// (`Latin1_General_CI_AS`, `SQL_Latin1_General_CP1_CI_AS`, `Vietnamese_CI_AS` — letters, digits and
/// underscores only), the same check `mysql_structure::validated_collation` runs for the same reason:
/// **a collation name cannot be bracket-quoted at all** — `COLLATE [Latin1_General_CI_AS]` is a
/// syntax error, confirmed against the real test server while writing this (not assumed). So this is
/// the only guard between user-chosen text and a statement that interpolates it bare.
fn validated_collation(collation: Option<&str>) -> Result<Option<&str>, AppError> {
    let Some(collation) = collation.map(str::trim).filter(|c| !c.is_empty()) else {
        return Ok(None);
    };
    if !collation
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(err!("error.invalidCollation", collation = collation));
    }
    Ok(Some(collation))
}

/// Creates a database, `COLLATE`d when `collation` is given (D14) — the one place SQL Server's
/// collation dialog has anything to write to, since a table has none of its own (`tableCollation:
/// false`).
pub async fn create_database(
    pool: &Pool,
    name: &str,
    collation: Option<&str>,
) -> Result<(), AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(err!("error.databaseNameRequired"));
    }
    let mut sql = format!("CREATE DATABASE {}", quote_ident(name));
    if let Some(collation) = validated_collation(collation)? {
        sql.push_str(&format!(" COLLATE {collation}"));
    }
    execute_single(pool, sql).await
}

/// Drops a database and every table in it.
///
/// `USE master` first, on the very connection issuing the drop — a pooled connection may still be
/// sitting inside the database being dropped from an earlier command on it, and `DROP DATABASE`
/// refuses a database its own issuing session is using. `ALTER DATABASE ... SET SINGLE_USER WITH
/// ROLLBACK IMMEDIATE` then takes care of every *other* connection in the way, pooled ones included:
/// it forcibly disconnects them, which is not something `Manager::recycle`'s `SELECT 1` needs help
/// noticing — a connection the server has already closed simply fails that check the next time it is
/// checked out, and deadpool replaces it then, the same as any other connection that went bad.
pub async fn drop_database(pool: &Pool, name: &str) -> Result<(), AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(err!("error.databaseNameRequired"));
    }
    let quoted = quote_ident(name);
    let mut client = pool
        .get()
        .await
        .map_err(|e| err!("error.mssql", message = e))?;
    client.simple_query("USE master").await.map_err(map_error)?;
    client
        .simple_query(format!(
            "ALTER DATABASE {quoted} SET SINGLE_USER WITH ROLLBACK IMMEDIATE"
        ))
        .await
        .map_err(map_error)?;
    client
        .simple_query(format!("DROP DATABASE {quoted}"))
        .await
        .map_err(map_error)?;
    Ok(())
}

/// Creates an empty table with one column: an `int` `id` the server numbers itself, primary key —
/// the same shape MySQL and PostgreSQL create theirs with. The rest is added from the Structure tab.
///
/// The name may carry a schema, exactly as the sidebar writes one — `sales.orders` creates the table
/// in `sales`, and a bare name creates it in `dbo` (D3). The schema itself is not created if it does
/// not exist; neither does PostgreSQL's `create_table` attempt that for `public`'s siblings.
pub async fn create_table(pool: &Pool, database: &str, table: &str) -> Result<(), AppError> {
    if table.trim().is_empty() {
        return Err(err!("error.tableNameRequired"));
    }
    let (schema, name) = resolve(table.trim());
    let qualified = three_part(database, &schema, &name);
    let sql = format!(
        "CREATE TABLE {qualified} ({} int IDENTITY(1,1) PRIMARY KEY)",
        quote_ident("id")
    );
    execute_all(pool, database, vec![sql]).await
}

/// Renames a table within its schema.
///
/// `sp_rename` is the only way SQL Server renames a table — there is no ANSI `RENAME TO`. Its object
/// name is a **string**, not an identifier (the same stored-procedure convention `mssql::reset_identity`
/// already relies on for `DBCC CHECKIDENT`), and it cannot move a table to a different schema: unlike
/// PostgreSQL's two-statement rename-then-`SET SCHEMA`, T-SQL has no schema-moving DDL for a table at
/// all short of dropping and recreating it. A `new_name` that names a different schema is refused
/// outright rather than silently kept in the old one — that would be the one outcome a rename can
/// produce that looks like success and is not.
pub async fn rename_table(
    pool: &Pool,
    database: &str,
    table: &str,
    new_name: &str,
) -> Result<(), AppError> {
    let old = table.trim();
    let new_name = new_name.trim();
    if old.is_empty() || new_name.is_empty() {
        return Err(err!("error.tableNameRequired"));
    }
    let (schema, name) = resolve(old);
    let (new_schema, new_bare) = resolve(new_name);
    if new_schema != schema {
        return Err(err!("error.mssqlRenameCannotChangeSchema"));
    }
    let sql = format!(
        "EXEC sp_rename {}, {}",
        quote_string(&format!("{schema}.{name}")),
        quote_string(&new_bare)
    );
    execute_all(pool, database, vec![sql]).await
}

/// Drops a table and everything in it. Plain `DROP TABLE`, not `IF EXISTS` — the same choice
/// `postgres_ddl::drop_table` makes, for the same reason: asking to drop something not there is worth
/// being told about.
pub async fn drop_table(pool: &Pool, database: &str, table: &str) -> Result<(), AppError> {
    if table.trim().is_empty() {
        return Err(err!("error.tableNameRequired"));
    }
    let (schema, name) = resolve(table.trim());
    let qualified = three_part(database, &schema, &name);
    execute_all(pool, database, vec![format!("DROP TABLE {qualified}")]).await
}

/// The `COLLATE ...` of a column, empty when none was asked for. **Not** bracket-quoted, unlike
/// every other identifier in this file — `COLLATE [Latin1_General_CI_AS]` is a syntax error on real
/// SQL Server (confirmed against the test server; an earlier draft of this function assumed quoting
/// was safe here and it is not), so [`validated_collation`]'s allow-list is what stands between user
/// input and this string instead.
fn collate_clause(collation: Option<&str>) -> Result<String, AppError> {
    Ok(match validated_collation(collation)? {
        Some(collation) => format!(" COLLATE {collation}"),
        None => String::new(),
    })
}

/// How a DEFAULT reaches the DDL. An expression goes in as written; anything else is a literal and
/// is quoted, except `NULL` typed on its own, meant as SQL NULL rather than the four characters — the
/// same rule `postgres_ddl::default_clause` follows.
fn default_clause(value: &str, is_expression: bool) -> String {
    let trimmed = value.trim();
    if is_expression {
        return trimmed.to_string();
    }
    if trimmed.eq_ignore_ascii_case("NULL") {
        return "NULL".to_string();
    }
    quote_string(value)
}

/// The `name type ...` of a column, as `CREATE TABLE`/`ADD` spells it. The comment is not here — it
/// is a separate statement, see [`comment_statement`].
pub(super) fn column_definition(spec: &ColumnSpec) -> Result<String, AppError> {
    let name = spec.name.trim();
    if name.is_empty() {
        return Err(err!("error.columnNameRequired"));
    }
    let data_type = spec.data_type.trim();
    if data_type.is_empty() {
        return Err(err!("error.columnTypeRequired"));
    }

    let mut sql = format!("{} {data_type}", quote_ident(name));
    sql.push_str(&collate_clause(spec.collation.as_deref())?);
    if spec.auto_increment {
        // IDENTITY is NOT NULL by definition (D7) and carries no DEFAULT of its own.
        sql.push_str(" IDENTITY(1,1) NOT NULL");
        return Ok(sql);
    }
    sql.push_str(if spec.nullable { " NULL" } else { " NOT NULL" });
    if let Some(default) = &spec.default_value {
        sql.push_str(&format!(
            " DEFAULT {}",
            default_clause(default, spec.default_is_expression)
        ));
    }
    Ok(sql)
}

/// Sets, replaces or clears a column's `MS_Description` extended property — the nearest thing SQL
/// Server has to an inline column comment, and what `mssql_structure::structure_columns` already
/// reads back as `comment`. `exists` is whether the column already carries one, since the three
/// stored procedures below are not interchangeable: asking `sp_addextendedproperty` to add a second
/// one, or `sp_updateextendedproperty` to update one that is not there, is an error rather than a
/// no-op.
pub(super) fn comment_statement(
    schema: &str,
    table: &str,
    column: &str,
    comment: &str,
    exists: bool,
) -> Option<String> {
    let comment = comment.trim();
    if comment.is_empty() && !exists {
        return None;
    }
    let proc = if comment.is_empty() {
        "sp_dropextendedproperty"
    } else if exists {
        "sp_updateextendedproperty"
    } else {
        "sp_addextendedproperty"
    };
    let mut sql = format!("EXEC {proc} @name = N'MS_Description'");
    if !comment.is_empty() {
        sql.push_str(&format!(", @value = {}", quote_nstring(comment)));
    }
    sql.push_str(&format!(
        ", @level0type = N'SCHEMA', @level0name = {}, \
         @level1type = N'TABLE', @level1name = {}, \
         @level2type = N'COLUMN', @level2name = {}",
        quote_nstring(schema),
        quote_nstring(table),
        quote_nstring(column)
    ));
    Some(sql)
}

/// What a column is now, as far as [`modify_column`] (Task 5) and [`drop_column`] need to know.
struct CurrentColumn {
    object_id: i32,
    column_id: i32,
    /// The default constraint's own name, which SQL Server usually generates
    /// (`DF__t__col__1A2B3C4D`) — needed to `DROP CONSTRAINT` it, since nothing about `ALTER COLUMN`
    /// takes a default along for the ride (D15).
    default_name: Option<String>,
    identity: bool,
    /// `''` when the column has no `MS_Description` — `comment_statement`'s `exists` is
    /// `!comment.is_empty()`, so a column deliberately commented with nothing and a column never
    /// commented at all are (rarely, harmlessly) treated alike.
    comment: String,
}

/// Reads what [`modify_column`] and [`drop_column`] need to know about a column before writing to
/// it: its identity, and its default constraint's *name* (to drop it) — not its full declaration,
/// since [`modify_column`] always redeclares a column in full rather than diffing against what it
/// was (D15's "whether or not each one changed"), so a fuller read here would go unused.
async fn current_column(
    pool: &Pool,
    database: &str,
    schema: &str,
    table: &str,
    column: &str,
) -> Result<CurrentColumn, AppError> {
    let db = quote_ident(database);
    let sql = format!(
        "SELECT c.object_id, c.column_id, c.is_identity, d.name AS default_name,
                COALESCE(CAST(ep.value AS nvarchar(max)), '') AS comment
         FROM {db}.sys.columns c
         JOIN {db}.sys.objects o ON o.object_id = c.object_id
         JOIN {db}.sys.schemas s ON s.schema_id = o.schema_id
         LEFT JOIN {db}.sys.default_constraints d ON d.object_id = c.default_object_id
         LEFT JOIN {db}.sys.extended_properties ep
             ON ep.major_id = c.object_id AND ep.minor_id = c.column_id
             AND ep.class = 1 AND ep.name = 'MS_Description'
         WHERE s.name = @P1 AND o.name = @P2 AND c.name = @P3"
    );

    let mut client = pool
        .get()
        .await
        .map_err(|e| err!("error.mssql", message = e))?;
    let row = client
        .query(sql, &[&schema, &table, &column])
        .await
        .map_err(map_error)?
        .into_row()
        .await
        .map_err(map_error)?
        .ok_or_else(|| err!("error.unknownColumn", table = table, name = column))?;

    Ok(CurrentColumn {
        object_id: row.get("object_id").unwrap_or(0),
        column_id: row.get("column_id").unwrap_or(0),
        default_name: row.get::<&str, _>("default_name").map(str::to_string),
        identity: row.get("is_identity").unwrap_or(false),
        comment: row.get::<&str, _>("comment").unwrap_or("").to_string(),
    })
}

/// Every index — plain, unique, or the primary key — that covers `column_id`, full column list
/// included (not just the one column asked about): dropping and recreating half of a composite index
/// would silently narrow it. Mirrors `mssql_structure::table_indexes`'s query almost exactly, with an
/// added `EXISTS` to filter to indexes that reach the one column [`modify_column`]/[`drop_column`]
/// care about.
async fn dependent_indexes(
    pool: &Pool,
    database: &str,
    object_id: i32,
    column_id: i32,
) -> Result<Vec<TableIndex>, AppError> {
    let db = quote_ident(database);
    let sql = format!(
        "SELECT i.name, i.is_unique, i.is_primary_key, LOWER(i.type_desc) AS index_type,
                c.name AS column_name, ic.key_ordinal
         FROM {db}.sys.indexes i
         JOIN {db}.sys.index_columns ic
             ON ic.object_id = i.object_id AND ic.index_id = i.index_id
             AND ic.is_included_column = 0
         JOIN {db}.sys.columns c ON c.object_id = ic.object_id AND c.column_id = ic.column_id
         WHERE i.object_id = @P1 AND i.index_id > 0 AND i.is_hypothetical = 0
           AND EXISTS (
               SELECT 1 FROM {db}.sys.index_columns ic2
               WHERE ic2.object_id = i.object_id AND ic2.index_id = i.index_id
                 AND ic2.column_id = @P2
           )
         ORDER BY i.is_primary_key DESC, i.name, ic.key_ordinal"
    );

    let mut client = pool
        .get()
        .await
        .map_err(|e| err!("error.mssql", message = e))?;
    let rows = client
        .query(sql, &[&object_id, &column_id])
        .await
        .map_err(map_error)?
        .into_first_result()
        .await
        .map_err(map_error)?;

    let mut indexes: Vec<TableIndex> = Vec::new();
    for row in &rows {
        let Some(name) = row.get::<&str, _>("name") else {
            continue;
        };
        if indexes.last().map(|last| last.name != name).unwrap_or(true) {
            indexes.push(TableIndex {
                name: name.to_string(),
                unique: row.get("is_unique").unwrap_or(false),
                primary: row.get("is_primary_key").unwrap_or(false),
                index_type: row.get::<&str, _>("index_type").unwrap_or("").to_string(),
                columns: Vec::new(),
                comment: String::new(),
            });
        }
        if let Some(index) = indexes.last_mut() {
            index.columns.push(IndexColumn {
                name: row.get::<&str, _>("column_name").map(str::to_string),
                prefix_length: None,
            });
        }
    }
    Ok(indexes)
}

pub async fn add_column(
    pool: &Pool,
    database: &str,
    table: &str,
    spec: &ColumnSpec,
) -> Result<(), AppError> {
    let (schema, table_name) = resolve(table);
    let qualified = three_part(database, &schema, &table_name);
    let definition = column_definition(spec)?;

    let mut statements = vec![format!("ALTER TABLE {qualified} ADD {definition}")];
    if let Some(sql) =
        comment_statement(&schema, &table_name, spec.name.trim(), &spec.comment, false)
    {
        statements.push(sql);
    }
    execute_all(pool, database, statements).await
}

/// Drops a column — its default constraint and every index covering it first: both block `DROP
/// COLUMN` exactly as they block `ALTER COLUMN` (D15), and neither is rebuilt afterwards, since the
/// column they covered no longer exists to cover. A comment needs no cleanup of its own: SQL Server
/// drops a column's extended properties along with the column.
pub async fn drop_column(
    pool: &Pool,
    database: &str,
    table: &str,
    name: &str,
) -> Result<(), AppError> {
    let column = name.trim();
    if column.is_empty() {
        return Err(err!("error.columnNameRequired"));
    }
    let (schema, table_name) = resolve(table);
    let qualified = three_part(database, &schema, &table_name);
    let current = current_column(pool, database, &schema, &table_name, column).await?;

    let mut statements = Vec::new();
    for index in dependent_indexes(pool, database, current.object_id, current.column_id).await? {
        statements
            .push(drop_index_statement(pool, database, &schema, &table_name, &index.name).await?);
    }
    if let Some(default_name) = &current.default_name {
        statements.push(format!(
            "ALTER TABLE {qualified} DROP CONSTRAINT {}",
            quote_ident(default_name)
        ));
    }
    statements.push(format!(
        "ALTER TABLE {qualified} DROP COLUMN {}",
        quote_ident(column)
    ));
    execute_all(pool, database, statements).await
}

/// The statement that removes an index by name — `DROP CONSTRAINT` for one backing a primary key or
/// a unique constraint, `DROP INDEX ... ON ...` for a plain or unique index. `sys.indexes` carries
/// both flags directly (`is_primary_key`, `is_unique_constraint`), unlike PostgreSQL, where the same
/// question needs a join out to `pg_constraint` (see `postgres_ddl::drop_index_statement`).
async fn drop_index_statement(
    pool: &Pool,
    database: &str,
    schema: &str,
    table: &str,
    name: &str,
) -> Result<String, AppError> {
    let db = quote_ident(database);
    // The two flags read separately and OR'd in Rust, not folded into one `CASE WHEN ... THEN 1
    // ELSE 0 END` column: SQL Server infers that expression's type from its integer literals as
    // `int`, not `bit` — so the two real `bit` columns underneath it come back as `I32`, and
    // `Row::get::<bool, _>` panics on the mismatch instead of erroring gracefully. Confirmed by
    // reproducing the user's report of the Structure tab hanging on "edit index to unique": the
    // panic inside this Tauri command's future left its IPC promise unresolved forever, which is
    // what showed up as a hang rather than an error dialog.
    let sql = format!(
        "SELECT i.is_primary_key, i.is_unique_constraint
         FROM {db}.sys.indexes i
         JOIN {db}.sys.objects o ON o.object_id = i.object_id
         JOIN {db}.sys.schemas s ON s.schema_id = o.schema_id
         WHERE s.name = @P1 AND o.name = @P2 AND i.name = @P3"
    );
    let mut client = pool
        .get()
        .await
        .map_err(|e| err!("error.mssql", message = e))?;
    let row = client
        .query(sql, &[&schema, &table, &name])
        .await
        .map_err(map_error)?
        .into_row()
        .await
        .map_err(map_error)?;
    let is_constraint = row
        .map(|row| row.get::<bool, _>(0).unwrap_or(false) || row.get::<bool, _>(1).unwrap_or(false))
        .unwrap_or(false);

    let qualified = three_part(database, schema, table);
    Ok(if is_constraint {
        format!(
            "ALTER TABLE {qualified} DROP CONSTRAINT {}",
            quote_ident(name)
        )
    } else {
        format!("DROP INDEX {} ON {qualified}", quote_ident(name))
    })
}

/// SQL Server's own two — a physical row order, not an access-method choice the way PostgreSQL's
/// `USING gin`/`USING gist` is. `None` leaves the server to decide, which means NONCLUSTERED for
/// `CREATE INDEX` and CLUSTERED for `ADD CONSTRAINT ... PRIMARY KEY` unless the table already has one.
fn validated_index_type(index_type: Option<&str>) -> Result<Option<&'static str>, AppError> {
    let Some(index_type) = index_type.map(str::trim).filter(|t| !t.is_empty()) else {
        return Ok(None);
    };
    match index_type.to_uppercase().as_str() {
        "CLUSTERED" => Ok(Some("CLUSTERED")),
        "NONCLUSTERED" => Ok(Some("NONCLUSTERED")),
        other => Err(err!("error.unknownIndexType", type = other)),
    }
}

/// An index name short enough for SQL Server's 128-character identifier limit — `CREATE INDEX` has
/// no unnamed form the way PostgreSQL's does, so a client leaving the name box empty needs one made
/// up on its behalf, the way it would have had to itself.
fn generated_index_name(table: &str, columns: &[IndexColumnSpec]) -> String {
    let mut name = format!(
        "IX_{table}_{}",
        columns
            .iter()
            .map(|c| c.name.trim())
            .collect::<Vec<_>>()
            .join("_")
    );
    name.truncate(128);
    name
}

/// The statement that creates one index — an `ALTER TABLE ... ADD [CONSTRAINT] ... PRIMARY KEY` for
/// a primary key, a `CREATE INDEX` for everything else.
fn create_index_statements(
    database: &str,
    schema: &str,
    table: &str,
    spec: &IndexSpec,
) -> Result<Vec<String>, AppError> {
    if spec.columns.is_empty() {
        return Err(err!("error.indexNeedsColumn"));
    }
    let columns = spec
        .columns
        .iter()
        .map(|column| {
            if column.name.trim().is_empty() {
                return Err(err!("error.indexColumnNameRequired"));
            }
            Ok(quote_ident(column.name.trim()))
        })
        .collect::<Result<Vec<_>, AppError>>()?
        .join(", ");

    let qualified = three_part(database, schema, table);
    let kind = spec.kind.to_lowercase();
    let using = validated_index_type(spec.index_type.as_deref())?
        .map(|method| format!("{method} "))
        .unwrap_or_default();

    if kind == "primary" {
        // `ADD CONSTRAINT name` may be left off entirely — the one place T-SQL's grammar lets a
        // constraint go unnamed.
        let name = spec.name.trim();
        let named = if name.is_empty() {
            String::new()
        } else {
            format!("CONSTRAINT {} ", quote_ident(name))
        };
        return Ok(vec![format!(
            "ALTER TABLE {qualified} ADD {named}PRIMARY KEY {using}({columns})"
        )]);
    }

    let unique = match kind.as_str() {
        "index" => "",
        "unique" => "UNIQUE ",
        other => return Err(err!("error.unknownIndexKind", kind = other)),
    };
    let name = spec.name.trim();
    let name = if name.is_empty() {
        generated_index_name(table, &spec.columns)
    } else {
        name.to_string()
    };
    Ok(vec![format!(
        "CREATE {unique}{using}INDEX {} ON {qualified} ({columns})",
        quote_ident(&name)
    )])
    // No comment statement: SQL Server keeps no comment on an index — see
    // `mssql_structure::TableIndex.comment`.
}

/// Turns one read-back `TableIndex` into the `IndexSpec` that recreates it — used only to put an
/// index back after [`modify_column`] has had to drop it out of `ALTER COLUMN`'s way.
fn index_spec_from(index: &TableIndex) -> IndexSpec {
    IndexSpec {
        name: index.name.clone(),
        kind: if index.primary {
            "primary".to_string()
        } else if index.unique {
            "unique".to_string()
        } else {
            "index".to_string()
        },
        index_type: Some(index.index_type.to_uppercase()),
        columns: index
            .columns
            .iter()
            .filter_map(|c| c.name.clone())
            .map(|name| IndexColumnSpec { name })
            .collect(),
    }
}

/// Redefines an existing column, `name` being what it is called now — so this is also how a column
/// is renamed.
///
/// SQL Server has no statement that restates a column whole the way MySQL's `CHANGE COLUMN` does, and
/// unlike PostgreSQL's incremental `ALTER COLUMN` clauses, its single `ALTER COLUMN` **replaces the
/// whole declaration** and is blocked outright by a default constraint or an index on the column
/// (D15). So this reads what is in the way first, clears it, redeclares the column in full — type,
/// nullable and collation together, whether or not each one changed, since leaving `NOT NULL` off
/// `ALTER COLUMN` silently makes the column nullable — then puts back what it cleared.
///
/// An `IDENTITY` column is locked further still: `ALTER COLUMN` is refused outright on one, full
/// stop, not merely for its identity property — so `current.identity` skips the `ALTER COLUMN`
/// statement entirely and only a rename or a comment change can still land. The Structure tab locks
/// type/nullable/collation on such a column (Task 9), so this has nothing to disagree with in the
/// ordinary path; `auto_increment` toggling either way is refused outright since there is no
/// statement that does it (D15's explicit non-goal).
pub async fn modify_column(
    pool: &Pool,
    database: &str,
    table: &str,
    name: &str,
    spec: &ColumnSpec,
) -> Result<(), AppError> {
    let old_name = name.trim();
    let new_name = spec.name.trim();
    if old_name.is_empty() || new_name.is_empty() {
        return Err(err!("error.columnNameRequired"));
    }
    let data_type = spec.data_type.trim();
    if data_type.is_empty() {
        return Err(err!("error.columnTypeRequired"));
    }

    let (schema, table_name) = resolve(table);
    let qualified = three_part(database, &schema, &table_name);
    let current = current_column(pool, database, &schema, &table_name, old_name).await?;

    if spec.auto_increment != current.identity {
        return Err(err!("error.mssqlIdentityToggleNotSupported"));
    }

    let dependent = if current.identity {
        Vec::new()
    } else {
        dependent_indexes(pool, database, current.object_id, current.column_id).await?
    };

    let mut statements = Vec::new();
    for index in &dependent {
        statements
            .push(drop_index_statement(pool, database, &schema, &table_name, &index.name).await?);
    }
    if let Some(default_name) = &current.default_name {
        statements.push(format!(
            "ALTER TABLE {qualified} DROP CONSTRAINT {}",
            quote_ident(default_name)
        ));
    }
    if new_name != old_name {
        statements.push(format!(
            "EXEC sp_rename {}, {}, 'COLUMN'",
            quote_string(&format!("{schema}.{table_name}.{old_name}")),
            quote_string(new_name)
        ));
    }

    if !current.identity {
        statements.push(format!(
            "ALTER TABLE {qualified} ALTER COLUMN {} {data_type}{} {}",
            quote_ident(new_name),
            collate_clause(spec.collation.as_deref())?,
            if spec.nullable { "NULL" } else { "NOT NULL" }
        ));
    }

    if let Some(default_value) = &spec.default_value {
        statements.push(format!(
            "ALTER TABLE {qualified} ADD DEFAULT {} FOR {}",
            default_clause(default_value, spec.default_is_expression),
            quote_ident(new_name)
        ));
    }

    for index in &dependent {
        statements.extend(create_index_statements(
            database,
            &schema,
            &table_name,
            &index_spec_from(index),
        )?);
    }

    if spec.comment.trim() != current.comment {
        if let Some(sql) = comment_statement(
            &schema,
            &table_name,
            new_name,
            &spec.comment,
            !current.comment.is_empty(),
        ) {
            statements.push(sql);
        }
    }

    execute_all(pool, database, statements).await
}

pub async fn add_index(
    pool: &Pool,
    database: &str,
    table: &str,
    spec: &IndexSpec,
) -> Result<(), AppError> {
    let (schema, name) = resolve(table);
    let statements = create_index_statements(database, &schema, &name, spec)?;
    execute_all(pool, database, statements).await
}

/// Replaces an index: SQL Server cannot alter one in place, so the old one is dropped and the new one
/// created — both inside the one transaction `execute_all` wraps everything in, which is what keeps
/// the table from spending any time without an index at all.
pub async fn modify_index(
    pool: &Pool,
    database: &str,
    table: &str,
    name: &str,
    spec: &IndexSpec,
) -> Result<(), AppError> {
    let old_name = name.trim();
    if old_name.is_empty() {
        return Err(err!("error.indexNameRequired"));
    }
    let (schema, table_name) = resolve(table);
    let drop_stmt = drop_index_statement(pool, database, &schema, &table_name, old_name).await?;
    let mut statements = vec![drop_stmt];
    statements.extend(create_index_statements(
        database,
        &schema,
        &table_name,
        spec,
    )?);
    execute_all(pool, database, statements).await
}

pub async fn drop_index(
    pool: &Pool,
    database: &str,
    table: &str,
    name: &str,
) -> Result<(), AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(err!("error.indexNameRequired"));
    }
    let (schema, table_name) = resolve(table);
    let statement = drop_index_statement(pool, database, &schema, &table_name, name).await?;
    execute_all(pool, database, vec![statement]).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quote_inside_a_string_literal_is_doubled() {
        assert_eq!(quote_string("it's"), "'it''s'");
        assert_eq!(quote_string("a\\b"), "'a\\b'");
    }

    #[test]
    fn an_nstring_carries_the_unicode_prefix() {
        assert_eq!(quote_nstring("mới"), "N'mới'");
        assert_eq!(quote_nstring("it's"), "N'it''s'");
    }

    fn spec(name: &str, data_type: &str) -> ColumnSpec {
        ColumnSpec {
            name: name.to_string(),
            data_type: data_type.to_string(),
            nullable: true,
            default_value: None,
            default_is_expression: false,
            auto_increment: false,
            collation: None,
            comment: String::new(),
        }
    }

    #[test]
    fn a_plain_column_is_named_typed_and_nullable() {
        assert_eq!(
            column_definition(&spec("title", "nvarchar(255)")).unwrap(),
            "[title] nvarchar(255) NULL"
        );
    }

    #[test]
    fn an_identity_column_takes_neither_null_nor_default() {
        let mut spec = spec("id", "int");
        spec.auto_increment = true;
        spec.default_value = Some("7".to_string());
        assert_eq!(
            column_definition(&spec).unwrap(),
            "[id] int IDENTITY(1,1) NOT NULL"
        );
    }

    #[test]
    fn a_collation_is_written_bare_not_bracket_quoted() {
        // `COLLATE [Latin1_General_CI_AS]` is a syntax error on real SQL Server — confirmed against
        // the test server. A collation name is validated against an allow-list instead of quoted.
        let mut spec = spec("name", "nvarchar(50)");
        spec.collation = Some("Latin1_General_CI_AS".to_string());
        assert_eq!(
            column_definition(&spec).unwrap(),
            "[name] nvarchar(50) COLLATE Latin1_General_CI_AS NULL"
        );
    }

    #[test]
    fn a_collation_with_a_character_outside_the_allow_list_is_refused() {
        let mut spec = spec("name", "nvarchar(50)");
        spec.collation = Some("Latin1_General_CI_AS; DROP TABLE t".to_string());
        assert!(column_definition(&spec).is_err());
    }

    #[test]
    fn a_literal_default_is_quoted_and_an_expression_is_not() {
        assert_eq!(default_clause("new", false), "'new'");
        assert_eq!(default_clause("it's", false), "'it''s'");
        assert_eq!(default_clause("getdate()", true), "getdate()");
        assert_eq!(default_clause("NULL", false), "NULL");
    }

    #[test]
    fn a_new_column_with_a_comment_only_ever_adds_one() {
        assert_eq!(
            comment_statement("dbo", "t", "c", "hello", false),
            Some(
                "EXEC sp_addextendedproperty @name = N'MS_Description', @value = N'hello', \
                 @level0type = N'SCHEMA', @level0name = N'dbo', @level1type = N'TABLE', \
                 @level1name = N't', @level2type = N'COLUMN', @level2name = N'c'"
                    .to_string()
            )
        );
    }

    #[test]
    fn clearing_an_existing_comment_drops_the_property_rather_than_setting_it_empty() {
        assert_eq!(
            comment_statement("dbo", "t", "c", "", true),
            Some(
                "EXEC sp_dropextendedproperty @name = N'MS_Description', @level0type = N'SCHEMA', \
                 @level0name = N'dbo', @level1type = N'TABLE', @level1name = N't', \
                 @level2type = N'COLUMN', @level2name = N'c'"
                    .to_string()
            )
        );
    }

    #[test]
    fn no_comment_and_none_before_writes_nothing() {
        assert_eq!(comment_statement("dbo", "t", "c", "", false), None);
    }
}

#[cfg(test)]
mod index_tests {
    use super::*;

    fn column(name: &str) -> IndexColumnSpec {
        IndexColumnSpec {
            name: name.to_string(),
        }
    }

    #[test]
    fn a_unique_index_names_its_method_and_columns() {
        let spec = IndexSpec {
            name: "orders_customer".to_string(),
            kind: "unique".to_string(),
            index_type: Some("nonclustered".to_string()),
            columns: vec![column("customer"), column("placed")],
        };
        assert_eq!(
            create_index_statements("mixlab_agent_test", "sales", "orders", &spec).unwrap(),
            vec![
                "CREATE UNIQUE NONCLUSTERED INDEX [orders_customer] ON [mixlab_agent_test].[sales].[orders] ([customer], [placed])"
            ]
        );
    }

    #[test]
    fn an_unnamed_index_gets_one_generated_from_its_table_and_columns() {
        let spec = IndexSpec {
            name: String::new(),
            kind: "index".to_string(),
            index_type: None,
            columns: vec![column("email")],
        };
        assert_eq!(
            create_index_statements("db", "dbo", "users", &spec).unwrap(),
            vec!["CREATE INDEX [IX_users_email] ON [db].[dbo].[users] ([email])"]
        );
    }

    #[test]
    fn a_primary_key_may_go_unnamed_unlike_a_plain_index() {
        let spec = IndexSpec {
            name: String::new(),
            kind: "primary".to_string(),
            index_type: Some("CLUSTERED".to_string()),
            columns: vec![column("id")],
        };
        assert_eq!(
            create_index_statements("db", "dbo", "t", &spec).unwrap(),
            vec!["ALTER TABLE [db].[dbo].[t] ADD PRIMARY KEY CLUSTERED ([id])"]
        );
    }

    #[test]
    fn an_index_kind_mssql_has_not_is_refused() {
        let spec = IndexSpec {
            name: String::new(),
            kind: "fulltext".to_string(),
            index_type: None,
            columns: vec![column("body")],
        };
        assert!(create_index_statements("db", "dbo", "t", &spec).is_err());
    }

    #[test]
    fn only_clustered_or_nonclustered_reach_the_sql() {
        assert_eq!(
            validated_index_type(Some("clustered")).unwrap(),
            Some("CLUSTERED")
        );
        assert_eq!(validated_index_type(Some("  ")).unwrap(), None);
        assert!(validated_index_type(Some("btree")).is_err());
    }

    #[test]
    fn a_generated_name_is_never_longer_than_the_identifier_limit() {
        let many_columns: Vec<IndexColumnSpec> =
            (0..40).map(|i| column(&format!("column_{i}"))).collect();
        assert!(generated_index_name("a_very_long_table_name_indeed", &many_columns).len() <= 128);
    }
}
