//! Creating and changing schema on ClickHouse: databases, tables, columns.
//!
//! A module of its own rather than more of `clickhouse.rs`, the same way `postgres/ddl.rs` sits
//! beside `postgres.rs`: that file is what reads, and DDL is a large enough job to stand apart.
//!
//! Every statement is built by a pure function and only then sent, so what to *write* is decided
//! where a test can read it and sending is left as `execute_check`. There is no transaction here —
//! ClickHouse has none — so a run of statements that fails partway leaves exactly what already ran.

use super::structure::{as_u64, structure_columns, StructureColumn};
use super::{execute_check, qualified, query, quote_ident, quote_literal, Connection};
use crate::error::AppError;
use serde::Deserialize;

/// The body of a ClickHouse string literal, or `None` for text that is not one whole literal.
///
/// "Whole" is what matters: `'a' || 'b'` also opens and closes with a quote but is an expression,
/// so the closing quote has to be the last character before this counts. Backslash escapes, the
/// same convention `clickhouse::quote_literal` writes them back out with.
pub(super) fn literal_body(text: &str) -> Option<String> {
    let mut chars = text.char_indices();
    if chars.next()?.1 != '\'' {
        return None;
    }
    let mut body = String::new();
    while let Some((index, ch)) = chars.next() {
        match ch {
            '\\' => body.push(chars.next()?.1),
            '\'' => {
                return if index + 1 == text.len() {
                    Some(body)
                } else {
                    None
                };
            }
            _ => body.push(ch),
        }
    }
    None
}

/// What `system.columns.default_expression` reports, split into the value to show and whether it
/// is an expression rather than a literal: `'active'` is the literal `active`, `now()` is an
/// expression, and so is `42` — writing it back out verbatim is right either way, and ClickHouse
/// gives nothing to tell a bare number from a function call with no arguments.
///
/// The inverse is `default_clause`, which writes the same value back into SQL.
pub(super) fn read_default(expression: &str) -> Option<(String, bool)> {
    let text = expression.trim();
    if text.is_empty() {
        return None;
    }
    match literal_body(text) {
        Some(body) => Some((body, false)),
        None => Some((text.to_string(), true)),
    }
}

/// The engines the "create table" dialog offers, in the order they are shown.
///
/// Four, not six: `CollapsingMergeTree` and `VersionedCollapsingMergeTree` require a parameter
/// each — a `sign` column, and a `version` one — naming a column that does not exist yet when the
/// table is created, and the server refuses them outright (`Code: 42 ... requires 1 parameter`,
/// checked against the test server). Choosing an engine cannot be undone once the table exists, so
/// the list holds only the ones certain to work.
pub const ENGINES: [&str; 4] = [
    "MergeTree",
    "ReplacingMergeTree",
    "SummingMergeTree",
    "AggregatingMergeTree",
];

/// The `CREATE TABLE` for a new table.
///
/// The table arrives nearly empty — one `id UInt64` column — and grows its real columns in the
/// Structure tab, exactly as on the other three engines. `ORDER BY tuple()` is an empty sorting
/// key: only `id` exists at this point, so any other choice would be guessing on the user's
/// behalf, and setting a real sorting key belongs to the index design.
///
/// The engine name goes into the SQL bare — there is no way to quote one — so the only safe thing
/// to do with it is refuse anything not in [`ENGINES`], the same posture
/// `mysql::structure::validated_collation` takes for a collation.
pub fn create_table_statement(
    database: &str,
    table: &str,
    engine: &str,
) -> Result<String, AppError> {
    let table = table.trim();
    if table.is_empty() {
        return Err(err!("error.tableNameRequired"));
    }
    let engine = ENGINES
        .iter()
        .find(|known| **known == engine.trim())
        .ok_or_else(|| err!("error.clickhouseUnknownEngine", engine = engine))?;
    Ok(format!(
        "CREATE TABLE {} (`id` UInt64) ENGINE = {engine} ORDER BY tuple()",
        qualified(database, table)
    ))
}

pub async fn create_table(
    conn: &Connection,
    database: &str,
    table: &str,
    engine: &str,
) -> Result<(), AppError> {
    execute_check(
        conn,
        &create_table_statement(database, table, engine)?,
        None,
    )
    .await
}

/// Renames a table within its database. ClickHouse's `RENAME TABLE` is a metadata change and
/// touches no part on disk.
pub async fn rename_table(
    conn: &Connection,
    database: &str,
    table: &str,
    new_name: &str,
) -> Result<(), AppError> {
    let new_name = new_name.trim();
    if new_name.is_empty() {
        return Err(err!("error.tableNameRequired"));
    }
    let sql = format!(
        "RENAME TABLE {} TO {}",
        qualified(database, table),
        qualified(database, new_name)
    );
    execute_check(conn, &sql, None).await
}

