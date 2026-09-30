//! Creating and changing schema on ClickHouse: databases, tables, columns.
//!
//! A module of its own rather than more of `clickhouse.rs`, the same way `postgres_ddl.rs` sits
//! beside `postgres.rs`: that file is what reads, and DDL is a large enough job to stand apart.
//!
//! Every statement is built by a pure function and only then sent, so what to *write* is decided
//! where a test can read it and sending is left as `execute_check`. There is no transaction here —
//! ClickHouse has none — so a run of statements that fails partway leaves exactly what already ran.

use super::clickhouse::{
    as_u64, execute_check, qualified, query, quote_ident, quote_literal, structure_columns,
    Connection, StructureColumn,
};
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
/// `mysql_structure::validated_collation` takes for a collation.
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
/// `clickhouse::StructureColumn`.
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
/// `clickhouse::SkipIndex`. See the ClickHouse index DDL design doc's D2/D3.
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
/// The shape of `postgres_ddl::modify_column`: read what the column is now, write only what really
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
mod tests {
    use super::*;

    fn skip_spec(
        name: &str,
        expr: &str,
        index_type: &str,
        args: &[&str],
        granularity: u64,
    ) -> SkipIndexSpec {
        SkipIndexSpec {
            name: name.to_string(),
            expr: expr.to_string(),
            index_type: index_type.to_string(),
            args: args.iter().map(|a| a.to_string()).collect(),
            granularity,
        }
    }

    #[test]
    fn a_type_with_no_arguments_is_written_bare() {
        assert_eq!(
            add_skip_index_statement(
                "shop",
                "orders",
                &skip_spec("ix1", "total", "minmax", &[], 1)
            )
            .unwrap(),
            "ALTER TABLE `shop`.`orders` ADD INDEX `ix1` total TYPE minmax GRANULARITY 1"
        );
    }

    #[test]
    fn a_type_with_arguments_writes_them_in_order() {
        assert_eq!(
            add_skip_index_statement(
                "shop",
                "orders",
                &skip_spec("ix1", "note", "ngrambf_v1", &["3", "256", "2", "0"], 4)
            )
            .unwrap(),
            "ALTER TABLE `shop`.`orders` ADD INDEX `ix1` note TYPE ngrambf_v1(3, 256, 2, 0) GRANULARITY 4"
        );
    }

    #[test]
    fn an_index_needs_a_name() {
        assert_eq!(
            add_skip_index_statement(
                "shop",
                "orders",
                &skip_spec("  ", "total", "minmax", &[], 1)
            )
            .unwrap_err(),
            err!("error.indexNameRequired")
        );
    }

    #[test]
    fn an_index_needs_an_expression() {
        assert_eq!(
            add_skip_index_statement("shop", "orders", &skip_spec("ix1", "  ", "minmax", &[], 1))
                .unwrap_err(),
            err!("error.clickhouseSkipIndexExprRequired")
        );
    }

    #[test]
    fn an_empty_key_writes_tuple() {
        assert_eq!(order_by_clause(&[]), "ORDER BY tuple()");
    }

    #[test]
    fn columns_are_quoted_and_kept_in_order() {
        assert_eq!(
            order_by_clause(&["b".to_string(), "a".to_string()]),
            "ORDER BY (`b`, `a`)"
        );
    }

    #[test]
    fn the_temp_name_carries_the_original_and_a_timestamp() {
        let name = temp_table_name("orders");
        let suffix = name.strip_prefix("orders__mixlab_rebuild_").expect(&name);
        assert!(suffix.parse::<u128>().is_ok(), "{suffix}");
    }

    /// The exact text verified against the test server for a table with every optional clause at
    /// once — `PARTITION BY`, `TTL`, `SETTINGS`, `COMMENT`, and a skip index sitting inside the
    /// column list.
    const SHOW_CREATE_WITH_EVERY_CLAUSE: &str = "CREATE TABLE shop.orders\n(\n    `a` UInt64,\n    `b` UInt64,\n    `c` String,\n    INDEX ix1 c TYPE minmax GRANULARITY 1\n)\nENGINE = MergeTree\nPARTITION BY toYYYYMM(ts)\nORDER BY (a, b)\nTTL ts + toIntervalDay(30)\nSETTINGS index_granularity = 4096\nCOMMENT 'test comment'";

