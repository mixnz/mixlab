//! One page of a table for the grid: the filters' `WHERE` clause, the foreign-key markers, and
//! the page they cut.

use super::{
    extra_tokens, map_error, primary_key, qualified_sql, qualify, quote_ident, resolve,
    row_to_json, table_columns, ColumnMeta, Filter, ForeignKey, PageQuery, TablePage,
};
use crate::error::AppError;
use crate::modules::db::drivers::filters::{escape_like, split_list};
use sqlx::{PgPool, Row};
use std::collections::BTreeMap;

/// Which columns of the table are foreign keys, and what each one points at.
///
/// Only the constraints the connected user can see are here, and a failure is swallowed by the
/// caller: the markers are decoration on the grid, and losing them must not cost the rows.
async fn foreign_keys(
    pool: &PgPool,
    schema: &str,
    table: &str,
) -> Result<BTreeMap<String, ForeignKey>, AppError> {
    let rows = sqlx::query(
        "SELECT att.attname AS column_name,
                refn.nspname AS ref_schema,
                refc.relname AS ref_table,
                refatt.attname AS ref_column
         FROM pg_constraint con
         JOIN pg_class rel ON rel.oid = con.conrelid
         JOIN pg_namespace nsp ON nsp.oid = rel.relnamespace
         JOIN pg_class refc ON refc.oid = con.confrelid
         JOIN pg_namespace refn ON refn.oid = refc.relnamespace
         JOIN LATERAL unnest(con.conkey, con.confkey) WITH ORDINALITY AS k(att, refatt, ord) ON TRUE
         JOIN pg_attribute att ON att.attrelid = con.conrelid AND att.attnum = k.att
         JOIN pg_attribute refatt ON refatt.attrelid = con.confrelid AND refatt.attnum = k.refatt
         WHERE con.contype = 'f' AND nsp.nspname = $1 AND rel.relname = $2
         ORDER BY con.conname, k.ord",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(map_error)?;

    let mut keys = BTreeMap::new();
    for row in &rows {
        keys.entry(row.get::<String, _>("column_name"))
            .or_insert_with(|| ForeignKey {
                table: qualify(
                    &row.get::<String, _>("ref_schema"),
                    &row.get::<String, _>("ref_table"),
                ),
                column: row.get::<String, _>("ref_column"),
            });
    }
    Ok(keys)
}

/// Turns the filter rows into a WHERE clause and the values to bind into it, in order.
///
/// Every value reaches the server as a bound parameter; the column name is the one part that has
/// to be interpolated, which is why it is checked against the table's own columns first.
///
/// Two operators differ from MySQL's by more than spelling. Comparisons are made against the
/// column cast to text, because a filter value arrives as text and PostgreSQL — unlike MySQL —
/// will not coerce it to the column's type on its own. And `regexp` is `~`, PostgreSQL's own
/// match operator.
///
/// The ordering operators are the one place that cast has to go the other way. `"id"::text > $1`
/// would sort 10 before 9, so the column is left as it is and the *value* is cast to the column's
/// type instead — which is what `columns` carries the type for. Binding the text alone is not
/// enough: sqlx names a bound `&str` as `text` on the wire, and PostgreSQL will not compare an
/// `integer` with one ("operator does not exist: integer > text").
pub(in crate::modules::db::drivers) fn build_where(
    filters: &[Filter],
    columns: &BTreeMap<String, String>,
) -> Result<(String, Vec<String>), AppError> {
    let mut clauses: Vec<String> = Vec::new();
    let mut binds: Vec<String> = Vec::new();
    // The number the next placeholder gets: PostgreSQL numbers them, so unlike MySQL's `?` they
    // cannot simply be counted off the bind list at the end.
    let mut next = 1usize;
    let mut placeholder = |binds: &mut Vec<String>, value: String| {
        binds.push(value);
        let p = format!("${next}");
        next += 1;
        p
    };

    for filter in filters {
        let Some(base_type) = columns.get(&filter.column) else {
            return Err(err!("error.unknownFilterColumn", column = &filter.column));
        };
        // Cast to text so that the value bound against it — always text — compares as typed. The
        // ordering operators are the exception: `>` on text would sort 10 before 9, so there the
        // column stands and the bound value is cast to its type with `typed` below.
        let col = format!("{}::text", quote_ident(&filter.column));
        let raw = quote_ident(&filter.column);
        // The type comes from `format_type`, which is PostgreSQL's own spelling of it — already
        // quoted where a name needs it, so it can be written into the cast as it stands.
        let typed = |p: String| format!("CAST({p} AS {base_type})");
        let value = filter.value.as_deref().unwrap_or("");
        let operator = filter.operator.as_str();

        let clause = match operator {
            "eq" => format!("{col} = {}", placeholder(&mut binds, value.to_string())),
            "ne" => format!("{col} <> {}", placeholder(&mut binds, value.to_string())),
            "gt" => format!(
                "{raw} > {}",
                typed(placeholder(&mut binds, value.to_string()))
            ),
            "gte" => format!(
                "{raw} >= {}",
                typed(placeholder(&mut binds, value.to_string()))
            ),
            "lt" => format!(
                "{raw} < {}",
                typed(placeholder(&mut binds, value.to_string()))
            ),
            "lte" => format!(
                "{raw} <= {}",
                typed(placeholder(&mut binds, value.to_string()))
            ),
            "contains" => format!(
                "{col} LIKE {}",
                placeholder(&mut binds, format!("%{}%", escape_like(value)))
            ),
            "notContains" => format!(
                "{col} NOT LIKE {}",
                placeholder(&mut binds, format!("%{}%", escape_like(value)))
            ),
            "startsWith" => format!(
                "{col} LIKE {}",
                placeholder(&mut binds, format!("{}%", escape_like(value)))
            ),
            "endsWith" => format!(
                "{col} LIKE {}",
                placeholder(&mut binds, format!("%{}", escape_like(value)))
            ),
            "like" => format!("{col} LIKE {}", placeholder(&mut binds, value.to_string())),
            "notLike" => format!(
                "{col} NOT LIKE {}",
                placeholder(&mut binds, value.to_string())
            ),
            "regexp" => format!("{col} ~ {}", placeholder(&mut binds, value.to_string())),
            "notRegexp" => format!("{col} !~ {}", placeholder(&mut binds, value.to_string())),
            "in" | "notIn" => {
                let items = split_list(value);
                if items.is_empty() {
                    continue;
                }
                let placeholders = items
                    .into_iter()
                    .map(|item| placeholder(&mut binds, item))
                    .collect::<Vec<_>>()
                    .join(", ");
                let sql_op = if operator == "in" { "IN" } else { "NOT IN" };
                format!("{col} {sql_op} ({placeholders})")
            }
            "between" | "notBetween" => {
                let items = split_list(value);
                // Two bounds or nothing — one of them alone says nothing about a range.
                if items.len() < 2 {
                    continue;
                }
                let low = typed(placeholder(&mut binds, items[0].clone()));
                let high = typed(placeholder(&mut binds, items[1].clone()));
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
        return Ok((String::new(), binds));
    }
    Ok((format!(" WHERE {}", clauses.join(" AND ")), binds))
}

/// Whether [`column_value`] has a decoder for this type, given as `format_type` spells it.
///
/// The ones it has not — arrays, user-defined enums, `inet`, `interval`, `money`, ranges,
/// composites — are asked for as text instead, which PostgreSQL can always produce and which shows
/// the value the way the server itself prints it. Without that a `text[]` column would arrive as
/// null, indistinguishable in the grid from a row that really is NULL.
///
/// Every number that can hold `NaN` or an infinity is on that list too, though [`column_value`] has
/// a decoder for each: `numeric` because `rust_decimal::Decimal` refuses those values outright, and
/// `real`/`double precision` because JSON has no way to write them — `serde_json::Number::from_f64`
/// answers `None`, and the branch falls through to null. No later branch would claim the column
/// either way, so a row holding `Infinity` would read in the grid as one holding nothing.
///
/// [`column_value`]: super::column_value
pub(super) fn is_decodable(data_type: &str) -> bool {
    let data_type = data_type.trim();
    // Tested before the modifier is cut off, since the `[]` comes *after* it: a
    // `character varying(255)[]` would otherwise be read as a plain `character varying`, decoded as
    // one, and arrive as null.
    if data_type.ends_with("[]") {
        return false;
    }
    let base = data_type.split('(').next().unwrap_or("").trim();
    matches!(
        base,
        "boolean"
            | "smallint"
            | "integer"
            | "bigint"
            | "text"
            | "character varying"
            | "character"
            | "name"
            | "uuid"
            | "json"
            | "jsonb"
            | "bytea"
            | "date"
            | "time without time zone"
            | "timestamp without time zone"
            | "timestamp with time zone"
    )
}

/// Reads one page of a table. `query.sort_column` orders the whole table before the page is cut
/// out of it, and is ignored unless it names a real column — which is also what keeps it out of
/// the SQL text unchecked. `query.filters` narrows the table down first, and `total` counts what
/// is left after them.
pub async fn table_data(
    pool: &PgPool,
    table: &str,
    query: &PageQuery,
) -> Result<TablePage, AppError> {
    let (schema, name) = resolve(table);
    let qualified = qualified_sql(&schema, &name);

    let column_rows = table_columns(pool, &schema, &name).await?;
    let columns: Vec<String> = column_rows
        .iter()
        .map(|r| r.get::<String, _>("name"))
        .collect();
    let mut foreign_keys = foreign_keys(pool, &schema, &name).await.unwrap_or_default();

    let column_meta: BTreeMap<String, ColumnMeta> = column_rows
        .iter()
        .map(|r| {
            let name = r.get::<String, _>("name");
            let foreign_key = foreign_keys.remove(&name);
            (
                name,
                ColumnMeta {
                    data_type: r.get::<String, _>("data_type"),
                    nullable: r.get::<bool, _>("nullable"),
                    default_value: r.get::<Option<String>, _>("default_value"),
                    extra: extra_tokens(r),
                    foreign_key,
                },
            )
        })
        .collect();

    let primary_key = primary_key(pool, &schema, &name).await.unwrap_or_default();
    let auto_increment_column = column_rows
        .iter()
        .find(|r| {
            let extra = extra_tokens(r);
            extra.contains("identity") || extra.contains("nextval")
        })
        .map(|r| r.get::<String, _>("name"));

    // The type without its modifier, which is what a filter value is cast to — `character varying`
    // rather than `character varying(255)`, since a cast that carries the length would truncate
    // rather than compare.
    let column_types: BTreeMap<String, String> = column_rows
        .iter()
        .map(|r| (r.get::<String, _>("name"), r.get::<String, _>("base_type")))
        .collect();
    let (where_clause, binds) = build_where(&query.filters, &column_types)?;

    let count_sql = format!("SELECT COUNT(*) FROM {qualified}{where_clause}");
    let mut count_query = sqlx::query_scalar(sqlx::AssertSqlSafe(count_sql));
    for value in &binds {
        count_query = count_query.bind(value.as_str());
    }
    let total: i64 = count_query.fetch_one(pool).await.map_err(map_error)?;

    // The ceiling is the largest page size the grid offers; see `mysql::table_data`.
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
    // Named one by one rather than `SELECT *`, so that a column of a type with no decoder can be
    // asked for as text — see {@link is_decodable}. The alias keeps the name the cast would
    // otherwise replace with `text`.
    let select_list = column_rows
        .iter()
        .map(|r| {
            let name = r.get::<String, _>("name");
            let column = quote_ident(&name);
            if is_decodable(&r.get::<String, _>("data_type")) {
                column
            } else {
                format!("{column}::text AS {column}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let data_sql = format!(
        "SELECT {select_list} FROM {qualified}{where_clause}{order_by} LIMIT {page_size} OFFSET {offset}"
    );
    let mut data_query = sqlx::query(sqlx::AssertSqlSafe(data_sql));
    for value in &binds {
        data_query = data_query.bind(value.as_str());
    }
    let rows = data_query.fetch_all(pool).await.map_err(map_error)?;

    Ok(TablePage {
        columns,
        column_meta,
        primary_key,
        auto_increment_column,
        rows: rows.iter().map(row_to_json).collect(),
        total,
    })
}