pub async fn drop_table(conn: &Connection, database: &str, table: &str) -> Result<(), AppError> {
    let sql = format!("DROP TABLE {}", qualified(database, table));
    execute_check(conn, &sql, None).await
}

/// Creates a database. No collation: ClickHouse has no such thing, so the argument
/// `SqlApi.createDatabase` carries for the other three engines is dropped at the command layer.
pub async fn create_database(conn: &Connection, name: &str) -> Result<(), AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(err!("error.databaseNameRequired"));
    }
    let sql = format!("CREATE DATABASE {}", quote_ident(name));
    execute_check(conn, &sql, None).await
}

pub async fn drop_database(conn: &Connection, name: &str) -> Result<(), AppError> {
    let sql = format!("DROP DATABASE {}", quote_ident(name));
    execute_check(conn, &sql, None).await
}

/// A `DEFAULT` clause ready to be appended to a column definition — the leading space is part of
/// it, so a caller only has to `push_str`. The inverse of [`read_default`]: a literal goes back out
/// quoted exactly the way it was unquoted coming in.
pub(super) fn default_clause(value: &str, is_expression: bool) -> String {
    if is_expression {
        format!(" DEFAULT {}", value.trim())
    } else {
        format!(" DEFAULT {}", quote_literal(value))
    }
}

/// What a column is to be declared as — the write-side counterpart of
/// `structure::StructureColumn`.
///
/// Narrower than the other three engines' `ColumnSpec` because ClickHouse has less: `nullable`
/// lives inside `data_type` (`Nullable(T)`, wrapped by the dialog before it is sent), there is no
/// `AUTO_INCREMENT`, no `ON UPDATE CURRENT_TIMESTAMP`, no per-column collation, and `after` is of
/// no use since `MODIFY COLUMN` cannot move a column (`SqlEditing.columnPosition: false`). Those
/// fields are still sent from the frontend through the shared `SqlColumnSpec` shape; serde drops a
/// field the struct has not got rather than refusing the payload.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnSpec {
    pub name: String,
    pub data_type: String,
    /// `None` writes no `DEFAULT` at all — and, on an existing column, drops the one it has.
    #[serde(default)]
    pub default_value: Option<String>,
    #[serde(default)]
    pub default_is_expression: bool,
    #[serde(default)]
    pub comment: String,
}

/// What this engine reads of the column the window sends; the rest is not its to use.
impl From<crate::modules::db::drivers::sql::types::ColumnSpec> for ColumnSpec {
    fn from(spec: crate::modules::db::drivers::sql::types::ColumnSpec) -> Self {
        Self {
            name: spec.name,
            data_type: spec.data_type,
            default_value: spec.default_value,
            default_is_expression: spec.default_is_expression,
            comment: spec.comment,
        }
    }
}

/// A column declared whole — what follows `ADD COLUMN` and equally what follows `MODIFY COLUMN`,
/// ClickHouse taking the same syntax in both places. PostgreSQL cannot: there each property is an
/// `ALTER COLUMN SET ...` of its own.
///
/// No `NULL`/`NOT NULL`: nullability is part of how the type is spelled here, and the dialog has
/// already wrapped `Nullable(...)` around `data_type` where it belongs.
pub fn column_definition(spec: &ColumnSpec) -> Result<String, AppError> {
    let name = spec.name.trim();
    if name.is_empty() {
        return Err(err!("error.columnNameRequired"));
    }
    let data_type = spec.data_type.trim();
    if data_type.is_empty() {
        return Err(err!("error.columnTypeRequired"));
    }
    let mut sql = format!("{} {data_type}", quote_ident(name));
    if let Some(value) = spec.default_value.as_deref() {
        sql.push_str(&default_clause(value, spec.default_is_expression));
    }
    if !spec.comment.is_empty() {
        sql.push_str(&format!(" COMMENT {}", quote_literal(&spec.comment)));
    }
    Ok(sql)
}

/// Adds a column, always at the end of the table.
///
/// `ADD COLUMN ... AFTER` does exist, but `MODIFY COLUMN` cannot move a column afterwards and
/// `SqlEditing.columnPosition` is one flag covering both — so no position is offered at all, the
/// same as on PostgreSQL and SQLite.
pub async fn add_column(
    conn: &Connection,
    database: &str,
    table: &str,
    spec: &ColumnSpec,
) -> Result<(), AppError> {
    let sql = format!(
        "ALTER TABLE {} ADD COLUMN {}",
        qualified(database, table),
        column_definition(spec)?
    );
    execute_check(conn, &sql, None).await
}