    #[test]
    fn rebuild_renames_the_table_and_replaces_only_the_order_by_line() {
        let rebuilt = rebuild_ddl(
            SHOW_CREATE_WITH_EVERY_CLAUSE,
            "shop",
            "orders__mixlab_rebuild_1",
            "ORDER BY (c)",
        )
        .unwrap();
        assert_eq!(
            rebuilt,
            "CREATE TABLE `shop`.`orders__mixlab_rebuild_1`\n(\n    `a` UInt64,\n    `b` UInt64,\n    `c` String,\n    INDEX ix1 c TYPE minmax GRANULARITY 1\n)\nENGINE = MergeTree\nPARTITION BY toYYYYMM(ts)\nORDER BY (c)\nTTL ts + toIntervalDay(30)\nSETTINGS index_granularity = 4096\nCOMMENT 'test comment'"
        );
    }

    #[test]
    fn rebuild_handles_an_empty_sorting_key() {
        let show_create = "CREATE TABLE shop.orders\n(\n    `id` UInt64\n)\nENGINE = MergeTree\nORDER BY tuple()\nSETTINGS index_granularity = 8192";
        let rebuilt = rebuild_ddl(
            show_create,
            "shop",
            "orders__mixlab_rebuild_2",
            "ORDER BY (id)",
        )
        .unwrap();
        assert_eq!(
            rebuilt,
            "CREATE TABLE `shop`.`orders__mixlab_rebuild_2`\n(\n    `id` UInt64\n)\nENGINE = MergeTree\nORDER BY (id)\nSETTINGS index_granularity = 8192"
        );
    }

    #[test]
    fn rebuild_refuses_text_with_no_order_by_line_to_replace() {
        assert_eq!(
            rebuild_ddl(
                "CREATE TABLE shop.orders\n(\n    `id` UInt64\n)\nENGINE = Memory",
                "shop",
                "t",
                "ORDER BY (id)"
            )
            .unwrap_err(),
            err!("error.clickhouseRebuildParse")
        );
    }

    #[test]
    fn modifying_sends_the_drop_before_the_add() {
        assert_eq!(
            modify_skip_index_statements(
                "shop",
                "orders",
                "ix1",
                &skip_spec("ix1", "total", "set", &["100"], 2)
            )
            .unwrap(),
            vec![
                "ALTER TABLE `shop`.`orders` DROP INDEX `ix1`",
                "ALTER TABLE `shop`.`orders` ADD INDEX `ix1` total TYPE set(100) GRANULARITY 2",
            ]
        );
    }

    #[test]
    fn a_quoted_string_is_a_literal_and_loses_its_quotes() {
        assert_eq!(
            read_default("'active'"),
            Some(("active".to_string(), false))
        );
    }

    #[test]
    fn a_function_call_is_an_expression_kept_verbatim() {
        assert_eq!(read_default("now()"), Some(("now()".to_string(), true)));
    }

    #[test]
    fn a_number_is_an_expression_there_being_nothing_to_tell_it_from_one() {
        assert_eq!(read_default("42"), Some(("42".to_string(), true)));
    }

    #[test]
    fn nothing_at_all_is_no_default() {
        assert_eq!(read_default(""), None);
        assert_eq!(read_default("   "), None);
    }

    #[test]
    fn a_concatenation_that_merely_starts_and_ends_with_a_quote_is_not_a_literal() {
        assert_eq!(
            read_default("'a' || 'b'"),
            Some(("'a' || 'b'".to_string(), true))
        );
    }

    #[test]
    fn an_escaped_quote_inside_a_literal_is_unescaped() {
        assert_eq!(read_default("'it\\'s'"), Some(("it's".to_string(), false)));
    }

    #[test]
    fn a_new_table_gets_one_placeholder_column_and_no_sorting_key() {
        assert_eq!(
            create_table_statement("shop", "orders", "MergeTree").unwrap(),
            "CREATE TABLE `shop`.`orders` (`id` UInt64) ENGINE = MergeTree ORDER BY tuple()"
        );
    }

    #[test]
    fn an_engine_outside_the_list_is_refused_rather_than_interpolated() {
        assert_eq!(
            create_table_statement("shop", "orders", "Kafka").unwrap_err(),
            err!("error.clickhouseUnknownEngine", engine = "Kafka")
        );
        assert_eq!(
            create_table_statement("shop", "orders", "MergeTree ORDER BY x; DROP TABLE y")
                .unwrap_err()
                .code,
            "error.clickhouseUnknownEngine"
        );
    }

    #[test]
    fn a_table_with_no_name_is_refused_before_any_sql_is_built() {
        assert_eq!(
            create_table_statement("shop", "  ", "MergeTree").unwrap_err(),
            err!("error.tableNameRequired")
        );
    }

    fn spec(name: &str, data_type: &str) -> ColumnSpec {
        ColumnSpec {
            name: name.to_string(),
            data_type: data_type.to_string(),
            default_value: None,
            default_is_expression: false,
            comment: String::new(),
        }
    }

