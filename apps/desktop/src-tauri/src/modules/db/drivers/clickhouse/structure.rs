//! What a ClickHouse table is made of, and what every table in a database weighs — the
//! Structure and Statistics tabs, plus the Query tab's schema outline. The shapes are the
//! ones `postgres/structure.rs` reports, because one grid draws either.

use super::{query_with_params, Connection};
use crate::error::AppError;
pub use crate::modules::db::drivers::sql::types::{
    IndexColumn, OutlineColumn, OutlineTable, SchemaOutline, StructureColumn, TableIndex,
    TableStats, TableStructure,
};
use serde::Serialize;
use serde_json::Value;

/// One data skipping index — ClickHouse's only secondary index, an approximate part-skipping filter
/// rather than a lookup structure. See the ClickHouse index DDL design doc's D2.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkipIndex {
    pub name: String,
    pub expr: String,
    pub index_type: String,
    pub args: Vec<String>,
    pub granularity: u64,
}

/// Splits `system.data_skipping_indices.type_full` (e.g. `"ngrambf_v1(3, 256, 2, 0)"`) into the bare
/// type name and its arguments, in order. No type any skip index uses nests parentheses the way
/// `Decimal(10, 2)` does inside a column type, so a first-`(` split is enough — no need for
/// `ColumnDialog`'s more careful nested-parens handling.
pub(super) fn parse_type_full(type_full: &str) -> (String, Vec<String>) {
    let type_full = type_full.trim();
    match type_full.find('(') {
        None => (type_full.to_string(), Vec::new()),
        Some(open) => {
            let name = type_full[..open].to_string();
            let close = type_full.rfind(')').unwrap_or(type_full.len());
            let inside = type_full[open + 1..close].trim();
            if inside.is_empty() {
                (name, Vec::new())
            } else {
                (
                    name,
                    inside.split(',').map(|a| a.trim().to_string()).collect(),
                )
            }
        }
    }
}

/// Everything the Structure tab shows about one table.
///
/// `indexes` carries at most one entry — the sorting key, shown as an index it is not, the same
/// way SQLite's rowid is. `skip_indexes` is ClickHouse's real secondary index, read from
/// `system.data_skipping_indices` — see the ClickHouse index DDL design doc's D2.
pub async fn table_structure(
    conn: &Connection,
    database: &str,
    table: &str,
) -> Result<TableStructure, AppError> {
    let columns = structure_columns(conn, database, table).await?;
    let key_columns: Vec<String> = columns
        .iter()
        .filter(|c| c.key == "PRI")
        .map(|c| c.name.clone())
        .collect();
    let indexes = if key_columns.is_empty() {
        Vec::new()
    } else {
        vec![TableIndex {
            name: "sorting_key".to_string(),
            unique: false,
            primary: true,
            index_type: "sorting_key".to_string(),
            columns: key_columns
                .into_iter()
                .map(|name| IndexColumn {
                    name: Some(name),
                    prefix_length: None,
                })
                .collect(),
            comment: String::new(),
        }]
    };
    let skip_indexes = structure_skip_indexes(conn, database, table).await?;
    let engine = table_engine(conn, database, table).await?;
    Ok(TableStructure {
        columns,
        indexes,
        skip_indexes,
        engine,
    })
}

/// `system.data_skipping_indices` for one table — ClickHouse's real secondary index, distinct from
/// the synthetic `sorting_key` row above.
async fn structure_skip_indexes(
    conn: &Connection,
    database: &str,
    table: &str,
) -> Result<Vec<SkipIndex>, AppError> {
    let result = query_with_params(
        conn,
        "SELECT name, type_full, expr, granularity FROM system.data_skipping_indices \
         WHERE database = {database:String} AND table = {table:String}",
        &[
            ("database".to_string(), database.to_string()),
            ("table".to_string(), table.to_string()),
        ],
    )
    .await?;

    Ok(result
        .data
        .iter()
        .filter_map(|row| {
            let name = row.get("name")?.as_str()?.to_string();
            let type_full = row.get("type_full")?.as_str()?.to_string();
            let expr = row
                .get("expr")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let granularity = as_u64(row.get("granularity")).unwrap_or(1);
            let (index_type, args) = parse_type_full(&type_full);
            Some(SkipIndex {
                name,
                expr,
                index_type,
                args,
                granularity,
            })
        })
        .collect())
}

/// `system.tables.engine` for one table — used to guard the sorting-key rebuild (D11): the frontend
/// disables it outside the four engines `ddl::ENGINES` already whitelists for creation.
async fn table_engine(
    conn: &Connection,
    database: &str,
    table: &str,
) -> Result<Option<String>, AppError> {
    let result = query_with_params(
        conn,
        "SELECT engine FROM system.tables WHERE database = {database:String} AND name = {table:String}",
        &[
            ("database".to_string(), database.to_string()),
            ("table".to_string(), table.to_string()),
        ],
    )
    .await?;
    Ok(result
        .data
        .first()
        .and_then(|row| row.get("engine")?.as_str().map(str::to_string)))
}