pub async fn drop_column(
    conn: &Connection,
    database: &str,
    table: &str,
    name: &str,
) -> Result<(), AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(err!("error.columnNameRequired"));
    }
    let sql = format!(
        "ALTER TABLE {} DROP COLUMN {}",
        qualified(database, table),
        quote_ident(name)
    );
    execute_check(conn, &sql, None).await
}

/// What a data skipping index is declared as — the write-side counterpart of
/// `structure::SkipIndex`. See the ClickHouse index DDL design doc's D2/D3.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkipIndexSpec {
    pub name: String,
    pub expr: String,
    pub index_type: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub granularity: u64,
}

/// `TYPE name(args) GRANULARITY n`, or `TYPE name GRANULARITY n` when the type takes none —
/// `GRANULARITY` is not required by the server (it defaults to `1` when omitted, checked against the
/// test server), but is always written here so the dialog and the server never disagree about the
/// default.
fn skip_index_type_clause(spec: &SkipIndexSpec) -> String {
    if spec.args.is_empty() {
        format!("TYPE {} GRANULARITY {}", spec.index_type, spec.granularity)
    } else {
        format!(
            "TYPE {}({}) GRANULARITY {}",
            spec.index_type,
            spec.args.join(", "),
            spec.granularity
        )
    }
}

/// `expr` is spliced in verbatim rather than quoted as an identifier or a literal: it is an
/// expression (`lower(note)` is as common a case as a bare column name), and there is no single
/// quoting rule that would be right for both.
pub fn add_skip_index_statement(
    database: &str,
    table: &str,
    spec: &SkipIndexSpec,
) -> Result<String, AppError> {
    let name = spec.name.trim();
    if name.is_empty() {
        return Err(err!("error.indexNameRequired"));
    }
    let expr = spec.expr.trim();
    if expr.is_empty() {
        return Err(err!("error.clickhouseSkipIndexExprRequired"));
    }
    Ok(format!(
        "ALTER TABLE {} ADD INDEX {} {expr} {}",
        qualified(database, table),
        quote_ident(name),
        skip_index_type_clause(spec)
    ))
}

pub async fn add_skip_index(
    conn: &Connection,
    database: &str,
    table: &str,
    spec: &SkipIndexSpec,
) -> Result<(), AppError> {
    execute_check(
        conn,
        &add_skip_index_statement(database, table, spec)?,
        None,
    )
    .await
}

pub async fn drop_skip_index(
    conn: &Connection,
    database: &str,
    table: &str,
    name: &str,
) -> Result<(), AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(err!("error.indexNameRequired"));
    }
    let sql = format!(
        "ALTER TABLE {} DROP INDEX {}",
        qualified(database, table),
        quote_ident(name)
    );
    execute_check(conn, &sql, None).await
}

/// `MODIFY INDEX ... TYPE ...` is a syntax error on ClickHouse (checked against the test server) —
/// an edit is a drop and a re-add, the same shape MySQL's index dialog already documents.
pub(super) fn modify_skip_index_statements(
    database: &str,
    table: &str,
    old_name: &str,
    spec: &SkipIndexSpec,
) -> Result<Vec<String>, AppError> {
    let old_name = old_name.trim();
    if old_name.is_empty() {
        return Err(err!("error.indexNameRequired"));
    }
    Ok(vec![
        format!(
            "ALTER TABLE {} DROP INDEX {}",
            qualified(database, table),
            quote_ident(old_name)
        ),
        add_skip_index_statement(database, table, spec)?,
    ])
}

pub async fn modify_skip_index(
    conn: &Connection,
    database: &str,
    table: &str,
    old_name: &str,
    spec: &SkipIndexSpec,
) -> Result<(), AppError> {
    for statement in modify_skip_index_statements(database, table, old_name, spec)? {
        execute_check(conn, &statement, None).await?;
    }
    Ok(())
}

/// A cheap `count()`, read both before a rebuild (so its warning can say how much it is about to
/// copy) and twice during one (so a mismatch after the copy can abort it — see `rebuild_order_by`).
pub async fn row_count(conn: &Connection, database: &str, table: &str) -> Result<u64, AppError> {
    let result = query(
        conn,
        &format!("SELECT count() AS n FROM {}", qualified(database, table)),
    )
    .await?;
    as_u64(result.data.first().and_then(|row| row.get("n")))
        .ok_or_else(|| err!("error.clickhouseRebuildParse"))
}

