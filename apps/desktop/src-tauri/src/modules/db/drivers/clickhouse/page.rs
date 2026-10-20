//! One page of a table for the grid: the filters' `WHERE` clause, and the page it cuts.

use super::{
    is_decodable, qualified, query_with_params, quote_ident, table_columns, ColumnMeta, Connection,
    Filter, PageQuery, TablePage,
};
use crate::error::AppError;
use crate::modules::db::drivers::filters::{escape_like, split_list};
use std::collections::BTreeMap;

/// Turns the filter rows into a `WHERE` clause and the parameters to bind into it, in
/// `{name:Type}` form — see [`query_with_params`].
///
/// Every comparison goes through `toString()` on the column, the value bound as `String`: the
/// frontend sends every filter value as text, and ClickHouse — unlike MySQL — does not coerce a
/// `String` parameter to compare against a `UInt64` column on its own. The ordering operators are
/// the exception, the same way they are on PostgreSQL: `toString()` on a number sorts `10` before
/// `9`, so there the column is left as it is and the parameter is typed to match instead.
pub(in crate::modules::db::drivers) fn build_where(
    filters: &[Filter],
    columns: &BTreeMap<String, String>,
) -> Result<(String, Vec<(String, String)>), AppError> {
    let mut clauses: Vec<String> = Vec::new();
    let mut params: Vec<(String, String)> = Vec::new();
    let mut next = 0usize;
    let mut placeholder = |params: &mut Vec<(String, String)>, ty: &str, value: String| {
        let name = format!("p{next}");
        next += 1;
        params.push((name.clone(), value));
        format!("{{{name}:{ty}}}")
    };

    for filter in filters {
        let Some(base_type) = columns.get(&filter.column) else {
            return Err(err!("error.unknownFilterColumn", column = &filter.column));
        };
        let col = format!("toString({})", quote_ident(&filter.column));
        let raw = quote_ident(&filter.column);
        let value = filter.value.as_deref().unwrap_or("");
        let operator = filter.operator.as_str();

        let clause = match operator {
            "eq" => format!(
                "{col} = {}",
                placeholder(&mut params, "String", value.to_string())
            ),
            "ne" => format!(
                "{col} <> {}",
                placeholder(&mut params, "String", value.to_string())
            ),
            "gt" => format!(
                "{raw} > {}",
                placeholder(&mut params, base_type, value.to_string())
            ),
            "gte" => format!(
                "{raw} >= {}",
                placeholder(&mut params, base_type, value.to_string())
            ),
            "lt" => format!(
                "{raw} < {}",
                placeholder(&mut params, base_type, value.to_string())
            ),
            "lte" => format!(
                "{raw} <= {}",
                placeholder(&mut params, base_type, value.to_string())
            ),
            "contains" => format!(
                "{col} LIKE {}",
                placeholder(&mut params, "String", format!("%{}%", escape_like(value)))
            ),
            "notContains" => format!(
                "{col} NOT LIKE {}",
                placeholder(&mut params, "String", format!("%{}%", escape_like(value)))
            ),
            "startsWith" => format!(
                "{col} LIKE {}",
                placeholder(&mut params, "String", format!("{}%", escape_like(value)))
            ),
            "endsWith" => format!(
                "{col} LIKE {}",
                placeholder(&mut params, "String", format!("%{}", escape_like(value)))
            ),
            "like" => format!(
                "{col} LIKE {}",
                placeholder(&mut params, "String", value.to_string())
            ),
            "notLike" => format!(
                "{col} NOT LIKE {}",
                placeholder(&mut params, "String", value.to_string())
            ),
            "in" | "notIn" => {
                let items = split_list(value);
                if items.is_empty() {
                    continue;
                }
                let placeholders = items
                    .into_iter()
                    .map(|item| placeholder(&mut params, "String", item))
                    .collect::<Vec<_>>()
                    .join(", ");
                let sql_op = if operator == "in" { "IN" } else { "NOT IN" };
                format!("{col} {sql_op} ({placeholders})")
            }
            "between" | "notBetween" => {
                let items = split_list(value);
                if items.len() < 2 {
                    continue;
                }
                let low = placeholder(&mut params, base_type, items[0].clone());
                let high = placeholder(&mut params, base_type, items[1].clone());
                let sql_op = if operator == "between" {
                    "BETWEEN"
                } else {
                    "NOT BETWEEN"
                };
                format!("{raw} {sql_op} {low} AND {high}")
            }
            "isNull" => format!("{raw} IS NULL"),
            "isNotNull" => format!("{raw} IS NOT NULL"),
            "isEmpty" => format!("{col} = ''"),
            "isNotEmpty" => format!("{col} <> ''"),
            other => return Err(err!("error.unknownFilterOperator", operator = other)),
        };

        clauses.push(clause);
    }

    if clauses.is_empty() {
        return Ok((String::new(), params));
    }
    Ok((format!(" WHERE {}", clauses.join(" AND ")), params))
}

