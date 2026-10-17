//! What a PostgreSQL table is made of, and what every table in a database weighs — the Structure
//! and Statistics tabs.
//!
//! The shapes reported here are the ones `mysql/structure.rs` reports, because one grid draws
//! either. Where PostgreSQL has nothing to put in a field, it is left at its empty value rather
//! than dropped: `on_update_current_timestamp` is a MySQL clause with no counterpart, and
//! `prefix_length` an index feature PostgreSQL does not have.

use super::{extra_tokens, map_error, qualify, resolve, system_schema_filter, DEFAULT_SCHEMA};
use crate::error::AppError;
pub use crate::modules::db::drivers::sql::types::{
    Collation, IndexColumn, OutlineColumn, OutlineTable, SchemaOutline, StructureColumn,
    TableIndex, TableStats, TableStructure,
};
use sqlx::{PgPool, Row};
use std::collections::HashMap;

/// Everything the Structure tab shows about one table: its columns in table order, and its indexes
/// with the primary key first.
pub async fn table_structure(pool: &PgPool, table: &str) -> Result<TableStructure, AppError> {
    let (schema, name) = resolve(table);
    Ok(TableStructure {
        columns: structure_columns(pool, &schema, &name).await?,
        indexes: table_indexes(pool, &schema, &name).await?,
        skip_indexes: Vec::new(),
        engine: None,
    })
}

