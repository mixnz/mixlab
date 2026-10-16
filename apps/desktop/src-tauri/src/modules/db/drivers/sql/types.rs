//! The shapes the SQL engines send to the window and read from it, one Rust type per TypeScript
//! type in `src/modules/db/types.ts` and `sql/api.ts`.
//!
//! The window reads one shape whatever the engine, so these are written once. An engine that
//! reads less of an input type than is here converts it into a type of its own; see each engine's
//! `ColumnSpec` and `IndexSpec`.

use crate::modules::db::drivers::clickhouse::structure::SkipIndex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// The row a foreign key column points at: what it references, not what it is declared as.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignKey {
    pub table: String,
    pub column: String,
}

/// What is known about one column beyond its name: everything a new row has to respect. That is
/// the type it is written as, whether it may be left NULL, and what it falls back to when an
/// INSERT leaves it out.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnMeta {
    /// The declared type as the server spells it, e.g. `varchar(255)` or `int unsigned`.
    pub data_type: String,
    pub nullable: bool,
    /// The column's DEFAULT, or `None` when it has none. An expression default is reported here
    /// too, unquoted, and `extra` is what tells it from a literal where the engine can say.
    pub default_value: Option<String>,
    /// The engine's own remarks on the column: `auto_increment`, `DEFAULT_GENERATED`, …
    pub extra: String,
    /// What this column references, when it is part of a foreign key. ClickHouse has no foreign
    /// keys and always leaves this `None`.
    pub foreign_key: Option<ForeignKey>,
}

/// One condition on the rows a page is cut out of. The grid's filter bar sends a list of these,
/// and they are ANDed together. `value` carries whatever the user typed, as text: the operator is
/// what says how to read it (a single value, a comma-separated list, a pair), and operators like
/// `isNull` ignore it entirely.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    pub column: String,
    pub operator: String,
    #[serde(default)]
    pub value: Option<String>,
}

/// Which page of a table is wanted, and what it is cut out of: the page itself, the order the
/// whole table is put in first, and the filters that narrow it down before either.
///
/// One value rather than five loose arguments: `page` and `page_size` are both `i64` and
/// `sort_desc` is a bare `bool`, so a positional call is one transposition away from silently
/// reading the wrong page in the wrong order.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageQuery {
    pub page: i64,
    pub page_size: i64,
    /// Ignored unless it names a real column of this table.
    #[serde(default)]
    pub sort_column: Option<String>,
    #[serde(default)]
    pub sort_desc: bool,
    /// ANDed together; `total` counts what is left after them.
    #[serde(default)]
    pub filters: Vec<Filter>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TablePage {
    pub columns: Vec<String>,
    /// Keyed by column name; `columns` is what carries their order.
    pub column_meta: BTreeMap<String, ColumnMeta>,
    pub primary_key: Vec<String>,
    /// The auto-increment column, if the table has one. Only such a table has a counter worth
    /// offering to reset after a delete.
    pub auto_increment_column: Option<String>,
    pub rows: Vec<Map<String, Value>>,
    pub total: i64,
}

/// One column as the table currently declares it. Carries the parts a definition is written from
/// plus the read-only facts around them: which key it belongs to, what the server computes itself.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureColumn {
    pub name: String,
    /// The full declared type as the server spells it: `varchar(255)`, `enum('a','b')`.
    pub data_type: String,
    pub nullable: bool,
    /// The column's DEFAULT, or `None` when it has none, which is also how `DEFAULT NULL` reads:
    /// the two are the same thing to a nullable column.
    pub default_value: Option<String>,
    /// Whether the default above is an expression (`uuid()`) rather than a literal.
    pub default_is_expression: bool,
    pub auto_increment: bool,
    pub on_update_current_timestamp: bool,
    /// A column the server computes from the others. Its expression is not read, so the editor
    /// offers no way to change one, only to drop it.
    pub generated: bool,
    pub collation: Option<String>,
    pub comment: String,
    /// `PRI`, `UNI`, `MUL` or empty: which kind of key this column is the leading column of.
    pub key: String,
    /// The engine's own remarks, verbatim, so nothing the fields above don't model disappears from
    /// the grid.
    pub extra: String,
}

