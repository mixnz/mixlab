//! One page of a table for the grid: the filters' `WHERE` clause, and the page it cuts.

use super::{
    is_decodable, qualified, query_with_params, quote_ident, table_columns, ColumnMeta, Connection,
    Filter, PageQuery, TablePage,
};
use crate::error::AppError;
use crate::modules::db::drivers::filters::{
    self, escape_like, LikeEscape, Placeholders, WhereSyntax,
};
use std::collections::BTreeMap;

/// What ClickHouse writes its own way in a filter's `WHERE`. Every text comparison goes through
/// `toString()` on the column: ClickHouse, unlike MySQL, does not coerce a `String` parameter to
/// compare against a `UInt64` column on its own.
const WHERE_SYNTAX: WhereSyntax = WhereSyntax {
    quote_ident,
    as_text: |quoted| format!("toString({quoted})"),
    regexp: None,
    escape_like,
    like_escape: LikeEscape::Never,
};

/// ClickHouse's named parameters, `{p0:String}`, `{p1:UInt64}`, … — see [`query_with_params`].
/// Each carries its own type: `String` for a text comparison, the column's type for an ordering
/// one, since `toString()` on a number sorts `10` before `9`.
#[derive(Default)]
struct Params {
    params: Vec<(String, String)>,
}

impl Placeholders for Params {
    type Bind = (String, String);

    fn text(&mut self, value: String) -> String {
        self.typed(value, "String")
    }

    fn typed(&mut self, value: String, column_type: &str) -> String {
        let name = format!("p{}", self.params.len());
        let placeholder = format!("{{{name}:{column_type}}}");
        self.params.push((name, value));
        placeholder
    }

    fn into_binds(self) -> Vec<(String, String)> {
        self.params
    }
}

/// Turns the filter rows into a `WHERE` clause and the parameters to bind into it, in
/// `{name:Type}` form. See [`filters::build_where`].
pub(in crate::modules::db::drivers) fn build_where(
    filters: &[Filter],
    columns: &BTreeMap<String, String>,
) -> Result<(String, Vec<(String, String)>), AppError> {
    filters::build_where(
        filters,
        |name| columns.get(name).map(String::as_str),
        &WHERE_SYNTAX,
        Params::default(),
    )
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