/// Rebuilds the whole table with a new sorting key: `ALTER TABLE ... MODIFY ORDER BY` cannot do this
/// for columns that already exist (checked against the test server — it refuses with
/// `Existing column X is used in the expression that was added to the sorting key`), so this copies
/// the table into a new one built with the key already right, then swaps names.
///
/// No transaction holds the steps together — ClickHouse has none. Every step before `EXCHANGE
/// TABLES` leaves the original table untouched on failure; the two after it either both succeed
/// (`Ok(None)`) or the swap succeeds and only the temp table's own cleanup fails (`Ok(Some(name))`,
/// not an error — the sorting key change itself has already landed). See the design doc's D7/D8.
pub async fn rebuild_order_by(
    conn: &Connection,
    database: &str,
    table: &str,
    columns: &[String],
) -> Result<Option<String>, AppError> {
    if columns.is_empty() {
        return Err(err!("error.clickhouseOrderByColumnsRequired"));
    }

    let show_create = query(
        conn,
        &format!("SHOW CREATE TABLE {}", qualified(database, table)),
    )
    .await?
    .data
    .first()
    .and_then(|row| row.get("statement")?.as_str().map(str::to_string))
    .ok_or_else(|| err!("error.clickhouseRebuildParse"))?;

    let temp_name = temp_table_name(table);
    let create_sql = rebuild_ddl(
        &show_create,
        database,
        &temp_name,
        &order_by_clause(columns),
    )?;
    execute_check(conn, &create_sql, None).await?;

    let insert_sql = format!(
        "INSERT INTO {} SELECT * FROM {}",
        qualified(database, &temp_name),
        qualified(database, table)
    );
    if let Err(cause) = execute_check(conn, &insert_sql, None).await {
        let _ = execute_check(
            conn,
            &format!("DROP TABLE {}", qualified(database, &temp_name)),
            None,
        )
        .await;
        return Err(cause);
    }

    let old_count = row_count(conn, database, table).await?;
    let new_count = row_count(conn, database, &temp_name).await?;
    if old_count != new_count {
        let _ = execute_check(
            conn,
            &format!("DROP TABLE {}", qualified(database, &temp_name)),
            None,
        )
        .await;
        return Err(err!("error.clickhouseRebuildCountMismatch", table = table));
    }

    execute_check(
        conn,
        &format!(
            "EXCHANGE TABLES {} AND {}",
            qualified(database, table),
            qualified(database, &temp_name)
        ),
        None,
    )
    .await?;

    match execute_check(
        conn,
        &format!("DROP TABLE {}", qualified(database, &temp_name)),
        None,
    )
    .await
    {
        Ok(()) => Ok(None),
        Err(_) => Ok(Some(temp_name)),
    }
}

/// The statements that bring column `current` to what `spec` describes - none at all when nothing
/// differs.
///
/// The shape of `postgres::ddl::modify_column`: read what the column is now, write only what really
/// changed. The reason is stronger here - a `MODIFY COLUMN` that changes the type is a mutation
/// rewriting every part of the table, and the `ALTER` waits for it (2.7 s over 43 million rows,
/// measured against the test server) - so emitting one to save a comment charges the user for
/// something they did not ask for.
///
/// A rename and a redefinition go out as two statements, rename first. That is not a style:
/// ClickHouse refuses to do both to one column in a single `ALTER`
/// (`Code: 48 ... Cannot rename and modify the same column in a single ALTER query`). It follows
/// that there is no way to avoid the risk of the rename landing and the redefinition failing -
/// there is no transaction to hold them together.
///
/// `nullable` is not compared on its own: it lives inside `data_type` as `Nullable(T)`, so
/// comparing the type catches it too.
pub fn modify_column_statements(
    qualified_table: &str,
    current: &StructureColumn,
    spec: &ColumnSpec,
) -> Result<Vec<String>, AppError> {
    let new_name = spec.name.trim();
    if new_name.is_empty() {
        return Err(err!("error.columnNameRequired"));
    }
    // Built before it is known to be needed, so that an empty type is refused even when the user
    // only renamed the column: a column with no type is a broken draft whatever the operation is.
    let definition = column_definition(spec)?;

    let mut statements = Vec::new();
    if new_name != current.name {
        statements.push(format!(
            "ALTER TABLE {qualified_table} RENAME COLUMN {} TO {}",
            quote_ident(&current.name),
            quote_ident(new_name)
        ));
    }

    let same_default = spec.default_value.as_deref().map(str::trim)
        == current.default_value.as_deref().map(str::trim)
        && (spec.default_value.is_none()
            || spec.default_is_expression == current.default_is_expression);
    let unchanged = spec.data_type.trim() == current.data_type
        && spec.comment == current.comment
        && same_default;
    if !unchanged {
        statements.push(format!(
            "ALTER TABLE {qualified_table} MODIFY COLUMN {definition}"
        ));
    }
    Ok(statements)
}