async fn structure_columns(
    pool: &PgPool,
    schema: &str,
    table: &str,
) -> Result<Vec<StructureColumn>, AppError> {
    let rows = sqlx::query(
        "SELECT a.attname AS name,
                format_type(a.atttypid, a.atttypmod) AS data_type,
                NOT a.attnotnull AS nullable,
                pg_get_expr(d.adbin, d.adrelid) AS default_value,
                a.attidentity::text AS identity,
                a.attgenerated::text AS generated,
                coll.collname AS collation,
                COALESCE(col_description(a.attrelid, a.attnum), '') AS comment,
                COALESCE(k.is_primary, false) AS is_primary,
                COALESCE(k.is_unique, false) AS is_unique,
                COALESCE(k.is_indexed, false) AS is_indexed
         FROM pg_attribute a
         JOIN pg_class c ON c.oid = a.attrelid
         JOIN pg_namespace n ON n.oid = c.relnamespace
         JOIN pg_type tp ON tp.oid = a.atttypid
         LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
         -- Only a collation the column asked for. A collatable column always carries one, and for
         -- almost all of them it is the type's own — reporting that would put `default` beside
         -- every piece of text on screen, and, worse, make the column editor read it as a choice
         -- the user had made and rewrite the table to \"restore\" it.
         LEFT JOIN pg_collation coll
             ON coll.oid = a.attcollation
             AND a.attcollation NOT IN (0, tp.typcollation)
         -- Aggregated to the side rather than grouped: a default is a `pg_node_tree`, which has no
         -- equality operator and so cannot appear in a GROUP BY at all.
         LEFT JOIN LATERAL (
             SELECT bool_or(i.indisprimary) AS is_primary,
                    bool_or(i.indisunique) AS is_unique,
                    count(*) > 0 AS is_indexed
             FROM pg_index i
             WHERE i.indrelid = a.attrelid AND i.indkey[0] = a.attnum
         ) k ON TRUE
         WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped
         ORDER BY a.attnum",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(map_error)?;

    Ok(rows
        .iter()
        .map(|row| {
            let extra = extra_tokens(row);
            let default_value: Option<String> = row.get("default_value");
            // Only the leading column of an index is marked, as MySQL's Key column is: a column
            // in the middle of a composite index is not usable as a lookup on its own.
            let key = if row.get::<bool, _>("is_primary") {
                "PRI"
            } else if row.get::<bool, _>("is_unique") {
                "UNI"
            } else if row.get::<bool, _>("is_indexed") {
                "MUL"
            } else {
                ""
            };
            StructureColumn {
                name: row.get("name"),
                data_type: row.get("data_type"),
                nullable: row.get("nullable"),
                default_is_expression: default_value.is_some(),
                default_value,
                auto_increment: extra.contains("identity") || extra.contains("nextval"),
                on_update_current_timestamp: false,
                generated: extra.contains("generated"),
                collation: row.get("collation"),
                comment: row.get("comment"),
                key: key.to_string(),
                extra,
            }
        })
        .collect())
}

/// The table's indexes, the primary key first.
///
/// Each position is joined against `pg_attribute` for the column's name as it is really spelled.
/// `pg_get_indexdef` would answer with it quoted — `"order"`, `"Name"` — and that spelling is
/// neither what the picker in the index dialog offers nor what `modify_index` may quote a second
/// time. A position over an expression rather than a column has no `pg_attribute` row, which is
/// the `None` that `IndexColumn::name` carries.
async fn table_indexes(
    pool: &PgPool,
    schema: &str,
    table: &str,
) -> Result<Vec<TableIndex>, AppError> {
    let rows = sqlx::query(
        "SELECT ic.relname AS name,
                i.indisunique AS is_unique,
                i.indisprimary AS is_primary,
                am.amname AS index_type,
                COALESCE(obj_description(ic.oid, 'pg_class'), '') AS comment,
                a.attname AS column_name
         FROM pg_index i
         JOIN pg_class c ON c.oid = i.indrelid
         JOIN pg_namespace n ON n.oid = c.relnamespace
         JOIN pg_class ic ON ic.oid = i.indexrelid
         JOIN pg_am am ON am.oid = ic.relam
         JOIN LATERAL unnest(i.indkey::smallint[]) WITH ORDINALITY AS k(attnum, ord) ON TRUE
         LEFT JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = k.attnum
         WHERE n.nspname = $1 AND c.relname = $2
         ORDER BY i.indisprimary DESC, ic.relname, k.ord",
    )
    .bind(schema)
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(map_error)?;

    let mut indexes: Vec<TableIndex> = Vec::new();
    for row in &rows {
        let name: String = row.get("name");
        if indexes.last().map(|last| last.name != name).unwrap_or(true) {
            indexes.push(TableIndex {
                name,
                unique: row.get("is_unique"),
                primary: row.get("is_primary"),
                index_type: row.get("index_type"),
                columns: Vec::new(),
                comment: row.get("comment"),
            });
        }
        // attnum 0 marks a position that indexes an expression rather than a column, and matches
        // no `pg_attribute` row — so the outer join leaves the name null, which is what says so.
        let name: Option<String> = row.get("column_name");
        if let Some(index) = indexes.last_mut() {
            index.columns.push(IndexColumn {
                name,
                prefix_length: None,
            });
        }
    }
    Ok(indexes)
}

/// What every table in the connected database weighs, across every schema the user can see.
///
/// Views and foreign tables are left out, as they are on MySQL: neither stores anything of its
/// own, and both would read here as empty tables.
pub async fn table_stats(pool: &PgPool) -> Result<Vec<TableStats>, AppError> {
    let schemas = system_schema_filter("n");
    let sql = format!(
        "SELECT n.nspname, c.relname,
                GREATEST(c.reltuples, 0)::bigint AS row_estimate,
                pg_table_size(c.oid) AS data_size,
                pg_indexes_size(c.oid) AS index_size
         FROM pg_class c
         JOIN pg_namespace n ON n.oid = c.relnamespace
         WHERE c.relkind IN ('r', 'p') AND {schemas}
         ORDER BY (n.nspname <> '{DEFAULT_SCHEMA}'), n.nspname, c.relname"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await
        .map_err(map_error)?;

    Ok(rows
        .iter()
        .map(|row| {
            let count = row.get::<i64, _>("row_estimate").max(0) as u64;
            let data_size = row.get::<i64, _>("data_size").max(0) as u64;
            TableStats {
                name: qualify(
                    &row.get::<String, _>("nspname"),
                    &row.get::<String, _>("relname"),
                ),
                rows: count,
                data_size,
                index_size: row.get::<i64, _>("index_size").max(0) as u64,
                avg_record_size: data_size.checked_div(count).unwrap_or(0),
            }
        })
        .collect())
}

/// Every collation the server has, the database's own default first.
///
/// PostgreSQL's collations belong to an encoding rather than to a character set, and the ones that
/// work for any encoding report `-1` — reported here as an empty charset, which is what groups
/// them together in the picker.
pub async fn collations(pool: &PgPool) -> Result<Vec<Collation>, AppError> {
    let rows = sqlx::query(
        "SELECT collname,
                COALESCE(pg_encoding_to_char(NULLIF(collencoding, -1)), '') AS charset,
                collname = 'default' AS is_default
         FROM pg_collation
         ORDER BY (collname <> 'default'), charset, collname",
    )
    .fetch_all(pool)
    .await
    .map_err(map_error)?;

    Ok(rows
        .iter()
        .map(|row| Collation {
            name: row.get("collname"),
            charset: row.get("charset"),
            is_default: row.get("is_default"),
        })
        .collect())
}

/// What the Query tab's completion knows about the connected database: every table and column of
/// it, across every schema the user can see, in one read.
///
/// Views are in it as well as tables — their columns complete like any others'. Only what the
/// connected user has privileges to see is here, so a missing table means "not visible to you".
pub async fn schema_outline(pool: &PgPool, database: &str) -> Result<SchemaOutline, AppError> {
    // The foreign keys first, so each column is built already knowing where it points. A failure
    // is not worth failing the whole outline over: completion is for the columns, and where a key
    // points is a line of detail beside them.
    let schemas = system_schema_filter("nsp");
    let key_sql = format!(
        "SELECT nsp.nspname AS schema_name, rel.relname AS table_name, att.attname AS column_name,
                refn.nspname AS ref_schema, refc.relname AS ref_table, refatt.attname AS ref_column
         FROM pg_constraint con
         JOIN pg_class rel ON rel.oid = con.conrelid
         JOIN pg_namespace nsp ON nsp.oid = rel.relnamespace
         JOIN pg_class refc ON refc.oid = con.confrelid
         JOIN pg_namespace refn ON refn.oid = refc.relnamespace
         JOIN LATERAL unnest(con.conkey, con.confkey) WITH ORDINALITY AS k(att, refatt, ord) ON TRUE
         JOIN pg_attribute att ON att.attrelid = con.conrelid AND att.attnum = k.att
         JOIN pg_attribute refatt ON refatt.attrelid = con.confrelid AND refatt.attnum = k.refatt
         WHERE con.contype = 'f' AND {schemas}"
    );
    let key_rows = sqlx::query(sqlx::AssertSqlSafe(key_sql))
        .fetch_all(pool)
        .await
        .unwrap_or_default();

    let mut references: HashMap<(String, String), String> = HashMap::new();
    for row in &key_rows {
        references.insert(
            (
                qualify(
                    &row.get::<String, _>("schema_name"),
                    &row.get::<String, _>("table_name"),
                ),
                row.get::<String, _>("column_name"),
            ),
            format!(
                "{}.{}",
                qualify(
                    &row.get::<String, _>("ref_schema"),
                    &row.get::<String, _>("ref_table")
                ),
                row.get::<String, _>("ref_column")
            ),
        );
    }

    let schemas = system_schema_filter("n");
    let column_sql = format!(
        "SELECT n.nspname, c.relname,
                a.attname AS column_name,
                format_type(a.atttypid, a.atttypmod) AS data_type,
                NOT a.attnotnull AS nullable,
                COALESCE(k.is_primary, false) AS is_primary,
                COALESCE(k.is_unique, false) AS is_unique,
                COALESCE(k.is_indexed, false) AS is_indexed
         FROM pg_attribute a
         JOIN pg_class c ON c.oid = a.attrelid
         JOIN pg_namespace n ON n.oid = c.relnamespace
         LEFT JOIN LATERAL (
             SELECT bool_or(i.indisprimary) AS is_primary,
                    bool_or(i.indisunique) AS is_unique,
                    count(*) > 0 AS is_indexed
             FROM pg_index i
             WHERE i.indrelid = a.attrelid AND i.indkey[0] = a.attnum
         ) k ON TRUE
         WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f') AND {schemas}
           AND a.attnum > 0 AND NOT a.attisdropped
         ORDER BY (n.nspname <> '{DEFAULT_SCHEMA}'), n.nspname, c.relname, a.attnum"
    );
    let column_rows = sqlx::query(sqlx::AssertSqlSafe(column_sql))
        .fetch_all(pool)
        .await
        .map_err(map_error)?;

    let mut tables: Vec<OutlineTable> = Vec::new();
    for row in &column_rows {
        let table = qualify(
            &row.get::<String, _>("nspname"),
            &row.get::<String, _>("relname"),
        );
        let name: String = row.get("column_name");
        let key = if row.get::<bool, _>("is_primary") {
            "PRI"
        } else if row.get::<bool, _>("is_unique") {
            "UNI"
        } else if row.get::<bool, _>("is_indexed") {
            "MUL"
        } else {
            ""
        };
        let column = OutlineColumn {
            references: references.get(&(table.clone(), name.clone())).cloned(),
            name,
            data_type: row.get("data_type"),
            nullable: row.get("nullable"),
            key: key.to_string(),
        };
        // Ordered by table and then by position within it, so each table's columns arrive together
        // and the one being built is always the last.
        match tables.last_mut() {
            Some(last) if last.name == table => last.columns.push(column),
            _ => tables.push(OutlineTable {
                name: table,
                columns: vec![column],
            }),
        }
    }

    Ok(SchemaOutline {
        database: database.to_string(),
        tables,
    })
}