/// One collation the connected server actually has, with the character set it belongs to. Read
/// from the server rather than listed here, because which collations exist depends on the version
/// and on how the server was built.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Collation {
    pub name: String,
    pub charset: String,
    /// Whether this is the character set's default: the collation a column of that charset gets
    /// when no `COLLATE` is written.
    pub is_default: bool,
}

/// One column of an index, in the order the index puts them in.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexColumn {
    /// `None` for a functional index, which indexes an expression rather than a column.
    pub name: Option<String>,
    /// How many leading characters are indexed, when only a prefix of the column is.
    pub prefix_length: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableIndex {
    pub name: String,
    pub unique: bool,
    pub primary: bool,
    /// How the index is stored or what kind it is, as the server reports it: `BTREE`, `HASH`,
    /// `FULLTEXT`, `CLUSTERED`, …
    pub index_type: String,
    pub columns: Vec<IndexColumn>,
    pub comment: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableStructure {
    /// In table order, which is the order a `SELECT *` returns them in.
    pub columns: Vec<StructureColumn>,
    /// The primary key first, then the rest.
    pub indexes: Vec<TableIndex>,
    /// Empty on every engine but ClickHouse, the only one with data skipping indexes. See
    /// `docs/specs/2026-09-04-clickhouse-index-ddl-design.md`.
    pub skip_indexes: Vec<SkipIndex>,
    /// The table engine, read only for ClickHouse; `None` on the others.
    pub engine: Option<String>,
}

/// One column as completion needs to know it: enough to offer the name and to say what it is,
/// and nothing of what an editor would need to change it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlineColumn {
    pub name: String,
    /// The declared type as the server spells it.
    pub data_type: String,
    pub nullable: bool,
    /// `PRI`, `UNI`, `MUL` or empty: which kind of key this column leads.
    pub key: String,
    /// `table.column` this one points at, when it is a foreign key. Shown beside the column in the
    /// completion list, which is where a join is usually being written.
    pub references: Option<String>,
}

/// One table, with its columns in table order.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlineTable {
    pub name: String,
    pub columns: Vec<OutlineColumn>,
}

/// Every table and column of one database in a single payload.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaOutline {
    /// The database this describes, so a cached outline can be told apart from the one asked for.
    pub database: String,
    pub tables: Vec<OutlineTable>,
}

/// What one table costs the server: the rows it holds and the bytes they and their indexes take.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableStats {
    pub name: String,
    /// The server's own figure. Most engines keep an estimate rather than a `COUNT(*)`, and it can
    /// be well off on a large table, as can the average size derived alongside it.
    pub rows: u64,
    /// The bytes the rows themselves take.
    pub data_size: u64,
    /// The bytes every index on the table takes together.
    pub index_size: u64,
    /// The average bytes per row as the engine reports it.
    pub avg_record_size: u64,
}

/// What a column is to be declared as: the write-side counterpart of [`StructureColumn`], holding
/// only the parts that go into a definition. This is everything the window sends; each engine
/// converts it into a `ColumnSpec` of its own holding the fields it reads.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnSpec {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    /// `None` writes no DEFAULT clause at all.
    #[serde(default)]
    pub default_value: Option<String>,
    /// Emits the default as an expression rather than as a quoted literal.
    #[serde(default)]
    pub default_is_expression: bool,
    #[serde(default)]
    pub auto_increment: bool,
    #[serde(default)]
    pub on_update_current_timestamp: bool,
    #[serde(default)]
    pub collation: Option<String>,
    #[serde(default)]
    pub comment: String,
    /// Where the column goes: `None` leaves it where it is (or appends a new one), `Some("")`
    /// puts it first, and `Some(name)` puts it directly after that column. MySQL alone reads it.
    #[serde(default)]
    pub after: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexColumnSpec {
    pub name: String,
    /// Indexes only the first `n` characters of the column. MySQL alone reads it.
    #[serde(default)]
    pub prefix_length: Option<i64>,
}

/// What an index is to be created as. Each engine converts it into an `IndexSpec` of its own.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexSpec {
    /// Left empty, the engine names the index itself.
    #[serde(default)]
    pub name: String,
    /// `index`, `unique`, `fulltext`, `spatial` or `primary`, as far as the engine has them.
    pub kind: String,
    #[serde(default)]
    pub index_type: Option<String>,
    pub columns: Vec<IndexColumnSpec>,
    #[serde(default)]
    pub comment: String,
}

#[cfg(test)]
mod tests;