/// Redefines the column currently called `name` - so this is also how a column is renamed.
///
/// Runs as ordinary synchronous DDL, **not** through `run_mutation_and_wait`: an
/// `ALTER ... MODIFY COLUMN` that changes the type does create a mutation, but the `ALTER` itself
/// waits for it before answering (`StorageMergeTree::alter` -> `waitForMutation`), checked against
/// the test server.
///
/// A `MODIFY COLUMN` that fails because the data will not convert leaves the table unreadable: the
/// metadata has already changed, the failed mutation stays behind, and every `SELECT` errors until
/// the type is put back. That is why the failure is wrapped rather than passed through - the user
/// needs to be told the way out, not only that something went wrong.
pub async fn modify_column(
    conn: &Connection,
    database: &str,
    table: &str,
    name: &str,
    spec: &ColumnSpec,
) -> Result<(), AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(err!("error.columnNameRequired"));
    }
    let current = structure_columns(conn, database, table)
        .await?
        .into_iter()
        .find(|column| column.name == name)
        .ok_or_else(|| err!("error.columnNameRequired"))?;

    let qualified_table = qualified(database, table);
    let type_changes = spec.data_type.trim() != current.data_type;
    for statement in modify_column_statements(&qualified_table, &current, spec)? {
        let changing_type = type_changes && statement.contains(" MODIFY COLUMN ");
        if let Err(cause) = execute_check(conn, &statement, None).await {
            return Err(if changing_type {
                err!(
                    "error.clickhouseTypeChangeFailed",
                    column = name,
                    table = table
                )
                .caused_by(cause)
            } else {
                cause
            });
        }
    }
    Ok(())
}

/// `ORDER BY (col1, col2)`, or `ORDER BY tuple()` for an empty key — the same spelling
/// `create_table_statement` already writes for a brand new table.
pub(super) fn order_by_clause(columns: &[String]) -> String {
    if columns.is_empty() {
        return "ORDER BY tuple()".to_string();
    }
    let quoted: Vec<String> = columns.iter().map(|c| quote_ident(c)).collect();
    format!("ORDER BY ({})", quoted.join(", "))
}

/// A name for the table the rebuild copies into that never collides with one a previous, failed run
/// left behind — the millisecond clock makes every call's name different from the last. See the
/// ClickHouse index DDL design doc's D7/D8.
pub(super) fn temp_table_name(table: &str) -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{table}__mixlab_rebuild_{millis}")
}

/// Turns `SHOW CREATE TABLE`'s own text into the `CREATE TABLE` for the rebuild copy: renamed to
/// `temp_name`, its `ORDER BY` line replaced. Everything else — `PARTITION BY`, `TTL`, `SETTINGS`,
/// `COMMENT`, any skip index inside the column list — is carried through untouched because it is
/// never looked at.
///
/// The first line is discarded and rebuilt rather than searched-and-replaced: ClickHouse only wraps
/// an identifier in backticks in its own output when the name actually needs it (a name with a space
/// gets them, a plain one does not — checked against the test server), so there is no one spelling
/// of the old `database.table` reference to search for. Rebuilding the line with `qualified`
/// sidesteps the question entirely rather than guessing which quoting the server chose.
pub(super) fn rebuild_ddl(
    show_create: &str,
    database: &str,
    temp_name: &str,
    order_by: &str,
) -> Result<String, AppError> {
    let mut lines = show_create.lines();
    lines
        .next()
        .ok_or_else(|| err!("error.clickhouseRebuildParse"))?;
    let mut found = false;
    let body: Vec<String> = lines
        .map(|line| {
            if line.starts_with("ORDER BY ") {
                found = true;
                order_by.to_string()
            } else {
                line.to_string()
            }
        })
        .collect();
    if !found {
        return Err(err!("error.clickhouseRebuildParse"));
    }
    Ok(format!(
        "CREATE TABLE {}\n{}",
        qualified(database, temp_name),
        body.join("\n")
    ))
}

/// What is decided here, rather than by a server's answer.
#[cfg(test)]
mod tests;
