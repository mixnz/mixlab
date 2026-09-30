//! What a SQLite table is made of, what every table weighs, and what the Query tab completes
//! against — the Structure and Statistics tabs, and the schema outline.
//!
//! The shapes reported here are the ones `mysql_structure.rs` and `postgres_structure.rs` report,
//! because one grid draws all three. Where SQLite has nothing to put in a field it is left at its
//! empty value rather than dropped: there are no column comments, no index methods to choose
//! between, and no index prefix lengths.
//!
//! Three things here have no counterpart in the other two, and all three come from the same fact —
//! SQLite keeps no catalogue of its own beyond `sqlite_master`:
//!
//! * A row count is counted, not estimated. There is no `information_schema.tables` holding a
//!   figure to read; `COUNT(*)` is the only answer there is.
//! * Sizes come from `dbstat`, a virtual table that walks the file's pages. It exists because
//!   MixLab bundles its own SQLite with `SQLITE_ENABLE_DBSTAT_VTAB` — see the `libsqlite3-sys`
//!   entry in `Cargo.toml`. A build linked against a system SQLite may not have it, so a failure
//!   reads as "sizes unknown" rather than failing the tab.
//! * A column's collation is not reported at all. It is not in `pragma_table_info`, and the only
//!   place it exists is the text of the `CREATE TABLE` in `sqlite_master` — parsing which, to fill
//!   one optional field, would be a SQL parser this module has no other use for.