    #[test]
    fn an_escaped_quote_survives_the_default_round_trip() {
        let (value, is_expression) = read_default("'it\'s'").unwrap();
        assert_eq!(default_clause(&value, is_expression), " DEFAULT 'it\'s'");
    }

    #[test]
    fn an_expression_default_goes_back_out_unquoted() {
        assert_eq!(default_clause("now()", true), " DEFAULT now()");
    }

    #[test]
    fn a_plain_column_is_just_a_name_and_a_type() {
        assert_eq!(
            column_definition(&spec("title", "String")).unwrap(),
            "`title` String"
        );
    }

    #[test]
    fn nullability_travels_inside_the_type_rather_than_as_a_clause_of_its_own() {
        assert_eq!(
            column_definition(&spec("title", "Nullable(String)")).unwrap(),
            "`title` Nullable(String)"
        );
    }

    #[test]
    fn a_literal_default_is_quoted_and_an_expression_is_not() {
        let mut literal = spec("state", "String");
        literal.default_value = Some("active".to_string());
        assert_eq!(
            column_definition(&literal).unwrap(),
            "`state` String DEFAULT 'active'"
        );

        let mut expression = spec("made", "DateTime");
        expression.default_value = Some("now()".to_string());
        expression.default_is_expression = true;
        assert_eq!(
            column_definition(&expression).unwrap(),
            "`made` DateTime DEFAULT now()"
        );
    }

    #[test]
    fn a_comment_is_written_only_when_there_is_one() {
        let mut commented = spec("title", "String");
        commented.comment = "the heading".to_string();
        assert_eq!(
            column_definition(&commented).unwrap(),
            "`title` String COMMENT 'the heading'"
        );
    }

    #[test]
    fn a_column_needs_both_a_name_and_a_type() {
        assert_eq!(
            column_definition(&spec("  ", "String")).unwrap_err(),
            err!("error.columnNameRequired")
        );
        assert_eq!(
            column_definition(&spec("title", " ")).unwrap_err(),
            err!("error.columnTypeRequired")
        );
    }

    fn current(name: &str, data_type: &str) -> StructureColumn {
        StructureColumn {
            name: name.to_string(),
            data_type: data_type.to_string(),
            nullable: data_type.starts_with("Nullable("),
            default_value: None,
            default_is_expression: false,
            auto_increment: false,
            on_update_current_timestamp: false,
            generated: false,
            collation: None,
            comment: String::new(),
            key: String::new(),
            extra: String::new(),
        }
    }

    #[test]
    fn a_column_saved_without_being_touched_sends_nothing_at_all() {
        let statements = modify_column_statements(
            "`s`.`t`",
            &current("title", "String"),
            &spec("title", "String"),
        )
        .unwrap();
        assert!(statements.is_empty(), "{statements:?}");
    }

    #[test]
    fn renaming_only_sends_only_a_rename() {
        assert_eq!(
            modify_column_statements(
                "`s`.`t`",
                &current("title", "String"),
                &spec("heading", "String")
            )
            .unwrap(),
            vec!["ALTER TABLE `s`.`t` RENAME COLUMN `title` TO `heading`"]
        );
    }

    #[test]
    fn changing_the_comment_only_leaves_the_type_alone() {
        let mut wanted = spec("title", "String");
        wanted.comment = "the heading".to_string();
        assert_eq!(
            modify_column_statements("`s`.`t`", &current("title", "String"), &wanted).unwrap(),
            vec!["ALTER TABLE `s`.`t` MODIFY COLUMN `title` String COMMENT 'the heading'"]
        );
    }

    #[test]
    fn a_rename_and_a_redefinition_are_two_statements_with_the_rename_first() {
        assert_eq!(
            modify_column_statements(
                "`s`.`t`",
                &current("title", "String"),
                &spec("heading", "Nullable(String)")
            )
            .unwrap(),
            vec![
                "ALTER TABLE `s`.`t` RENAME COLUMN `title` TO `heading`",
                "ALTER TABLE `s`.`t` MODIFY COLUMN `heading` Nullable(String)",
            ]
        );
    }

    #[test]
    fn dropping_a_default_is_a_change_like_any_other() {
        let mut had_one = current("state", "String");
        had_one.default_value = Some("active".to_string());
        assert_eq!(
            modify_column_statements("`s`.`t`", &had_one, &spec("state", "String")).unwrap(),
            vec!["ALTER TABLE `s`.`t` MODIFY COLUMN `state` String"]
        );
    }
}
