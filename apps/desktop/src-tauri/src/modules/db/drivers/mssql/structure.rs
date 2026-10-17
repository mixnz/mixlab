//! What a SQL Server table is made of, and what every table in a database weighs — the Structure
//! and Statistics tabs, plus the Query tab's schema outline.
//!
//! The shapes reported here are the ones `postgres/structure.rs` reports, because one grid draws
//! either. Where SQL Server has nothing to put in a field it is left at its empty value rather than
//! dropped: `on_update_current_timestamp` is a MySQL clause with no counterpart here, and
//! `prefix_length` an index feature SQL Server does not have.

use super::{
    display_type, extra_tokens, is_rowversion_type, map_error, qualify, quote_ident,
    read_uncommitted, resolve, strip_default_parens, Pool, DEFAULT_SCHEMA,
};
use crate::error::AppError;
pub use crate::modules::db::drivers::sql::types::{
    Collation, IndexColumn, OutlineColumn, OutlineTable, SchemaOutline, StructureColumn,
    TableIndex, TableStats, TableStructure,
};
use std::collections::HashMap;

/// Which kind of key a column leads, as MySQL's `Key` column spells it.
///
/// Only the **leading** column of an index is marked, which is what the callers pass: a column in
/// the middle of a composite index is not usable as a lookup on its own, and marking it would
/// promise the grid something the server will not do.
pub(super) fn key_marker(is_primary: bool, is_unique: bool, is_indexed: bool) -> &'static str {
    if is_primary {
        "PRI"
    } else if is_unique {
        "UNI"
    } else if is_indexed {
        "MUL"
    } else {
        ""
    }
}

/// Whether a default is an expression rather than a literal.
///
/// SQL Server stores both as text in `sys.default_constraints.definition`, so the two are told
/// apart by shape once the wrapping parentheses are off: a quoted string or a number is a literal,
/// and anything else — `getdate()`, `newid()`, `next value for s` — is an expression. The mark is
/// what the column grid puts beside a default so `newid()` and the text `newid()` do not read
/// alike.
fn is_expression_default(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }
    // A quoted string is a literal, `N'...'` — the Unicode spelling — included.
    if trimmed.starts_with('\'') || trimmed.starts_with("N'") {
        return false;
    }
    // So is a number, with or without a sign or a decimal point.
    let digits = trimmed.trim_start_matches(['-', '+']);
    !(!digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit() || c == '.'))
}

/// Everything the Structure tab shows about one table: its columns in table order, and its indexes
/// with the primary key first.
pub async fn table_structure(
    pool: &Pool,
    database: &str,
    table: &str,
) -> Result<TableStructure, AppError> {
    let (schema, name) = resolve(table);
    Ok(TableStructure {
        columns: structure_columns(pool, database, &schema, &name).await?,
        indexes: table_indexes(pool, database, &schema, &name).await?,
        skip_indexes: Vec::new(),
        engine: None,
    })
}