use super::sqlite::{map_error, quote_ident, split_default};
use crate::error::AppError;
use serde::Serialize;
use sqlx::{Row, SqlitePool};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureColumn {
    pub name: String,
    /// The declared type as the table's own DDL spells it, which in SQLite decides an affinity
    /// rather than a type — `VARCHAR(255)` and `TEXT` behave identically, and a column may be
    /// declared with no type at all.
    pub data_type: String,
    pub nullable: bool,
    pub default_value: Option<String>,
    pub default_is_expression: bool,
    /// The `INTEGER PRIMARY KEY` that aliases the rowid — the only column SQLite fills in itself.
    pub auto_increment: bool,
    /// Always false. MySQL's `ON UPDATE CURRENT_TIMESTAMP` has no SQLite counterpart; the same
    /// effect is a trigger, which is not a property of the column.
    pub on_update_current_timestamp: bool,
    pub generated: bool,
    /// Always `None` — see the note at the top of this file.
    pub collation: Option<String>,
    /// Always empty: SQLite has no column comments.
    pub comment: String,
    /// `PRI`, `UNI`, `MUL` or empty, as MySQL spells it — which kind of key this column leads.
    pub key: String,
    /// The tokens described on `sqlite::ColumnMeta::extra`.
    pub extra: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexColumn {
    /// `None` for an index over an expression rather than a column.
    pub name: Option<String>,
    /// Always `None`: SQLite indexes a whole value, and has no counterpart to MySQL's
    /// index-the-first-n-characters.
    pub prefix_length: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableIndex {
    pub name: String,
    pub unique: bool,
    pub primary: bool,
    /// Always `btree`. Every SQLite index is one, which is also why the index dialog offers no
    /// method to choose — see `sqliteEditing`.
    pub index_type: String,
    pub columns: Vec<IndexColumn>,
    /// Always empty: SQLite has no index comments.
    pub comment: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableStructure {
    /// In table order, which is the order a `SELECT *` returns them in.
    pub columns: Vec<StructureColumn>,
    /// The primary key first, then the rest by name.
    pub indexes: Vec<TableIndex>,
    /// Always empty: data skipping indices are a ClickHouse-only concept. See
    /// `docs/specs/2026-09-04-clickhouse-index-ddl-design.md`.
    pub skip_indexes: Vec<super::clickhouse::SkipIndex>,
    /// Always `None` — the engine guard in the Structure tab only ever reads this for ClickHouse.
    pub engine: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Collation {
    pub name: String,
    pub charset: String,
    pub is_default: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableStats {
    pub name: String,
    /// Counted, not estimated — SQLite keeps no figure to read.
    pub rows: u64,
    pub data_size: u64,
    pub index_size: u64,
    /// Derived rather than reported: the data size over the counted rows.
    pub avg_record_size: u64,
}

/// The name SQLite gives the primary key of a table whose key is the rowid.
///
/// Such a key has no index of its own — the table *is* the index, the rows being stored in rowid
/// order — so there is nothing in `pragma_index_list` to name. Shown under MySQL's name for the
/// same thing, which is what the grid's other rows are named after.
const IMPLICIT_PRIMARY_KEY: &str = "PRIMARY";

/// Everything the Structure tab shows about one table: its columns in table order, and its indexes
/// with the primary key first.
pub async fn table_structure(pool: &SqlitePool, table: &str) -> Result<TableStructure, AppError> {
    let indexes = table_indexes(pool, table).await?;
    Ok(TableStructure {
        columns: structure_columns(pool, table, &indexes).await?,
        indexes,
        skip_indexes: Vec::new(),
        engine: None,
    })
}

/// The columns, in table order.
///
/// `key` is worked out from the indexes rather than read: SQLite has no per-column flag for it, so
/// which kind of key a column leads is a question about what indexes exist over it.
async fn structure_columns(
    pool: &SqlitePool,
    table: &str,
    indexes: &[TableIndex],
) -> Result<Vec<StructureColumn>, AppError> {
    let rows = sqlx::query(
        "select name, type, \"notnull\", dflt_value, pk, hidden from pragma_table_xinfo(?)",
    )
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(map_error)?;

    let key_count = rows.iter().filter(|r| r.get::<i64, _>("pk") > 0).count();

    Ok(rows
        .iter()
        .filter(|r| r.get::<i64, _>("hidden") != 1)
        .map(|r| {
            let name = r.get::<String, _>("name");
            let declared_type = r.get::<String, _>("type");
            let pk = r.get::<i64, _>("pk");
            let hidden = r.get::<i64, _>("hidden");
            let generated = hidden == 2 || hidden == 3;
            let auto_increment =
                key_count == 1 && pk == 1 && declared_type.eq_ignore_ascii_case("integer");
            let (default_value, default_is_expression) =
                split_default(r.get::<Option<String>, _>("dflt_value"));

            let mut extra: Vec<&str> = Vec::new();
            if auto_increment {
                extra.push("rowid");
            }
            if generated {
                extra.push("generated");
            }

            StructureColumn {
                key: key_for(&name, pk, indexes),
                name,
                data_type: declared_type,
                nullable: r.get::<i64, _>("notnull") == 0,
                default_value,
                default_is_expression,
                auto_increment,
                on_update_current_timestamp: false,
                generated,
                collation: None,
                comment: String::new(),
                extra: extra.join(" "),
            }
        })
        .collect())
}

/// Which kind of key this column leads, as MySQL spells it: `PRI` for the primary key, `UNI` for a
/// unique index it is first in, `MUL` for any other index it is first in, and empty otherwise.
fn key_for(column: &str, pk: i64, indexes: &[TableIndex]) -> String {
    if pk > 0 {
        return "PRI".to_string();
    }
    let leads =
        |index: &&TableIndex| index.columns.first().and_then(|c| c.name.as_deref()) == Some(column);
    if indexes.iter().filter(leads).any(|index| index.unique) {
        return "UNI".to_string();
    }
    if indexes.iter().any(|index| leads(&index)) {
        return "MUL".to_string();
    }
    String::new()
}

/// The indexes over one table, the primary key first and the rest by name.
async fn table_indexes(pool: &SqlitePool, table: &str) -> Result<Vec<TableIndex>, AppError> {
    let listed = sqlx::query("select name, \"unique\", origin from pragma_index_list(?)")
        .bind(table)
        .fetch_all(pool)
        .await
        .map_err(map_error)?;

    let mut indexes: Vec<TableIndex> = Vec::new();
    for row in &listed {
        let name = row.get::<String, _>("name");
        let columns = sqlx::query("select name from pragma_index_info(?) order by seqno")
            .bind(&name)
            .fetch_all(pool)
            .await
            .map_err(map_error)?;
        indexes.push(TableIndex {
            unique: row.get::<i64, _>("unique") != 0,
            // `pk` for the index behind a PRIMARY KEY, `u` for one behind a UNIQUE constraint,
            // `c` for one someone wrote a CREATE INDEX for.
            primary: row.get::<String, _>("origin") == "pk",
            name,
            index_type: "btree".to_string(),
            columns: columns
                .iter()
                .map(|c| IndexColumn {
                    // Null for a column of an expression index — `CREATE INDEX … ON t (lower(x))`.
                    name: c.get::<Option<String>, _>("name"),
                    prefix_length: None,
                })
                .collect(),
            comment: String::new(),
        });
    }

    /* A rowid primary key has no index to have been listed, so it is added here from the columns
    that carry it. Without this the Structure tab would show a table whose key column is marked
    PRI and whose index list does not mention a primary key at all. */
    if !indexes.iter().any(|index| index.primary) {
        let key = sqlx::query("select name, pk from pragma_table_xinfo(?) order by pk")
            .bind(table)
            .fetch_all(pool)
            .await
            .map_err(map_error)?;
        let columns: Vec<IndexColumn> = key
            .iter()
            .filter(|r| r.get::<i64, _>("pk") > 0)
            .map(|r| IndexColumn {
                name: Some(r.get::<String, _>("name")),
                prefix_length: None,
            })
            .collect();
        if !columns.is_empty() {
            indexes.push(TableIndex {
                name: IMPLICIT_PRIMARY_KEY.to_string(),
                unique: true,
                primary: true,
                index_type: "btree".to_string(),
                columns,
                comment: String::new(),
            });
        }
    }

    indexes.sort_by(|a, b| b.primary.cmp(&a.primary).then_with(|| a.name.cmp(&b.name)));
    Ok(indexes)
}

/// Every collation this SQLite can order text by.
///
/// Three, unless an extension has registered more: `BINARY` (the default, byte for byte), `NOCASE`
/// (ASCII letters only — it does not fold anything outside A–Z) and `RTRIM`. Read from the engine
/// rather than hard-coded, so a loaded extension's own collation shows up in the picker.
///
/// `charset` is empty for all of them. SQLite has no character sets: text is UTF-8 and there is
/// nothing to group the list by.
pub async fn collations(pool: &SqlitePool) -> Result<Vec<Collation>, AppError> {
    let rows = sqlx::query("select name from pragma_collation_list() order by name")
        .fetch_all(pool)
        .await
        .map_err(map_error)?;

    Ok(rows
        .iter()
        .map(|r| {
            let name = r.get::<String, _>("name");
            Collation {
                is_default: name.eq_ignore_ascii_case("binary"),
                name,
                charset: String::new(),
            }
        })
        .collect())
}

/// What every table in the file weighs.
///
/// Views are left out, as they are on the other engines: a view stores nothing of its own and would
/// read here as an empty table.
///
/// The row counts are exact and cost a scan each. That is the trade SQLite forces — there is no
/// stored estimate to read — and it is affordable for the same reason the engine is: the database
/// is a local file rather than a server being asked on someone else's behalf.
pub async fn table_stats(pool: &SqlitePool) -> Result<Vec<TableStats>, AppError> {
    let tables: Vec<String> = sqlx::query_scalar(
        r"select name from sqlite_master
          where type = 'table' and name not like 'sqlite\_%' escape '\'
          order by name",
    )
    .fetch_all(pool)
    .await
    .map_err(map_error)?;

    let sizes = page_sizes(pool).await.unwrap_or_default();
    let index_owners = index_owners(pool).await.unwrap_or_default();

    let mut index_sizes: HashMap<String, u64> = HashMap::new();
    for (index, table) in &index_owners {
        *index_sizes.entry(table.clone()).or_default() += sizes.get(index).copied().unwrap_or(0);
    }

    let mut stats = Vec::with_capacity(tables.len());
    for name in tables {
        let rows: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "select count(*) from {}",
            quote_ident(&name)
        )))
        .fetch_one(pool)
        .await
        .map_err(map_error)?;
        let rows = rows.max(0) as u64;
        let data_size = sizes.get(&name).copied().unwrap_or(0);

        stats.push(TableStats {
            index_size: index_sizes.get(&name).copied().unwrap_or(0),
            avg_record_size: data_size.checked_div(rows).unwrap_or(0),
            data_size,
            rows,
            name,
        });
    }
    Ok(stats)
}

/// How many bytes of the file each table and index occupies, by name.
///
/// `dbstat` walks the whole file, so this is one read for every table rather than one per table.
/// A failure is the caller's to swallow: it means this SQLite was built without the virtual table,
/// and sizes are the one part of the tab that can be missing without the rest being wrong.
pub(super) async fn page_sizes(pool: &SqlitePool) -> Result<HashMap<String, u64>, AppError> {
    let rows = sqlx::query("select name, sum(pgsize) as bytes from dbstat group by name")
        .fetch_all(pool)
        .await
        .map_err(map_error)?;
    Ok(rows
        .iter()
        .map(|r| {
            (
                r.get::<String, _>("name"),
                r.get::<i64, _>("bytes").max(0) as u64,
            )
        })
        .collect())
}

/// Which table each index belongs to.
async fn index_owners(pool: &SqlitePool) -> Result<Vec<(String, String)>, AppError> {
    let rows = sqlx::query("select name, tbl_name from sqlite_master where type = 'index'")
        .fetch_all(pool)
        .await
        .map_err(map_error)?;
    Ok(rows
        .iter()
        .map(|r| (r.get::<String, _>("name"), r.get::<String, _>("tbl_name")))
        .collect())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlineColumn {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    /// `PRI`, `UNI`, `MUL` or empty, as MySQL spells it — the completion list draws either.
    pub key: String,
    /// The `table.column` this one points at, when it is a foreign key.
    pub references: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlineTable {
    pub name: String,
    pub columns: Vec<OutlineColumn>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaOutline {
    /// The database this describes, so a cached outline can be told apart from the one asked for.
    pub database: String,
    pub tables: Vec<OutlineTable>,
}

/// Every table and column of the file, for the Query tab's completion.
///
/// Views are in it as well as tables — their columns complete like any others'.
pub async fn schema_outline(pool: &SqlitePool, database: &str) -> Result<SchemaOutline, AppError> {
    let names: Vec<String> = sqlx::query_scalar(
        r"select name from sqlite_master
          where type in ('table', 'view') and name not like 'sqlite\_%' escape '\'
          order by name",
    )
    .fetch_all(pool)
    .await
    .map_err(map_error)?;

    let mut tables = Vec::with_capacity(names.len());
    for name in names {
        /* One read per table, which is what SQLite offers: `pragma_table_xinfo` answers about one
        table, and there is no catalogue view listing every column of every table the way
        `information_schema.columns` does. A schema large enough for that to hurt is not a shape
        SQLite is usually put in. */
        let columns = sqlx::query("select name, type, \"notnull\", pk from pragma_table_xinfo(?)")
            .bind(&name)
            .fetch_all(pool)
            .await
            .map_err(map_error)?;
        let keys = foreign_key_targets(pool, &name).await.unwrap_or_default();
        let indexes = table_indexes(pool, &name).await.unwrap_or_default();

        tables.push(OutlineTable {
            columns: columns
                .iter()
                .map(|c| {
                    let column = c.get::<String, _>("name");
                    OutlineColumn {
                        key: key_for(&column, c.get::<i64, _>("pk"), &indexes),
                        references: keys.get(&column).cloned(),
                        name: column,
                        data_type: c.get::<String, _>("type"),
                        nullable: c.get::<i64, _>("notnull") == 0,
                    }
                })
                .collect(),
            name,
        });
    }

    Ok(SchemaOutline {
        database: database.to_string(),
        tables,
    })
}

/// Where each foreign key column of a table points, as `table.column`.
async fn foreign_key_targets(
    pool: &SqlitePool,
    table: &str,
) -> Result<BTreeMap<String, String>, AppError> {
    let rows = sqlx::query("select \"table\", \"from\", \"to\" from pragma_foreign_key_list(?)")
        .bind(table)
        .fetch_all(pool)
        .await
        .map_err(map_error)?;
    Ok(rows
        .iter()
        .map(|r| {
            let target = r.get::<String, _>("table");
            let column = r
                .get::<Option<String>, _>("to")
                .unwrap_or_else(|| "rowid".to_string());
            (r.get::<String, _>("from"), format!("{target}.{column}"))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::super::sqlite::tests::Fixture;
    use super::*;

    #[tokio::test]
    async fn a_rowid_key_is_shown_as_an_index_it_does_not_have() {
        let (_fixture, pool) = Fixture::open().await;
        let structure = table_structure(&pool, "post").await.unwrap();

        /* There is no index behind an INTEGER PRIMARY KEY — the table is stored in rowid order —
        so `pragma_index_list` never mentions it. Without the synthesised row the Structure tab
        would mark `id` as PRI and then list no primary key at all. */
        let primary = &structure.indexes[0];
        assert_eq!(primary.name, IMPLICIT_PRIMARY_KEY);
        assert!(primary.primary && primary.unique);
        assert_eq!(primary.columns[0].name.as_deref(), Some("id"));
    }

    #[tokio::test]
    async fn a_real_primary_key_index_is_the_one_reported() {
        let (_fixture, pool) = Fixture::open().await;
        // `tag`'s key is two columns, so SQLite does build an index for it — and that one, not a
        // synthesised stand-in, is what the tab shows.
        let structure = table_structure(&pool, "tag").await.unwrap();
        let primary = &structure.indexes[0];
        assert!(primary.primary);
        assert!(primary.name.starts_with("sqlite_autoindex_tag"));
        assert_eq!(
            primary
                .columns
                .iter()
                .filter_map(|c| c.name.as_deref())
                .collect::<Vec<_>>(),
            vec!["id", "label"]
        );
    }

    #[tokio::test]
    async fn the_primary_key_comes_first_and_the_rest_by_name() {
        let (_fixture, pool) = Fixture::open().await;
        let structure = table_structure(&pool, "post").await.unwrap();
        let names: Vec<&str> = structure
            .indexes
            .iter()
            .map(|index| index.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec![IMPLICIT_PRIMARY_KEY, "post_author", "post_title"]
        );
    }

    #[tokio::test]
    async fn a_column_says_which_kind_of_key_it_leads() {
        let (_fixture, pool) = Fixture::open().await;
        let structure = table_structure(&pool, "post").await.unwrap();
        let key = |name: &str| {
            structure
                .columns
                .iter()
                .find(|c| c.name == name)
                .unwrap_or_else(|| panic!("no column {name}"))
                .key
                .clone()
        };
        assert_eq!(key("id"), "PRI");
        // Read off the indexes, since SQLite has no per-column flag for it.
        assert_eq!(key("title"), "UNI");
        assert_eq!(key("author_id"), "MUL");
        assert_eq!(key("body"), "");
    }

    #[tokio::test]
    async fn a_literal_default_loses_its_quotes_and_an_expression_keeps_its_shape() {
        let (_fixture, pool) = Fixture::open().await;
        let structure = table_structure(&pool, "author").await.unwrap();
        let bio = structure.columns.iter().find(|c| c.name == "bio").unwrap();
        // `DEFAULT 'anonymous'` arrives from SQLite still quoted; the grid shows the value.
        assert_eq!(bio.default_value.as_deref(), Some("anonymous"));
        assert!(!bio.default_is_expression);

        let post = table_structure(&pool, "post").await.unwrap();
        let created = post
            .columns
            .iter()
            .find(|c| c.name == "created_at")
            .unwrap();
        assert_eq!(created.default_value.as_deref(), Some("CURRENT_TIMESTAMP"));
        // Without the mark, this and the *text* "CURRENT_TIMESTAMP" would read alike.
        assert!(created.default_is_expression);

        let views = post.columns.iter().find(|c| c.name == "views").unwrap();
        assert_eq!(views.default_value.as_deref(), Some("0"));
        assert!(!views.default_is_expression);
    }

    #[tokio::test]
    async fn the_structure_tab_sees_the_generated_column() {
        let (_fixture, pool) = Fixture::open().await;
        let structure = table_structure(&pool, "post").await.unwrap();
        let slug = structure.columns.iter().find(|c| c.name == "slug").unwrap();
        assert!(slug.generated);
        assert!(!slug.auto_increment);
    }

    #[tokio::test]
    async fn the_three_built_in_collations_are_read_from_the_engine() {
        let (_fixture, pool) = Fixture::open().await;
        let collations = collations(&pool).await.unwrap();
        let names: Vec<&str> = collations.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"BINARY") && names.contains(&"NOCASE") && names.contains(&"RTRIM"));
        // BINARY is what a column gets without a COLLATE.
        assert!(
            collations
                .iter()
                .find(|c| c.name == "BINARY")
                .unwrap()
                .is_default
        );
    }

    #[tokio::test]
    async fn stats_count_the_rows_and_leave_the_views_out() {
        let (_fixture, pool) = Fixture::open().await;
        let stats = table_stats(&pool).await.unwrap();
        let names: Vec<&str> = stats.iter().map(|s| s.name.as_str()).collect();
        // `recent` is a view — it stores nothing of its own — and `sqlite_sequence` is SQLite's.
        assert_eq!(names, vec!["author", "loose", "post", "tag"]);
        assert_eq!(stats.iter().find(|s| s.name == "post").unwrap().rows, 3);
    }

    #[tokio::test]
    async fn stats_report_a_size_because_the_engine_is_bundled_with_dbstat() {
        let (_fixture, pool) = Fixture::open().await;
        let stats = table_stats(&pool).await.unwrap();
        let post = stats.iter().find(|s| s.name == "post").unwrap();
        /* `dbstat` is a compile-time option, and this asserts the one MixLab ships with — see the
        `libsqlite3-sys` entry in Cargo.toml. If this ever fails, the build has stopped bundling
        its own SQLite and the Statistics tab has quietly gone to zeroes. */
        assert!(post.data_size > 0, "no data size: dbstat is missing");
        assert!(post.index_size > 0, "no index size: post has two indexes");
        assert!(post.avg_record_size > 0);
    }

    #[tokio::test]
    async fn the_outline_carries_every_table_with_its_keys() {
        let (_fixture, pool) = Fixture::open().await;
        let outline = schema_outline(&pool, "main").await.unwrap();
        assert_eq!(outline.database, "main");

        let names: Vec<&str> = outline.tables.iter().map(|t| t.name.as_str()).collect();
        // Views complete like tables do.
        assert_eq!(names, vec!["author", "loose", "post", "recent", "tag"]);

        let post = outline.tables.iter().find(|t| t.name == "post").unwrap();
        let author_id = post.columns.iter().find(|c| c.name == "author_id").unwrap();
        assert_eq!(author_id.references.as_deref(), Some("author.id"));
        assert_eq!(author_id.key, "MUL");
        assert!(!author_id.nullable);
    }
}