/// Reads one page of a table. `auto_increment_column` is always `None` — v1 never writes, so
/// nothing needs to know which column a resettable counter would be on.
///
/// `primary_key` is the table's sorting key (`system.tables.sorting_key`) rather than a real
/// constraint — ClickHouse has none — split naively on `, `. For a plain list of columns, which
/// almost every `ORDER BY` is, that is exactly right; for a key built from an expression it shows
/// the expression as if it were one long column name, which is a display quirk and not a
/// correctness problem, since nothing in v1 uses this to identify a row.
pub async fn table_data(
    conn: &Connection,
    database: &str,
    table: &str,
    query: &PageQuery,
) -> Result<TablePage, AppError> {
    let table_ref = qualified(database, table);
    let column_rows = table_columns(conn, database, table).await?;
    let columns: Vec<String> = column_rows.iter().map(|c| c.name.clone()).collect();

    let column_meta: BTreeMap<String, ColumnMeta> = column_rows
        .iter()
        .map(|c| {
            (
                c.name.clone(),
                ColumnMeta {
                    data_type: c.type_name.clone(),
                    nullable: c.type_name.starts_with("Nullable("),
                    default_value: None,
                    extra: String::new(),
                    foreign_key: None,
                },
            )
        })
        .collect();

    let column_types: BTreeMap<String, String> = column_rows
        .iter()
        .map(|c| (c.name.clone(), c.type_name.clone()))
        .collect();
    let (where_clause, params) = build_where(&query.filters, &column_types)?;
    let ch_params: Vec<(String, String)> = params;

    let sorting_key_result = query_with_params(
        conn,
        "SELECT sorting_key FROM system.tables WHERE database = {database:String} AND name = {table:String}",
        &[
            ("database".to_string(), database.to_string()),
            ("table".to_string(), table.to_string()),
        ],
    )
    .await;
    let sorting_key = sorting_key_result
        .ok()
        .and_then(|result| {
            result
                .data
                .first()
                .and_then(|row| row.get("sorting_key")?.as_str().map(str::to_string))
        })
        .unwrap_or_default();
    let primary_key: Vec<String> = if sorting_key.trim().is_empty() {
        Vec::new()
    } else {
        sorting_key.split(", ").map(str::to_string).collect()
    };

    let count_sql = format!("SELECT count() AS total FROM {table_ref}{where_clause}");
    let count_result = query_with_params(conn, &count_sql, &ch_params).await?;
    let total: i64 = count_result
        .data
        .first()
        .and_then(|row| row.get("total"))
        .and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse().ok())
                .or_else(|| v.as_i64())
        })
        .unwrap_or(0);

    let page_size = query.page_size.clamp(1, 5000);
    let offset = query.page.max(0).saturating_mul(page_size);
    let order_by = query
        .sort_column
        .as_deref()
        .filter(|c| columns.iter().any(|existing| existing == c))
        .map(|c| {
            format!(
                " ORDER BY {} {}",
                quote_ident(c),
                if query.sort_desc { "DESC" } else { "ASC" }
            )
        })
        .unwrap_or_default();
    let select_list = column_rows
        .iter()
        .map(|c| {
            let ident = quote_ident(&c.name);
            if is_decodable(&c.type_name) {
                ident
            } else {
                format!("toString({ident}) AS {ident}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let data_sql = format!(
        "SELECT {select_list} FROM {table_ref}{where_clause}{order_by} LIMIT {page_size} OFFSET {offset}"
    );
    let data_result = query_with_params(conn, &data_sql, &ch_params).await?;

    Ok(TablePage {
        columns,
        column_meta,
        primary_key,
        auto_increment_column: None,
        rows: data_result.data,
        total,
    })
}