/// The columns of one table with everything the Structure tab shows about them.
///
/// A second read rather than `mssql::table_columns` reused: the grid needs three things that one
/// does not carry — the collation, the description, and which kind of key the column leads — and
/// asking for them in the same statement is one round trip instead of two.
///
/// The description is `sys.extended_properties`' `MS_Description`, which is where SSMS puts what it
/// calls a column's Description and the nearest thing SQL Server has to MySQL's column comment.
async fn structure_columns(
    pool: &Pool,
    database: &str,
    schema: &str,
    table: &str,
) -> Result<Vec<StructureColumn>, AppError> {
    let db = quote_ident(database);
    let sql = read_uncommitted(&format!(
        "SELECT c.name, t.name AS type_name, c.max_length, c.precision, c.scale,
                c.is_nullable, c.is_identity, c.is_computed, c.collation_name,
                d.definition,
                COALESCE(CAST(ep.value AS nvarchar(max)), '') AS comment,
                COALESCE(k.is_primary, 0) AS is_primary,
                COALESCE(k.is_unique, 0) AS is_unique,
                COALESCE(k.is_indexed, 0) AS is_indexed
         FROM {db}.sys.columns c
         JOIN {db}.sys.objects o ON o.object_id = c.object_id
         JOIN {db}.sys.schemas s ON s.schema_id = o.schema_id
         JOIN {db}.sys.types t ON t.user_type_id = c.user_type_id
         LEFT JOIN {db}.sys.default_constraints d ON d.object_id = c.default_object_id
         LEFT JOIN {db}.sys.extended_properties ep
             ON ep.major_id = c.object_id AND ep.minor_id = c.column_id
             AND ep.class = 1 AND ep.name = 'MS_Description'
         OUTER APPLY (
             SELECT MAX(CASE WHEN i.is_primary_key = 1 THEN 1 ELSE 0 END) AS is_primary,
                    MAX(CASE WHEN i.is_unique = 1 THEN 1 ELSE 0 END) AS is_unique,
                    MAX(1) AS is_indexed
             FROM {db}.sys.indexes i
             JOIN {db}.sys.index_columns ic
                 ON ic.object_id = i.object_id AND ic.index_id = i.index_id
             WHERE i.object_id = c.object_id AND ic.column_id = c.column_id
               AND ic.key_ordinal = 1
         ) k
         WHERE s.name = @P1 AND o.name = @P2
         ORDER BY c.column_id"
    ));

    let mut client = pool
        .get()
        .await
        .map_err(|e| err!("error.mssql", message = e))?;
    let rows = client
        .query(sql, &[&schema, &table])
        .await
        .map_err(map_error)?
        .into_first_result()
        .await
        .map_err(map_error)?;

    Ok(rows
        .iter()
        .filter_map(|row| {
            let name: &str = row.get("name")?;
            let type_name: &str = row.get("type_name")?;
            let default_value = row.get::<&str, _>("definition").map(strip_default_parens);
            let is_identity = row.get("is_identity").unwrap_or(false);
            let is_computed = row.get("is_computed").unwrap_or(false);
            Some(StructureColumn {
                name: name.to_string(),
                data_type: display_type(
                    type_name,
                    row.get("max_length").unwrap_or(0),
                    row.get("precision").unwrap_or(0),
                    row.get("scale").unwrap_or(0),
                ),
                nullable: row.get("is_nullable").unwrap_or(true),
                default_is_expression: default_value
                    .as_deref()
                    .map(is_expression_default)
                    .unwrap_or(false),
                default_value,
                auto_increment: is_identity,
                on_update_current_timestamp: false,
                generated: is_computed,
                collation: row.get::<&str, _>("collation_name").map(str::to_string),
                comment: row.get::<&str, _>("comment").unwrap_or("").to_string(),
                key: key_marker(
                    row.get::<i32, _>("is_primary").unwrap_or(0) == 1,
                    row.get::<i32, _>("is_unique").unwrap_or(0) == 1,
                    row.get::<i32, _>("is_indexed").unwrap_or(0) == 1,
                )
                .to_string(),
                extra: extra_tokens(is_identity, is_computed, is_rowversion_type(type_name)),
            })
        })
        .collect())
}

