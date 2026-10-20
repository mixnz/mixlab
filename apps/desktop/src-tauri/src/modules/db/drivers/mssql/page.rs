//! One page of a table for the grid: the filters' `WHERE` clause, the order and the foreign-key
//! markers, and the page they cut.

use super::{
    escape_like_mssql, extra_tokens, map_error, primary_key, qualify, quote_ident,
    read_uncommitted, resolve, row_to_json, select_expr, table_columns, three_part, ColumnMeta,
    Filter, ForeignKey, PageQuery, Pool, TablePage,
};
use crate::error::AppError;
use crate::modules::db::drivers::filters::split_list;
use std::collections::BTreeMap;

/// Turns the filter rows into a WHERE clause and the values to bind into it, in order.
///
/// Every value reaches the server as a bound parameter — tiberius numbers them `@P1`, `@P2`, …, the
/// way PostgreSQL numbers `$1`, so they cannot simply be counted off the bind list at the end. The
/// column name is the one part that has to be interpolated, which is why it is checked against the
/// table's own columns first.
///
/// Two differences from the PostgreSQL version this is shaped after:
///
/// * **No regex.** SQL Server has no counterpart to `~` or `REGEXP` before its 2025 release, so
///   there is no arm for `regexp`/`notRegexp` and `mssqlDialect.regexpFilter` is false — the
///   operator is gone from the dropdown rather than offered and failing (D12).
/// * **Case is the column's business.** There is no `ILIKE` here, and no `LOWER()` wrapped around
///   anything: whether a comparison is case-sensitive is decided by the column's collation, and
///   most installations default to a `_CI_` one — so in practice these filters ignore case. Forcing
///   it either way with `LOWER()` would only make every one of them unable to use an index.
///
/// Text comparisons cast the column to `nvarchar(max)` so a value bound as text compares as typed.
/// The ordering operators are the exception, and cast nothing: `CAST([id] AS nvarchar(max)) > '9'`
/// would sort 10 before 9. There the column stands and SQL Server converts the bound text to the
/// column's own type, which is what it does implicitly in a comparison — and what it refuses,
/// loudly, when the text is not a value of that type after all. That refusal is the right answer:
/// the user typed something this column could not hold.
pub(in crate::modules::db::drivers) fn build_where(
    filters: &[Filter],
    columns: &BTreeMap<String, String>,
) -> Result<(String, Vec<String>), AppError> {
    let mut clauses: Vec<String> = Vec::new();
    let mut binds: Vec<String> = Vec::new();
    let mut next = 1usize;
    let mut placeholder = |binds: &mut Vec<String>, value: String| {
        binds.push(value);
        let p = format!("@P{next}");
        next += 1;
        p
    };

    for filter in filters {
        if !columns.contains_key(&filter.column) {
            return Err(err!("error.unknownFilterColumn", column = &filter.column));
        }
        let raw = quote_ident(&filter.column);
        let col = format!("CAST({raw} AS nvarchar(max))");
        let value = filter.value.as_deref().unwrap_or("");
        let operator = filter.operator.as_str();
        // Every pattern this builds escapes with a backslash, and SQL Server only knows that if the
        // pattern says so.
        let like = |negated: bool, p: String| {
            let not = if negated { "NOT " } else { "" };
            format!("{col} {not}LIKE {p} ESCAPE '\\'")
        };

        let clause = match operator {
            "eq" => format!("{col} = {}", placeholder(&mut binds, value.to_string())),
            "ne" => format!("{col} <> {}", placeholder(&mut binds, value.to_string())),
            "gt" => format!("{raw} > {}", placeholder(&mut binds, value.to_string())),
            "gte" => format!("{raw} >= {}", placeholder(&mut binds, value.to_string())),
            "lt" => format!("{raw} < {}", placeholder(&mut binds, value.to_string())),
            "lte" => format!("{raw} <= {}", placeholder(&mut binds, value.to_string())),
            "contains" => like(
                false,
                placeholder(&mut binds, format!("%{}%", escape_like_mssql(value))),
            ),
            "notContains" => like(
                true,
                placeholder(&mut binds, format!("%{}%", escape_like_mssql(value))),
            ),
            "startsWith" => like(
                false,
                placeholder(&mut binds, format!("{}%", escape_like_mssql(value))),
            ),
            "endsWith" => like(
                false,
                placeholder(&mut binds, format!("%{}", escape_like_mssql(value))),
            ),
            // `like`/`notLike` hand the pattern over as the user wrote it — that is the point of
            // the operator — so the value is not escaped. The `ESCAPE` clause still stands, which
            // is how a pattern of their own can escape a wildcard too.
            "like" => like(false, placeholder(&mut binds, value.to_string())),
            "notLike" => like(true, placeholder(&mut binds, value.to_string())),
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
                let low = placeholder(&mut binds, items[0].clone());
                let high = placeholder(&mut binds, items[1].clone());
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

/// The ORDER BY a page is cut out of.
///
/// Always present, because `OFFSET ... ROWS FETCH NEXT ... ROWS ONLY` is grammatically part of
/// `ORDER BY` and SQL Server refuses the one without the other. `(SELECT NULL)` is what "no order
/// was asked for" is spelled as: it satisfies the grammar and asks the server to sort nothing,
/// which is the semantics `postgres::table_data` gets by leaving the clause out entirely. Ordering
/// by the first column instead would sort the whole table on every page turn.
pub(super) fn order_by_clause(
    sort_column: Option<&str>,
    sort_desc: bool,
    columns: &[String],
) -> String {
    sort_column
        .filter(|c| columns.iter().any(|existing| existing == c))
        .map(|c| {
            format!(
                " ORDER BY {} {}",
                quote_ident(c),
                if sort_desc { "DESC" } else { "ASC" }
            )
        })
        .unwrap_or_else(|| " ORDER BY (SELECT NULL)".to_string())
}

/// Which columns of the table are foreign keys, and what each one points at.
///
/// A failure is swallowed by the caller: the markers are decoration on the grid, and losing them
/// must not cost the rows.
async fn foreign_keys(
    pool: &Pool,
    database: &str,
    schema: &str,
    table: &str,
) -> Result<BTreeMap<String, ForeignKey>, AppError> {
    let db = quote_ident(database);
    let sql = read_uncommitted(&format!(
        "SELECT c.name AS column_name,
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
             AND rc.column_id = fk.referenced_column_id
         WHERE s.name = @P1 AND o.name = @P2
         ORDER BY fk.constraint_object_id, fk.constraint_column_id"
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

    let mut keys = BTreeMap::new();
    for row in &rows {
        let (Some(column), Some(ref_schema), Some(ref_table), Some(ref_column)) = (
            row.get::<&str, _>("column_name"),
            row.get::<&str, _>("ref_schema"),
            row.get::<&str, _>("ref_table"),
            row.get::<&str, _>("ref_column"),
        ) else {
            continue;
        };
        // The first key a column takes part in is the one shown; a column in two keys is rare and
        // the grid has one marker to give it.
        keys.entry(column.to_string())
            .or_insert_with(|| ForeignKey {
                table: qualify(ref_schema, ref_table),
                column: ref_column.to_string(),
            });
    }
    Ok(keys)
}

/// Reads one page of a table, and how many rows the filters leave to page through.
///
/// `table` arrives as the sidebar names it — bare for a table of `dbo`, `schema.table` otherwise —
/// and [`resolve`] turns it back into the two parts. `database` is named in the SQL rather than
/// switched to with `USE`: the pooled session may be sitting on any database at all, and a `USE`
/// would leave it somewhere else for whoever borrows the connection next (D2).
pub async fn table_data(
    pool: &Pool,
    database: &str,
    table: &str,
    query: &PageQuery,
) -> Result<TablePage, AppError> {
    let (schema, name) = resolve(table);
    let qualified = three_part(database, &schema, &name);

    let column_rows = table_columns(pool, database, &schema, &name).await?;
    let columns: Vec<String> = column_rows.iter().map(|c| c.name.clone()).collect();
    let mut keys = foreign_keys(pool, database, &schema, &name)
        .await
        .unwrap_or_default();

    let column_meta: BTreeMap<String, ColumnMeta> = column_rows
        .iter()
        .map(|c| {
            (
                c.name.clone(),
                ColumnMeta {
                    data_type: c.data_type.clone(),
                    nullable: c.nullable,
                    default_value: c.default_value.clone(),
                    extra: extra_tokens(c.is_identity, c.is_computed, c.is_rowversion),
                    foreign_key: keys.remove(&c.name),
                },
            )
        })
        .collect();

    let primary_key = primary_key(pool, database, &schema, &name)
        .await
        .unwrap_or_default();
    let auto_increment_column = column_rows
        .iter()
        .find(|c| c.is_identity)
        .map(|c| c.name.clone());

    // `build_where` only needs to know a column exists; unlike PostgreSQL, SQL Server converts a
    // bound string to the column's type in a comparison itself, so nothing here casts the value.
    let column_types: BTreeMap<String, String> = column_rows
        .iter()
        .map(|c| (c.name.clone(), c.data_type.clone()))
        .collect();
    let (where_clause, binds) = build_where(&query.filters, &column_types)?;

    let mut client = pool
        .get()
        .await
        .map_err(|e| err!("error.mssql", message = e))?;

    // `COUNT_BIG` rather than `COUNT`, which answers as an `int` and overflows — with an error from
    // the server, not a wrong number — somewhere past two billion rows.
    let count_sql = read_uncommitted(&format!(
        "SELECT COUNT_BIG(*) FROM {qualified}{where_clause}"
    ));
    let mut count_query = tiberius::Query::new(count_sql);
    for value in &binds {
        count_query.bind(value.as_str());
    }
    let total: i64 = count_query
        .query(&mut client)
        .await
        .map_err(map_error)?
        .into_row()
        .await
        .map_err(map_error)?
        .and_then(|row| row.get::<i64, _>(0))
        .unwrap_or(0);

    // The ceiling is the largest page size the grid offers; see `mysql::table_data`.
    let page_size = query.page_size.clamp(1, 5000);
    let offset = query.page.max(0).saturating_mul(page_size);
    let order_by = order_by_clause(query.sort_column.as_deref(), query.sort_desc, &columns);
    let select_list = column_rows
        .iter()
        .map(|c| select_expr(&quote_ident(&c.name), &c.data_type))
        .collect::<Vec<_>>()
        .join(", ");
    let data_sql = read_uncommitted(&format!(
        "SELECT {select_list} FROM {qualified}{where_clause}{order_by} \
         OFFSET {offset} ROWS FETCH NEXT {page_size} ROWS ONLY"
    ));
    let mut data_query = tiberius::Query::new(data_sql);
    for value in &binds {
        data_query.bind(value.as_str());
    }
    let rows = data_query
        .query(&mut client)
        .await
        .map_err(map_error)?
        .into_first_result()
        .await
        .map_err(map_error)?;

    Ok(TablePage {
        columns,
        column_meta,
        primary_key,
        auto_increment_column,
        rows: rows.iter().map(row_to_json).collect(),
        total,
    })
}