pub(super) async fn structure_columns(
    conn: &Connection,
    database: &str,
    table: &str,
) -> Result<Vec<StructureColumn>, AppError> {
    let result = query_with_params(
        conn,
        "SELECT name, type, default_kind, default_expression, comment, is_in_primary_key \
         FROM system.columns \
         WHERE database = {database:String} AND table = {table:String} \
         ORDER BY position",
        &[
            ("database".to_string(), database.to_string()),
            ("table".to_string(), table.to_string()),
        ],
    )
    .await?;

    Ok(result
        .data
        .iter()
        .filter_map(|row| {
            let name = row.get("name")?.as_str()?.to_string();
            let data_type = row.get("type")?.as_str()?.to_string();
            let default_kind = row
                .get("default_kind")
                .and_then(Value::as_str)
                .unwrap_or("");
            let default_expression = row
                .get("default_expression")
                .and_then(Value::as_str)
                .unwrap_or("");
            // `'active'` and `now()` are different things, and `system.columns` tells them apart
            // by the quotes — see `ddl::read_default`. Without splitting them here every
            // default would be marked an expression, and a literal would be quoted a second time on
            // its way back out.
            let default = super::ddl::read_default(default_expression);
            let is_primary = row
                .get("is_in_primary_key")
                .and_then(truthy)
                .unwrap_or(false);
            Some(StructureColumn {
                nullable: data_type.starts_with("Nullable("),
                data_type,
                default_value: default.as_ref().map(|(value, _)| value.clone()),
                default_is_expression: default.as_ref().is_some_and(|(_, expr)| *expr),
                auto_increment: false,
                on_update_current_timestamp: false,
                // MATERIALIZED and ALIAS columns are computed from the others, the closest
                // ClickHouse comes to a MySQL/PostgreSQL generated column.
                generated: default_kind == "MATERIALIZED" || default_kind == "ALIAS",
                collation: None,
                comment: row
                    .get("comment")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                key: if is_primary {
                    "PRI".to_string()
                } else {
                    String::new()
                },
                extra: default_kind.to_string(),
                name,
            })
        })
        .collect())
}

/// `FORMAT JSON` writes ClickHouse's `UInt8` booleans as either `0`/`1` or `true`/`false`
/// depending on how the column arrived (a plain `UInt8` reads as a number; the handful of
/// genuinely `Bool`-typed system columns read as JSON booleans) — this reads either.
fn truthy(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => n.as_i64().map(|n| n != 0),
        Value::String(s) => Some(s == "1" || s.eq_ignore_ascii_case("true")),
        _ => None,
    }
}

/// What every table in `database` weighs. `total_rows`/`total_bytes` are `NULL` for an engine that
/// keeps no such count — a `Log` table, a `View` — which reads here as zero rather than as a
/// missing table.
pub async fn table_stats(conn: &Connection, database: &str) -> Result<Vec<TableStats>, AppError> {
    let result = query_with_params(
        conn,
        "SELECT name, total_rows, total_bytes FROM system.tables \
         WHERE database = {database:String} ORDER BY name",
        &[("database".to_string(), database.to_string())],
    )
    .await?;

    Ok(result
        .data
        .iter()
        .filter_map(|row| {
            let name = row.get("name")?.as_str()?.to_string();
            let rows = as_u64(row.get("total_rows")).unwrap_or(0);
            let data_size = as_u64(row.get("total_bytes")).unwrap_or(0);
            Some(TableStats {
                name,
                rows,
                data_size,
                index_size: 0,
                avg_record_size: data_size.checked_div(rows).unwrap_or(0),
            })
        })
        .collect())
}

/// A number `FORMAT JSON` may have written as a string — see `scalar`'s own note on 64-bit
/// integers — or left out as JSON `null`, which is what a `NULL` becomes.
pub(super) fn as_u64(value: Option<&Value>) -> Option<u64> {
    match value {
        Some(Value::String(s)) => s.parse().ok(),
        Some(Value::Number(n)) => n.as_u64(),
        _ => None,
    }
}

/// What the Query tab's completion knows about `database`: every table and column of it, in one
/// read.
pub async fn schema_outline(conn: &Connection, database: &str) -> Result<SchemaOutline, AppError> {
    let result = query_with_params(
        conn,
        "SELECT table, name, type, is_in_primary_key FROM system.columns \
         WHERE database = {database:String} ORDER BY table, position",
        &[("database".to_string(), database.to_string())],
    )
    .await?;

    let mut tables: Vec<OutlineTable> = Vec::new();
    for row in &result.data {
        let (Some(table), Some(name), Some(data_type)) = (
            row.get("table").and_then(Value::as_str),
            row.get("name").and_then(Value::as_str),
            row.get("type").and_then(Value::as_str),
        ) else {
            continue;
        };
        let is_primary = row
            .get("is_in_primary_key")
            .and_then(truthy)
            .unwrap_or(false);
        let column = OutlineColumn {
            name: name.to_string(),
            nullable: data_type.starts_with("Nullable("),
            data_type: data_type.to_string(),
            key: if is_primary {
                "PRI".to_string()
            } else {
                String::new()
            },
            references: None,
        };
        match tables.last_mut() {
            Some(last) if last.name == table => last.columns.push(column),
            _ => tables.push(OutlineTable {
                name: table.to_string(),
                columns: vec![column],
            }),
        }
    }
    Ok(SchemaOutline {
        database: database.to_string(),
        tables,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_type_with_no_arguments_parses_to_an_empty_list() {
        assert_eq!(
            parse_type_full("minmax"),
            ("minmax".to_string(), Vec::new())
        );
    }

    #[test]
    fn a_single_argument_type_parses_its_one_value() {
        assert_eq!(
            parse_type_full("set(100)"),
            ("set".to_string(), vec!["100".to_string()])
        );
    }

    #[test]
    fn a_four_argument_type_parses_all_four_in_order() {
        assert_eq!(
            parse_type_full("ngrambf_v1(3, 256, 2, 0)"),
            (
                "ngrambf_v1".to_string(),
                vec!["3", "256", "2", "0"]
                    .into_iter()
                    .map(String::from)
                    .collect()
            )
        );
    }

    #[test]
    fn empty_parentheses_parse_to_an_empty_list_not_one_empty_string() {
        assert_eq!(
            parse_type_full("bloom_filter()"),
            ("bloom_filter".to_string(), Vec::new())
        );
    }
}