/// The table's indexes, the primary key first.
///
/// Only key columns are listed, in key order. An index's *included* columns (`INCLUDE (a, b)`) are
/// left out: they are payload rather than key, nothing can be looked up by them, and the shared
/// grid has one list of columns to show. The heap — `index_id = 0`, which is what a table with no
/// clustered index has — is not an index and is filtered out.
async fn table_indexes(
    pool: &Pool,
    database: &str,
    schema: &str,
    table: &str,
) -> Result<Vec<TableIndex>, AppError> {
    let db = quote_ident(database);
    let sql = read_uncommitted(&format!(
        "SELECT i.name, i.is_unique, i.is_primary_key, LOWER(i.type_desc) AS index_type,
                c.name AS column_name, ic.key_ordinal
         FROM {db}.sys.indexes i
         JOIN {db}.sys.objects o ON o.object_id = i.object_id
         JOIN {db}.sys.schemas s ON s.schema_id = o.schema_id
         JOIN {db}.sys.index_columns ic
             ON ic.object_id = i.object_id AND ic.index_id = i.index_id
             AND ic.is_included_column = 0
         JOIN {db}.sys.columns c
             ON c.object_id = ic.object_id AND c.column_id = ic.column_id
         WHERE s.name = @P1 AND o.name = @P2 AND i.index_id > 0 AND i.is_hypothetical = 0
         ORDER BY i.is_primary_key DESC, i.name, ic.key_ordinal"
    ));

    let mut client = pool
        .get()
        .await
        .map_err(|e| err!("error.mssql", message = e))?;
    let rows = client
        .query(sql, &[&schema, &table])
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
                // SQL Server keeps no comment on an index. An extended property could hold one, but
                // nothing writes one and SSMS does not show one either.
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

/// The data size over the rows, and zero where there are no rows to divide by.
pub(super) fn average_record_size(data_size: u64, rows: u64) -> u64 {
    data_size.checked_div(rows).unwrap_or(0)
}

/// What every table in `database` weighs.
///
/// Views are left out, as they are on MySQL and PostgreSQL: a view stores nothing of its own and
/// would read here as an empty table.
///
/// `index_id IN (0, 1)` is the table's own data — 0 is a heap, 1 a clustered index, and a table has
/// one or the other, never both. Everything else is a nonclustered index, which is what makes the
/// two sums a split of the same rows rather than two reads. Pages are 8 KB, which is fixed for
/// every edition SQL Server has shipped.
///
/// Reading `sys.dm_db_partition_stats` needs `VIEW DATABASE STATE`, which a login with rights to
/// read the data usually has; without it this comes back as the server's own error rather than as
/// zeroes, and the Statistics tab shows it.
pub async fn table_stats(pool: &Pool, database: &str) -> Result<Vec<TableStats>, AppError> {
    let db = quote_ident(database);
    let sql = read_uncommitted(&format!(
        "SELECT s.name AS schema_name, t.name AS table_name,
                SUM(CASE WHEN p.index_id IN (0, 1) THEN p.row_count ELSE 0 END) AS row_count,
                SUM(CASE WHEN p.index_id IN (0, 1) THEN p.used_page_count ELSE 0 END) * 8192
                    AS data_size,
                SUM(CASE WHEN p.index_id NOT IN (0, 1) THEN p.used_page_count ELSE 0 END) * 8192
                    AS index_size
         FROM {db}.sys.dm_db_partition_stats p
         JOIN {db}.sys.tables t ON t.object_id = p.object_id
         JOIN {db}.sys.schemas s ON s.schema_id = t.schema_id
         GROUP BY s.name, t.name
         ORDER BY CASE WHEN s.name = '{DEFAULT_SCHEMA}' THEN 0 ELSE 1 END, s.name, t.name"
    ));

    let mut client = pool
        .get()
        .await
        .map_err(|e| err!("error.mssql", message = e))?;
    let rows = client
        .query(sql, &[])
        .await
        .map_err(map_error)?
        .into_first_result()
        .await
        .map_err(map_error)?;

    Ok(rows
        .iter()
        .filter_map(|row| {
            let schema: &str = row.get("schema_name")?;
            let table: &str = row.get("table_name")?;
            // Every one of these is a bigint on the wire; a negative can only come of a catalogue
            // mid-update, and reads here as the zero it is indistinguishable from.
            let rows_count = row.get::<i64, _>("row_count").unwrap_or(0).max(0) as u64;
            let data_size = row.get::<i64, _>("data_size").unwrap_or(0).max(0) as u64;
            let index_size = row.get::<i64, _>("index_size").unwrap_or(0).max(0) as u64;
            Some(TableStats {
                name: qualify(schema, table),
                rows: rows_count,
                data_size,
                index_size,
                avg_record_size: average_record_size(data_size, rows_count),
            })
        })
        .collect())
}

/// Every collation this server supports, the server's own marked.
///
/// `sys.fn_helpcollations()` is the list SQL Server offers about itself; it is long — a few thousand
/// rows — and is read once per connection by whoever opens a dialog that declares one.
pub async fn collations(pool: &Pool) -> Result<Vec<Collation>, AppError> {
    let mut client = pool
        .get()
        .await
        .map_err(|e| err!("error.mssql", message = e))?;
    let rows = client
        .query(
            "SELECT name, CAST(SERVERPROPERTY('Collation') AS nvarchar(128)) AS server_collation
             FROM sys.fn_helpcollations()
             ORDER BY name",
            &[],
        )
        .await
        .map_err(map_error)?
        .into_first_result()
        .await
        .map_err(map_error)?;

    Ok(rows
        .iter()
        .filter_map(|row| {
            let name: &str = row.get("name")?;
            let server: &str = row.get("server_collation").unwrap_or("");
            Some(Collation {
                name: name.to_string(),
                charset: String::new(),
                is_default: name.eq_ignore_ascii_case(server),
            })
        })
        .collect())
}

/// What the Query tab's completion knows about the connected database: every table and column of
/// it, across every schema the user can see, in two reads.
///
/// Views are in it as well as tables — their columns complete like any others', which is why this
/// reads `sys.objects` filtered to `U` and `V` rather than `sys.tables`, the same filter
/// `mssql::list_tables` uses so the two lists cannot drift apart.
///
/// The foreign keys are read first and separately. A failure there is not worth failing the whole
/// outline over: completion is for the columns, and where a key points is a line of detail beside
/// them.
pub async fn schema_outline(pool: &Pool, database: &str) -> Result<SchemaOutline, AppError> {
    let db = quote_ident(database);
    let mut client = pool
        .get()
        .await
        .map_err(|e| err!("error.mssql", message = e))?;

    let key_sql = read_uncommitted(&format!(
        "SELECT s.name AS schema_name, o.name AS table_name, c.name AS column_name,
                rs.name AS ref_schema, ro.name AS ref_table, rc.name AS ref_column
         FROM {db}.sys.foreign_key_columns fk
         JOIN {db}.sys.objects o ON o.object_id = fk.parent_object_id
         JOIN {db}.sys.schemas s ON s.schema_id = o.schema_id
         JOIN {db}.sys.columns c
             ON c.object_id = fk.parent_object_id AND c.column_id = fk.parent_column_id
         JOIN {db}.sys.objects ro ON ro.object_id = fk.referenced_object_id
         JOIN {db}.sys.schemas rs ON rs.schema_id = ro.schema_id
         JOIN {db}.sys.columns rc
             ON rc.object_id = fk.referenced_object_id
             AND rc.column_id = fk.referenced_column_id"
    ));
    let key_rows = match client.query(key_sql, &[]).await {
        Ok(stream) => stream.into_first_result().await.unwrap_or_default(),
        Err(_) => Vec::new(),
    };

    let mut references: HashMap<(String, String), String> = HashMap::new();
    for row in &key_rows {
        let (
            Some(schema),
            Some(table),
            Some(column),
            Some(ref_schema),
            Some(ref_table),
            Some(ref_column),
        ) = (
            row.get::<&str, _>("schema_name"),
            row.get::<&str, _>("table_name"),
            row.get::<&str, _>("column_name"),
            row.get::<&str, _>("ref_schema"),
            row.get::<&str, _>("ref_table"),
            row.get::<&str, _>("ref_column"),
        )
        else {
            continue;
        };
        references.insert(
            (qualify(schema, table), column.to_string()),
            format!("{}.{}", qualify(ref_schema, ref_table), ref_column),
        );
    }

    let column_sql = read_uncommitted(&format!(
        "SELECT s.name AS schema_name, o.name AS table_name, c.name AS column_name,
                t.name AS type_name, c.max_length, c.precision, c.scale, c.is_nullable,
                COALESCE(k.is_primary, 0) AS is_primary,
                COALESCE(k.is_unique, 0) AS is_unique,
                COALESCE(k.is_indexed, 0) AS is_indexed
         FROM {db}.sys.columns c
         JOIN {db}.sys.objects o ON o.object_id = c.object_id
         JOIN {db}.sys.schemas s ON s.schema_id = o.schema_id
         JOIN {db}.sys.types t ON t.user_type_id = c.user_type_id
         OUTER APPLY (
             SELECT MAX(CASE WHEN i.is_primary_key = 1 THEN 1 ELSE 0 END) AS is_primary,
                    MAX(CASE WHEN i.is_unique = 1 THEN 1 ELSE 0 END) AS is_unique,
                    MAX(1) AS is_indexed
             FROM {db}.sys.indexes i
             JOIN {db}.sys.index_columns ic
                 ON ic.object_id = i.object_id AND ic.index_id = i.index_id
             WHERE i.object_id = c.object_id AND ic.column_id = c.column_id
               AND ic.key_ordinal = 1
         ) k
         WHERE o.type IN ('U', 'V')
         ORDER BY CASE WHEN s.name = '{DEFAULT_SCHEMA}' THEN 0 ELSE 1 END,
                  s.name, o.name, c.column_id"
    ));
    let column_rows = client
        .query(column_sql, &[])
        .await
        .map_err(map_error)?
        .into_first_result()
        .await
        .map_err(map_error)?;

    let mut tables: Vec<OutlineTable> = Vec::new();
    for row in &column_rows {
        let (Some(schema), Some(table), Some(column), Some(type_name)) = (
            row.get::<&str, _>("schema_name"),
            row.get::<&str, _>("table_name"),
            row.get::<&str, _>("column_name"),
            row.get::<&str, _>("type_name"),
        ) else {
            continue;
        };
        let name = qualify(schema, table);
        if tables.last().map(|last| last.name != name).unwrap_or(true) {
            tables.push(OutlineTable {
                name: name.clone(),
                columns: Vec::new(),
            });
        }
        if let Some(last) = tables.last_mut() {
            last.columns.push(OutlineColumn {
                name: column.to_string(),
                data_type: display_type(
                    type_name,
                    row.get("max_length").unwrap_or(0),
                    row.get("precision").unwrap_or(0),
                    row.get("scale").unwrap_or(0),
                ),
                nullable: row.get("is_nullable").unwrap_or(true),
                key: key_marker(
                    row.get::<i32, _>("is_primary").unwrap_or(0) == 1,
                    row.get::<i32, _>("is_unique").unwrap_or(0) == 1,
                    row.get::<i32, _>("is_indexed").unwrap_or(0) == 1,
                )
                .to_string(),
                references: references.remove(&(name.clone(), column.to_string())),
            });
        }
    }

    Ok(SchemaOutline {
        database: database.to_string(),
        tables,
    })
}

#[cfg(test)]
mod tests {
    use super::{average_record_size, is_expression_default, key_marker};

    /// The three markers in the order they win: a primary key column is `PRI` even though it is
    /// also unique and also indexed, which is what keeps one column from carrying two claims.
    #[test]
    fn the_strongest_key_a_column_leads_is_the_one_marked() {
        assert_eq!(key_marker(true, true, true), "PRI");
        assert_eq!(key_marker(false, true, true), "UNI");
        assert_eq!(key_marker(false, false, true), "MUL");
        assert_eq!(key_marker(false, false, false), "");
    }

    /// SQL Server stores a literal default and an expression one the same way, so the grid's mark
    /// is read off the shape of the text — a quoted string or a number is a literal, a call is not.
    #[test]
    fn a_call_is_an_expression_and_a_written_out_value_is_not() {
        assert!(is_expression_default("getdate()"));
        assert!(is_expression_default("newid()"));
        assert!(!is_expression_default("0"));
        assert!(!is_expression_default("-1.5"));
        assert!(!is_expression_default("'new'"));
        assert!(!is_expression_default("N'mới'"));
        assert!(!is_expression_default(""));
    }

    /// SQL Server keeps no average row width, so it is derived — and an empty table has to divide
    /// by nothing rather than panic.
    #[test]
    fn an_empty_table_has_no_average_row_to_divide_by() {
        assert_eq!(average_record_size(8192, 100), 81);
        assert_eq!(average_record_size(8192, 0), 0);
        assert_eq!(average_record_size(0, 10), 0);
    }
}
