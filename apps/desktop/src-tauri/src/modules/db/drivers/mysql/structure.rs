//! The shape of a table rather than its rows: what the Structure tab reads about a table's
//! columns and indexes, and the `ALTER TABLE` statements its editors write back.
//!
//! Everything here goes through `information_schema`, which only ever shows what the connected
//! user has privileges on — so a short list means "this is what you may see", not necessarily
//! "this is all there is".

use super::{map_error, quote_ident};
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use sqlx::mysql::MySqlRow;
use sqlx::{MySqlPool, Row};
use std::collections::{HashMap, HashSet};

/// Reads a text column that `information_schema` may hand over as bytes rather than as characters
/// (`COLUMN_DEFAULT` is a blob-backed column on some servers), and reports an absent value as
/// `None` either way.
fn text(row: &MySqlRow, name: &str) -> Option<String> {
    if let Ok(value) = row.try_get::<Option<String>, _>(name) {
        return value;
    }
    row.try_get::<Option<Vec<u8>>, _>(name)
        .ok()
        .flatten()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

fn text_or_empty(row: &MySqlRow, name: &str) -> String {
    text(row, name).unwrap_or_default()
}

/// One column as the table currently declares it. Carries the parts a definition is written from
/// plus the read-only facts around them — which key it belongs to, what MySQL computes itself.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureColumn {
    pub name: String,
    /// The full declared type as MySQL spells it: `varchar(255)`, `int unsigned`, `enum('a','b')`.
    pub data_type: String,
    pub nullable: bool,
    /// The column's DEFAULT, or `None` when it has none — which is also how `DEFAULT NULL` reads,
    /// the two being the same thing to a nullable column.
    pub default_value: Option<String>,
    /// Whether the default above is an expression (`uuid()`) rather than a literal. MySQL 8 is
    /// what reports this; on 5.7 only the `CURRENT_TIMESTAMP` family is recognisable, and that
    /// one is recognised from the text itself. MariaDB reports it nowhere, and is read a different
    /// way entirely — see [`mariadb_default`].
    pub default_is_expression: bool,
    pub auto_increment: bool,
    pub on_update_current_timestamp: bool,
    /// A column MySQL computes from the others. Its expression is not read here, so the editor
    /// offers no way to change one — only to drop it.
    pub generated: bool,
    pub collation: Option<String>,
    pub comment: String,
    /// `PRI`, `UNI`, `MUL` or empty: which kind of key this column is the leading column of.
    pub key: String,
    /// `SHOW COLUMNS`' Extra, verbatim — shown as-is so nothing the fields above don't model
    /// disappears from the grid.
    pub extra: String,
}

/// One collation the connected server actually has, with the character set it belongs to.
///
/// Read from the server rather than listed here, because which collations exist is a property of
/// the version and of how the server was built: `utf8mb4_0900_ai_ci` only exists from MySQL 8.0,
/// `utf8mb3_*` is spelled `utf8_*` before it, and MariaDB has a set of its own.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Collation {
    pub name: String,
    pub charset: String,
    /// Whether this is the character set's default — the collation a column of that charset gets
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
    /// `BTREE`, `HASH`, `FULLTEXT` or `SPATIAL` as MySQL reports it — the last two are a kind of
    /// index rather than a way of storing one, but `information_schema` puts them in this column.
    pub index_type: String,
    pub columns: Vec<IndexColumn>,
    pub comment: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableStructure {
    /// In table order, which is the order a `SELECT *` returns them in.
    pub columns: Vec<StructureColumn>,
    /// The primary key first, then the rest as `information_schema` lists them.
    pub indexes: Vec<TableIndex>,
    /// Always empty: data skipping indices are a ClickHouse-only concept. See
    /// `docs/specs/2026-09-04-clickhouse-index-ddl-design.md`.
    pub skip_indexes: Vec<super::super::clickhouse::structure::SkipIndex>,
    /// Always `None` — the engine guard in the Structure tab only ever reads this for ClickHouse.
    pub engine: Option<String>,
}

/// What a column is to be declared as. The write-side counterpart of {@link StructureColumn}:
/// only the parts that go into a definition, since the rest is not the editor's to set.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnSpec {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    /// `None` writes no DEFAULT clause at all.
    #[serde(default)]
    pub default_value: Option<String>,
    /// Emits the default as an expression rather than as a quoted literal. The
    /// `CURRENT_TIMESTAMP` family is recognised without it; this is for the rest (`uuid()`).
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
    /// puts it first, and `Some(name)` puts it directly after that column.
    #[serde(default)]
    pub after: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexColumnSpec {
    pub name: String,
    /// Indexes only the first `n` characters of the column. Required for a `TEXT`/`BLOB` column,
    /// which cannot be indexed whole.
    #[serde(default)]
    pub prefix_length: Option<i64>,
}

/// What an index is to be created as.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexSpec {
    /// Left empty, MySQL names the index after its first column. Ignored for a primary key,
    /// which is always called `PRIMARY`.
    #[serde(default)]
    pub name: String,
    /// `index`, `unique`, `fulltext`, `spatial` or `primary`.
    pub kind: String,
    /// `BTREE` or `HASH`, or `None` for the storage engine's own default. Only meaningful for a
    /// plain or unique index — `FULLTEXT` and `SPATIAL` have one structure each.
    #[serde(default)]
    pub index_type: Option<String>,
    pub columns: Vec<IndexColumnSpec>,
    #[serde(default)]
    pub comment: String,
}

/// Wraps text as a SQL string literal, escaping what would otherwise end it early.
fn quote_string(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "''"))
}

/// Reads a SQL string literal back to the text it stands for, or `None` when the value is not one.
///
/// The inverse of [`quote_string`], and then some: this reads what a server wrote rather than what
/// this app did, so a quote inside the literal may arrive doubled (`''`) or backslash-escaped, and
/// the rest of MySQL's backslash escapes may appear alongside it.
fn unquote_string(value: &str) -> Option<String> {
    let inner = value.strip_prefix('\'')?.strip_suffix('\'')?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            // The closing quote is already gone, so a quote in here can only be the first half of
            // a doubled one.
            '\'' => {
                chars.next();
                out.push('\'');
            }
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('0') => out.push('\0'),
                Some('b') => out.push('\u{8}'),
                Some('Z') => out.push('\u{1a}'),
                // `\\`, `\'`, `\"` — and anything else, which is MySQL's own rule for an escape
                // it does not recognise: the character stands for itself.
                Some(other) => out.push(other),
                None => out.push('\\'),
            },
            _ => out.push(c),
        }
    }
    Some(out)
}

/// What MariaDB means by the text in `COLUMN_DEFAULT`, which is not what MySQL means by it.
///
/// MySQL reports the default's *value* — `abc` for `DEFAULT 'abc'`, SQL NULL for a column with no
/// default — and puts `DEFAULT_GENERATED` in `EXTRA` when that value is really an expression.
/// MariaDB reports the default's *source text* instead, as it would be written in the DDL, and
/// marks nothing in `EXTRA`:
///
/// | declared          | MariaDB reports          |
/// | ----------------- | ------------------------ |
/// | `DEFAULT 'abc'`   | `'abc'`                  |
/// | `DEFAULT 'a''b'`  | `'a''b'`                 |
/// | `DEFAULT 'NULL'`  | `'NULL'`                 |
/// | `DEFAULT 7`       | `7`                      |
/// | `DEFAULT (1 + 1)` | `(1 + 1)`                |
/// | `DEFAULT NULL`    | `NULL`                   |
/// | no default        | `NULL`, the same text    |
///
/// So a quoted literal is unquoted back to its value, the bare word `NULL` is read as no default —
/// the two rows it stands for mean the same thing to a nullable column, and a column that is not
/// nullable reports SQL NULL instead — and whatever is left is an expression, which is the same
/// distinction `DEFAULT_GENERATED` draws on MySQL. Taken untranslated, every one of these would
/// reach the Structure tab wrong: a default of `'abc'` with the quotes in it, and a `NULL` shown
/// on every nullable column that has no default at all.
fn mariadb_default(reported: Option<String>) -> (Option<String>, bool) {
    let Some(reported) = reported else {
        return (None, false);
    };
    let trimmed = reported.trim();
    if trimmed == "NULL" {
        return (None, false);
    }
    if let Some(literal) = unquote_string(trimmed) {
        return (Some(literal), false);
    }
    // A number is the only other literal MariaDB writes unquoted, so anything left that is not one
    // is an expression.
    if trimmed.parse::<f64>().is_ok() {
        return (Some(trimmed.to_string()), false);
    }
    (Some(trimmed.to_string()), true)
}

/// Which of `table`'s columns default to an expression rather than to a literal, on MariaDB.
///
/// The grid reads a column's default from `SHOW COLUMNS`, which both servers answer the same way —
/// already unquoted, the value itself. What only MySQL adds is `DEFAULT_GENERATED` in `EXTRA`,
/// saying that what it just reported is an expression to be evaluated rather than text to be
/// stored. Without it a column declared `DEFAULT (uuid())` starts a new row off at the six
/// characters `uuid()`, and the row is written with them — the wrong value, and no error to say
/// so. `information_schema` is the only place MariaDB still distinguishes the two, in the source
/// text [`mariadb_default`] reads, so it is read alongside and the marker put in by hand.
pub async fn mariadb_expression_defaults(
    conn: &mut sqlx::MySqlConnection,
    database: &str,
    table: &str,
) -> Result<HashSet<String>, AppError> {
    let rows = sqlx::query(
        "SELECT COLUMN_NAME, COLUMN_DEFAULT
         FROM information_schema.COLUMNS
         WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?",
    )
    .bind(database)
    .bind(table)
    .fetch_all(&mut *conn)
    .await
    .map_err(map_error)?;

    Ok(rows
        .iter()
        .filter(|row| mariadb_default(text(row, "COLUMN_DEFAULT")).1)
        .map(|row| text_or_empty(row, "COLUMN_NAME"))
        .collect())
}

/// The functions a DEFAULT may name without being parenthesised — the only expressions a server
/// older than 8.0 can have, and the ones a user types without thinking of them as expressions.
fn is_bare_default_function(upper: &str) -> bool {
    upper.starts_with("CURRENT_TIMESTAMP")
        || upper.starts_with("NOW(")
        || upper == "CURRENT_DATE"
        || upper == "CURRENT_TIME"
        || upper == "CURDATE()"
        || upper == "CURTIME()"
}

/// Whether the column is one of the date/time types, which is what makes a bare
/// `CURRENT_TIMESTAMP` in its default unambiguous: no temporal column can hold those characters as
/// text, whereas a `varchar` perfectly well can.
fn is_temporal_type(data_type: &str) -> bool {
    let lower = data_type.trim().to_lowercase();
    ["timestamp", "datetime", "date", "time", "year"]
        .iter()
        .any(|kind| lower.starts_with(kind))
}

/// How a DEFAULT reaches the DDL. A literal is quoted — MySQL reads a quoted number back as the
/// number, so only the expressions need telling apart, and `NULL` typed on its own is meant as
/// SQL NULL rather than as the four characters.
///
/// A temporal column's `CURRENT_TIMESTAMP` is recognised as the function it can only be; on any
/// other column that text is quoted like anything else, and asking for the function there is what
/// `is_expression` is for.
fn default_clause(value: &str, is_expression: bool, data_type: &str) -> String {
    let trimmed = value.trim();
    let upper = trimmed.to_uppercase();
    if is_bare_default_function(&upper) && (is_expression || is_temporal_type(data_type)) {
        return trimmed.to_string();
    }
    if is_expression {
        // MySQL 8 requires an expression default to be parenthesised, and writes them back out
        // that way itself — so one that already is comes through untouched.
        return if trimmed.starts_with('(') {
            trimmed.to_string()
        } else {
            format!("({trimmed})")
        };
    }
    if upper == "NULL" {
        return "NULL".to_string();
    }
    quote_string(value)
}

/// A collation as it may be interpolated into DDL, or `None` when none was asked for.
///
/// A collation name is never a quotable value in MySQL's grammar — it goes in bare — so the only
/// safe thing to do with one is to refuse anything that is not shaped like a name.
fn validated_collation(collation: Option<&str>) -> Result<Option<&str>, AppError> {
    let Some(collation) = collation.map(str::trim).filter(|c| !c.is_empty()) else {
        return Ok(None);
    };
    if !collation
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(err!("error.invalidCollation", collation = collation));
    }
    Ok(Some(collation))
}

/// Turns a spec into the `name type ...` part of an `ADD`/`CHANGE COLUMN` clause.
///
/// The type is interpolated as the user wrote it: MySQL's type grammar is far too large to model,
/// and this client runs user-authored SQL by design. The statement it lands in is a prepared one,
/// which MySQL will not accept more than one statement for, so a `;` in the text cannot become a
/// second statement.
fn column_definition(spec: &ColumnSpec) -> Result<String, AppError> {
    if spec.name.trim().is_empty() {
        return Err(err!("error.columnNameRequired"));
    }
    let data_type = spec.data_type.trim();
    if data_type.is_empty() {
        return Err(err!("error.columnTypeRequired"));
    }

    let mut sql = format!("{} {}", quote_ident(spec.name.trim()), data_type);
    if let Some(collation) = validated_collation(spec.collation.as_deref())? {
        sql.push_str(&format!(" COLLATE {collation}"));
    }
    sql.push_str(if spec.nullable { " NULL" } else { " NOT NULL" });
    if let Some(default) = &spec.default_value {
        sql.push_str(&format!(
            " DEFAULT {}",
            default_clause(default, spec.default_is_expression, data_type)
        ));
    }
    if spec.on_update_current_timestamp {
        sql.push_str(" ON UPDATE CURRENT_TIMESTAMP");
    }
    if spec.auto_increment {
        sql.push_str(" AUTO_INCREMENT");
    }
    let comment = spec.comment.trim();
    if !comment.is_empty() {
        sql.push_str(&format!(" COMMENT {}", quote_string(comment)));
    }
    Ok(sql)
}

/// The trailing `FIRST`/`AFTER x` of a column clause, empty when the column stays where it is.
fn position_clause(after: Option<&str>) -> String {
    match after {
        None => String::new(),
        Some("") => " FIRST".to_string(),
        Some(name) => format!(" AFTER {}", quote_ident(name)),
    }
}

/// The `ADD ...` clause that creates an index, minus the `ALTER TABLE` in front of it.
fn add_index_clause(spec: &IndexSpec) -> Result<String, AppError> {
    if spec.columns.is_empty() {
        return Err(err!("error.indexNeedsColumn"));
    }

    let columns = spec
        .columns
        .iter()
        .map(|column| {
            if column.name.trim().is_empty() {
                return Err(err!("error.indexColumnNameRequired"));
            }
            let quoted = quote_ident(column.name.trim());
            Ok(match column.prefix_length {
                Some(length) if length > 0 => format!("{quoted}({length})"),
                _ => quoted,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?
        .join(", ");

    let kind = spec.kind.to_lowercase();
    if kind == "primary" {
        // A primary key carries no name of its own, and MySQL accepts neither a comment nor a
        // USING clause on the `ADD PRIMARY KEY` form.
        return Ok(format!("ADD PRIMARY KEY ({columns})"));
    }

    let keyword = match kind.as_str() {
        "index" => "INDEX",
        "unique" => "UNIQUE INDEX",
        "fulltext" => "FULLTEXT INDEX",
        "spatial" => "SPATIAL INDEX",
        other => return Err(err!("error.unknownIndexKind", kind = other)),
    };
    let name = spec.name.trim();
    let named = if name.is_empty() {
        // Left unnamed, MySQL names the index after its first column — which is what a client
        // that made one up would have had to guess at anyway.
        String::new()
    } else {
        format!(" {}", quote_ident(name))
    };

    let mut clause = format!("ADD {keyword}{named} ({columns})");
    // Only the two general-purpose kinds have a choice of structure; FULLTEXT and SPATIAL each
    // have exactly one, and MySQL rejects a USING clause on them.
    if kind == "index" || kind == "unique" {
        if let Some(index_type) = spec
            .index_type
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            let upper = index_type.to_uppercase();
            if upper != "BTREE" && upper != "HASH" {
                return Err(err!("error.unknownIndexType", type = index_type));
            }
            clause.push_str(&format!(" USING {upper}"));
        }
    }
    let comment = spec.comment.trim();
    if !comment.is_empty() {
        clause.push_str(&format!(" COMMENT {}", quote_string(comment)));
    }
    Ok(clause)
}

/// The `DROP ...` clause that removes an index by name.
fn drop_index_clause(name: &str) -> String {
    if name == "PRIMARY" {
        // The primary key has no name to drop it by, only its own keyword.
        "DROP PRIMARY KEY".to_string()
    } else {
        format!("DROP INDEX {}", quote_ident(name))
    }
}

fn qualified(database: &str, table: &str) -> String {
    format!("{}.{}", quote_ident(database), quote_ident(table))
}

/// Runs one statement built here. Prepared rather than sent as text, so that MySQL itself refuses
/// anything that turns out to be more than a single statement.
async fn execute(pool: &MySqlPool, sql: String) -> Result<(), AppError> {
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(map_error)
}

/// `mariadb` says which of the two servers answered, because a column's DEFAULT is the one thing
/// they report differently — see [`mariadb_default`].
pub async fn table_structure(
    pool: &MySqlPool,
    mariadb: bool,
    database: &str,
    table: &str,
) -> Result<TableStructure, AppError> {
    let mut conn = pool.acquire().await.map_err(map_error)?;

    let column_rows = sqlx::query(
        "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_DEFAULT, EXTRA, COLUMN_COMMENT,
                COLUMN_KEY, COLLATION_NAME
         FROM information_schema.COLUMNS
         WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?
         ORDER BY ORDINAL_POSITION",
    )
    .bind(database)
    .bind(table)
    .fetch_all(&mut *conn)
    .await
    .map_err(map_error)?;

    if column_rows.is_empty() {
        return Err(err!(
            "error.noVisibleColumns",
            database = database,
            table = table
        ));
    }

    let columns = column_rows
        .iter()
        .map(|row| {
            let extra = text_or_empty(row, "EXTRA");
            let extra_lower = extra.to_lowercase();
            // `DEFAULT_GENERATED` is MySQL 8's marker for an expression default. The
            // `CURRENT_TIMESTAMP` family carries it too, but is recognised from its own text
            // wherever it appears, so it needs nothing from here. MariaDB marks nothing at all and
            // says it in the default's own text instead.
            let (default_value, default_is_expression) = if mariadb {
                mariadb_default(text(row, "COLUMN_DEFAULT"))
            } else {
                (
                    text(row, "COLUMN_DEFAULT"),
                    extra_lower.contains("default_generated"),
                )
            };
            StructureColumn {
                name: text_or_empty(row, "COLUMN_NAME"),
                data_type: text_or_empty(row, "COLUMN_TYPE"),
                nullable: text_or_empty(row, "IS_NULLABLE").eq_ignore_ascii_case("YES"),
                default_value,
                default_is_expression,
                auto_increment: extra_lower.contains("auto_increment"),
                on_update_current_timestamp: extra_lower.contains("on update current_timestamp"),
                // Matched on the two full phrases rather than on "generated" alone, which
                // `DEFAULT_GENERATED` also contains.
                generated: extra_lower.contains("virtual generated")
                    || extra_lower.contains("stored generated"),
                collation: text(row, "COLLATION_NAME"),
                comment: text_or_empty(row, "COLUMN_COMMENT"),
                key: text_or_empty(row, "COLUMN_KEY"),
                extra,
            }
        })
        .collect();

    // `NON_UNIQUE` and `SUB_PART` are typed differently across MySQL versions (signed vs
    // unsigned, int vs bigint), so both are cast to one decodable type by the server.
    let index_rows = sqlx::query(
        "SELECT INDEX_NAME, CAST(NON_UNIQUE AS SIGNED) AS non_unique, COLUMN_NAME,
                CAST(SUB_PART AS SIGNED) AS sub_part, INDEX_TYPE, INDEX_COMMENT
         FROM information_schema.STATISTICS
         WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?
         ORDER BY INDEX_NAME, SEQ_IN_INDEX",
    )
    .bind(database)
    .bind(table)
    .fetch_all(&mut *conn)
    .await
    .map_err(map_error)?;

    let mut indexes: Vec<TableIndex> = Vec::new();
    for row in &index_rows {
        let name = text_or_empty(row, "INDEX_NAME");
        let column = IndexColumn {
            // NULL for a functional index, which indexes an expression instead of a column.
            name: text(row, "COLUMN_NAME"),
            prefix_length: row.try_get::<Option<i64>, _>("sub_part").unwrap_or(None),
        };
        // The rows come ordered by index and then by position within it, so each index's columns
        // arrive together and the one being built is always the last.
        match indexes.last_mut() {
            Some(last) if last.name == name => last.columns.push(column),
            _ => indexes.push(TableIndex {
                primary: name == "PRIMARY",
                unique: row.try_get::<i64, _>("non_unique").unwrap_or(1) == 0,
                index_type: text_or_empty(row, "INDEX_TYPE"),
                comment: text_or_empty(row, "INDEX_COMMENT"),
                name,
                columns: vec![column],
            }),
        }
    }
    // The primary key first — it is the one index the table is organised by. `sort_by_key` is
    // stable, so everything else keeps the order it was listed in.
    indexes.sort_by_key(|index| !index.primary);

    Ok(TableStructure {
        columns,
        indexes,
        skip_indexes: Vec::new(),
        engine: None,
    })
}

/// One column as completion needs to know it: enough to offer the name and to say what it is,
/// and nothing of what an editor would need to change it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlineColumn {
    pub name: String,
    /// The declared type as MySQL spells it: `varchar(255)`, `int unsigned`.
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

/// What the Query tab's completion knows about the selected database.
///
/// `table_structure` answers the same question for one table and in far more detail. This is the
/// shape completion needs instead: every table at once, and only what an offered name has to
/// carry. It is read once per database and kept, so the cost is two reads of `information_schema`
/// rather than one per table.
///
/// A table whose columns the connected user may not see simply is not in the list — the same rule
/// as everywhere else in this module. Views come back alongside base tables; a view's columns
/// complete exactly like a table's, so nothing here needs to tell the two apart.
pub async fn schema_outline(pool: &MySqlPool, database: &str) -> Result<SchemaOutline, AppError> {
    let mut conn = pool.acquire().await.map_err(map_error)?;

    // The foreign keys first, so that each column can be built already knowing where it points.
    //
    // A failure here is not one worth failing the whole outline over. `KEY_COLUMN_USAGE` is the
    // one read in this function that a server can refuse or be slow about — it is the heaviest of
    // the `information_schema` views on MySQL 5.7, and a user may hold privileges on the tables
    // without holding them on the constraints. Completion is for the columns; where a foreign key
    // points is a line of detail beside them, and going without it costs nothing else.
    let key_rows = sqlx::query(
        "SELECT TABLE_NAME, COLUMN_NAME, REFERENCED_TABLE_NAME, REFERENCED_COLUMN_NAME
         FROM information_schema.KEY_COLUMN_USAGE
         WHERE TABLE_SCHEMA = ? AND REFERENCED_TABLE_NAME IS NOT NULL",
    )
    .bind(database)
    .fetch_all(&mut *conn)
    .await
    .unwrap_or_default();

    let mut references: HashMap<(String, String), String> = HashMap::new();
    for row in &key_rows {
        let target = match (
            text(row, "REFERENCED_TABLE_NAME"),
            text(row, "REFERENCED_COLUMN_NAME"),
        ) {
            (Some(table), Some(column)) => format!("{table}.{column}"),
            _ => continue,
        };
        references.insert(
            (
                text_or_empty(row, "TABLE_NAME"),
                text_or_empty(row, "COLUMN_NAME"),
            ),
            target,
        );
    }

    let column_rows = sqlx::query(
        "SELECT TABLE_NAME, COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_KEY
         FROM information_schema.COLUMNS
         WHERE TABLE_SCHEMA = ?
         ORDER BY TABLE_NAME, ORDINAL_POSITION",
    )
    .bind(database)
    .fetch_all(&mut *conn)
    .await
    .map_err(map_error)?;

    let mut tables: Vec<OutlineTable> = Vec::new();
    for row in &column_rows {
        let table = text_or_empty(row, "TABLE_NAME");
        let name = text_or_empty(row, "COLUMN_NAME");
        let column = OutlineColumn {
            references: references.get(&(table.clone(), name.clone())).cloned(),
            name,
            data_type: text_or_empty(row, "COLUMN_TYPE"),
            nullable: text_or_empty(row, "IS_NULLABLE").eq_ignore_ascii_case("YES"),
            key: text_or_empty(row, "COLUMN_KEY"),
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

/// Every collation the server supports, in character set order. Read once and offered as a list to
/// choose from, so a column's `COLLATE` can only ever name something this server knows.
pub async fn collations(pool: &MySqlPool) -> Result<Vec<Collation>, AppError> {
    let rows = sqlx::query(
        "SELECT COLLATION_NAME, CHARACTER_SET_NAME, IS_DEFAULT
         FROM information_schema.COLLATIONS
         ORDER BY CHARACTER_SET_NAME, COLLATION_NAME",
    )
    .fetch_all(pool)
    .await
    .map_err(map_error)?;

    Ok(rows
        .iter()
        .map(|row| Collation {
            name: text_or_empty(row, "COLLATION_NAME"),
            charset: text_or_empty(row, "CHARACTER_SET_NAME"),
            // `Yes` on the character set's default, empty on the rest.
            is_default: text_or_empty(row, "IS_DEFAULT").eq_ignore_ascii_case("Yes"),
        })
        .filter(|collation| !collation.name.is_empty())
        .collect())
}

/// What one table costs the server: the rows it holds and the bytes they and their indexes take.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableStats {
    pub name: String,
    /// InnoDB's is an estimate sampled from the index, not a `COUNT(*)` — it can be well off on a
    /// large table, and so can the average size derived alongside it.
    pub rows: u64,
    /// The bytes the rows themselves take, `DATA_LENGTH`.
    pub data_size: u64,
    /// The bytes every index on the table takes together, `INDEX_LENGTH`.
    pub index_size: u64,
    /// The average bytes per row as the engine reports it, `AVG_ROW_LENGTH`.
    pub avg_record_size: u64,
}

/// What every table in the database weighs, listed by name.
///
/// Only base tables are counted. A view stores nothing of its own and `information_schema` reports
/// NULL for all four numbers on one, which would show up here as a table with no rows in it rather
/// than as what it is.
pub async fn table_stats(pool: &MySqlPool, database: &str) -> Result<Vec<TableStats>, AppError> {
    let rows = sqlx::query(
        "SELECT TABLE_NAME, TABLE_ROWS, DATA_LENGTH, INDEX_LENGTH, AVG_ROW_LENGTH
         FROM information_schema.TABLES
         WHERE TABLE_SCHEMA = ? AND TABLE_TYPE = 'BASE TABLE'
         ORDER BY TABLE_NAME",
    )
    .bind(database)
    .fetch_all(pool)
    .await
    .map_err(map_error)?;

    Ok(rows
        .iter()
        .map(|row| TableStats {
            name: text_or_empty(row, "TABLE_NAME"),
            rows: counter(row, "TABLE_ROWS"),
            data_size: counter(row, "DATA_LENGTH"),
            index_size: counter(row, "INDEX_LENGTH"),
            avg_record_size: counter(row, "AVG_ROW_LENGTH"),
        })
        .collect())
}

/// One of `information_schema`'s counters. They are NULL for a table the engine keeps no figure
/// for, which reads the same here as a table with nothing in it: zero. Declared unsigned, but read
/// as signed too — the column types of `information_schema` are not the same on every server, and
/// a mismatch would otherwise turn every table's size into a silent zero.
fn counter(row: &MySqlRow, name: &str) -> u64 {
    if let Ok(value) = row.try_get::<Option<u64>, _>(name) {
        return value.unwrap_or(0);
    }
    row.try_get::<Option<i64>, _>(name)
        .ok()
        .flatten()
        .unwrap_or(0)
        .max(0) as u64
}

/// Creates a database. Not a table's shape at all, but it is written the same way as everything
/// else here — a statement built from a quoted name and a checked collation.
///
/// `collation` alone is enough: MySQL takes the character set from the collation's own. Left out,
/// the database inherits the server's.
pub async fn create_database(
    pool: &MySqlPool,
    name: &str,
    collation: Option<&str>,
) -> Result<(), AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(err!("error.databaseNameRequired"));
    }
    let mut sql = format!("CREATE DATABASE {}", quote_ident(name));
    if let Some(collation) = validated_collation(collation)? {
        sql.push_str(&format!(" COLLATE = {collation}"));
    }
    execute(pool, sql).await
}

/// Drops a database and every table in it.
pub async fn drop_database(pool: &MySqlPool, name: &str) -> Result<(), AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(err!("error.databaseNameRequired"));
    }
    execute(pool, format!("DROP DATABASE {}", quote_ident(name))).await
}

/// The character set a dump of this database should be transferred in.
///
/// mysqldump converts every string on its way out to the character set it is told to use, so the
/// one that changes nothing is the one the data is already in: where the whole database agrees on
/// a single character set, that one is used and the bytes come out exactly as stored. Where it
/// does not — a `latin1` column beside a `utf8mb4` one — there is no such character set, and
/// `utf8mb4` is picked as the one that can hold everything the others can (every character set
/// MySQL supports maps into Unicode), with mysqldump's own `SET NAMES` telling the restore how to
/// read it back.
pub async fn dump_charset(pool: &MySqlPool, database: &str) -> Result<String, AppError> {
    const FALLBACK: &str = "utf8mb4";

    let mut charsets: Vec<String> = sqlx::query(
        "SELECT DEFAULT_CHARACTER_SET_NAME AS charset
         FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = ?
         UNION
         SELECT DISTINCT CHARACTER_SET_NAME AS charset
         FROM information_schema.COLUMNS
         WHERE TABLE_SCHEMA = ? AND CHARACTER_SET_NAME IS NOT NULL",
    )
    .bind(database)
    .bind(database)
    .fetch_all(pool)
    .await
    .map_err(map_error)?
    .iter()
    .filter_map(|row| text(row, "charset"))
    // The name is about to reach a command line, so anything not shaped like a character set name
    // is dropped rather than passed on — which leaves the fallback to be used instead.
    .filter(|charset| {
        !charset.is_empty()
            && charset
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
    .collect();
    charsets.sort();
    charsets.dedup();

    Ok(match charsets.as_slice() {
        [only] => only.clone(),
        _ => FALLBACK.to_string(),
    })
}

/// Creates a table with nothing in it but the key every table ends up wanting: an unsigned
/// `int(11)` `id` that MySQL fills in itself. A table has to be declared with at least one column,
/// and the Structure tab is where the rest of them are added — so this asks for the two things
/// that cannot be changed as cheaply afterwards, the name and the collation.
///
/// `collation` alone is enough: MySQL takes the character set from the collation's own, so naming
/// both would only be a way of contradicting oneself. Left out, the table inherits the database's.
pub async fn create_table(
    pool: &MySqlPool,
    database: &str,
    table: &str,
    collation: Option<&str>,
) -> Result<(), AppError> {
    let name = table.trim();
    if name.is_empty() {
        return Err(err!("error.tableNameRequired"));
    }
    let id = quote_ident("id");
    let mut sql = format!(
        "CREATE TABLE {} ({id} int(11) unsigned NOT NULL AUTO_INCREMENT, PRIMARY KEY ({id}))",
        qualified(database, name)
    );
    if let Some(collation) = validated_collation(collation)? {
        sql.push_str(&format!(" COLLATE = {collation}"));
    }
    execute(pool, sql).await
}

/// Renames a table within its database. `RENAME TABLE` rather than `ALTER TABLE ... RENAME`, since
/// it is the one form MySQL guarantees to be atomic — nothing ever sees both names, or neither.
pub async fn rename_table(
    pool: &MySqlPool,
    database: &str,
    table: &str,
    new_name: &str,
) -> Result<(), AppError> {
    let new_name = new_name.trim();
    if table.trim().is_empty() || new_name.is_empty() {
        return Err(err!("error.tableNameRequired"));
    }
    execute(
        pool,
        format!(
            "RENAME TABLE {} TO {}",
            qualified(database, table.trim()),
            qualified(database, new_name)
        ),
    )
    .await
}

/// Drops a table and everything in it. Plain `DROP TABLE`, not `IF EXISTS`: asking to drop
/// something that is not there is worth being told about rather than passing quietly.
pub async fn drop_table(pool: &MySqlPool, database: &str, table: &str) -> Result<(), AppError> {
    if table.trim().is_empty() {
        return Err(err!("error.tableNameRequired"));
    }
    execute(
        pool,
        format!("DROP TABLE {}", qualified(database, table.trim())),
    )
    .await
}

pub async fn add_column(
    pool: &MySqlPool,
    database: &str,
    table: &str,
    spec: &ColumnSpec,
) -> Result<(), AppError> {
    let definition = column_definition(spec)?;
    let position = position_clause(spec.after.as_deref());
    execute(
        pool,
        format!(
            "ALTER TABLE {} ADD COLUMN {definition}{position}",
            qualified(database, table)
        ),
    )
    .await
}

/// Redefines an existing column, `name` being what it is called now — `CHANGE COLUMN` is what
/// lets a rename and a redefinition happen in the one statement, so the editor can offer both in
/// the one form.
pub async fn modify_column(
    pool: &MySqlPool,
    database: &str,
    table: &str,
    name: &str,
    spec: &ColumnSpec,
) -> Result<(), AppError> {
    if name.trim().is_empty() {
        return Err(err!("error.columnNameRequired"));
    }
    let definition = column_definition(spec)?;
    let position = position_clause(spec.after.as_deref());
    execute(
        pool,
        format!(
            "ALTER TABLE {} CHANGE COLUMN {} {definition}{position}",
            qualified(database, table),
            quote_ident(name)
        ),
    )
    .await
}

pub async fn drop_column(
    pool: &MySqlPool,
    database: &str,
    table: &str,
    name: &str,
) -> Result<(), AppError> {
    if name.trim().is_empty() {
        return Err(err!("error.columnNameRequired"));
    }
    execute(
        pool,
        format!(
            "ALTER TABLE {} DROP COLUMN {}",
            qualified(database, table),
            quote_ident(name)
        ),
    )
    .await
}

pub async fn add_index(
    pool: &MySqlPool,
    database: &str,
    table: &str,
    spec: &IndexSpec,
) -> Result<(), AppError> {
    let clause = add_index_clause(spec)?;
    execute(
        pool,
        format!("ALTER TABLE {} {clause}", qualified(database, table)),
    )
    .await
}

/// Replaces an index. MySQL cannot alter one in place, so the old index is dropped and the new one
/// created — both in a single `ALTER TABLE`, which is what keeps the table from spending any time
/// without the index at all.
pub async fn modify_index(
    pool: &MySqlPool,
    database: &str,
    table: &str,
    name: &str,
    spec: &IndexSpec,
) -> Result<(), AppError> {
    if name.trim().is_empty() {
        return Err(err!("error.indexNameRequired"));
    }
    let clause = add_index_clause(spec)?;
    execute(
        pool,
        format!(
            "ALTER TABLE {} {}, {clause}",
            qualified(database, table),
            drop_index_clause(name)
        ),
    )
    .await
}

pub async fn drop_index(
    pool: &MySqlPool,
    database: &str,
    table: &str,
    name: &str,
) -> Result<(), AppError> {
    if name.trim().is_empty() {
        return Err(err!("error.indexNameRequired"));
    }
    execute(
        pool,
        format!(
            "ALTER TABLE {} {}",
            qualified(database, table),
            drop_index_clause(name)
        ),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A column spec with nothing set, for a test to change one thing about.
    fn column(name: &str, data_type: &str) -> ColumnSpec {
        ColumnSpec {
            name: name.to_string(),
            data_type: data_type.to_string(),
            nullable: true,
            default_value: None,
            default_is_expression: false,
            auto_increment: false,
            on_update_current_timestamp: false,
            collation: None,
            comment: String::new(),
            after: None,
        }
    }

    fn index(kind: &str, columns: &[&str]) -> IndexSpec {
        IndexSpec {
            name: String::new(),
            kind: kind.to_string(),
            index_type: None,
            columns: columns
                .iter()
                .map(|name| IndexColumnSpec {
                    name: name.to_string(),
                    prefix_length: None,
                })
                .collect(),
            comment: String::new(),
        }
    }

    /// The clause a column definition adds up to, in the order MySQL's grammar wants it.
    #[test]
    fn a_column_definition_is_assembled_in_the_order_mysql_reads_it() {
        assert_eq!(
            column_definition(&column("id", "int")).unwrap(),
            "`id` int NULL"
        );

        let mut spec = column("id", "int");
        spec.nullable = false;
        spec.auto_increment = true;
        spec.comment = "the key".to_string();
        assert_eq!(
            column_definition(&spec).unwrap(),
            "`id` int NOT NULL AUTO_INCREMENT COMMENT 'the key'",
        );

        // Every part at once, so that a reordering shows up here rather than at the server.
        let mut spec = column("name", "varchar(50)");
        spec.nullable = false;
        spec.collation = Some("utf8mb4_bin".to_string());
        spec.default_value = Some("anon".to_string());
        spec.comment = "who".to_string();
        assert_eq!(
            column_definition(&spec).unwrap(),
            "`name` varchar(50) COLLATE utf8mb4_bin NOT NULL DEFAULT 'anon' COMMENT 'who'",
        );

        // A timestamp column is the one place both CURRENT_TIMESTAMP clauses appear.
        let mut spec = column("seen", "timestamp");
        spec.default_value = Some("CURRENT_TIMESTAMP".to_string());
        spec.on_update_current_timestamp = true;
        assert_eq!(
            column_definition(&spec).unwrap(),
            "`seen` timestamp NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP",
        );
    }

    /// The two things a definition refuses, because the server's own message for either is worse.
    #[test]
    fn a_column_needs_a_name_and_a_type() {
        assert!(column_definition(&column("   ", "int")).is_err());
        assert!(column_definition(&column("id", "  ")).is_err());
    }

    /// Which defaults are quoted and which are not. Getting this wrong is silent: the column ends
    /// up holding the *text* `CURRENT_TIMESTAMP` rather than the time.
    #[test]
    fn a_default_is_quoted_unless_it_can_only_be_an_expression() {
        // On a temporal column the function is recognised without being declared one.
        assert_eq!(
            default_clause("CURRENT_TIMESTAMP", false, "timestamp"),
            "CURRENT_TIMESTAMP"
        );
        assert_eq!(default_clause("now()", false, "datetime"), "now()");
        // On anything else the same text is a string, which is what it would have to mean.
        assert_eq!(
            default_clause("CURRENT_TIMESTAMP", false, "varchar(64)"),
            "'CURRENT_TIMESTAMP'"
        );
        // Unless it is declared an expression.
        assert_eq!(
            default_clause("CURRENT_TIMESTAMP", true, "varchar(64)"),
            "CURRENT_TIMESTAMP"
        );

        // Any other expression is parenthesised, as MySQL 8 requires — and one that already is
        // comes through untouched rather than doubly wrapped.
        assert_eq!(default_clause("uuid()", true, "char(36)"), "(uuid())");
        assert_eq!(default_clause("(uuid())", true, "char(36)"), "(uuid())");

        // `NULL` typed on its own is SQL NULL; quoted, it is four characters.
        assert_eq!(default_clause("NULL", false, "int"), "NULL");
        assert_eq!(default_clause("null", false, "int"), "NULL");
        assert_eq!(
            default_clause("nullable", false, "varchar(20)"),
            "'nullable'"
        );

        // A number is quoted too: MySQL reads a quoted number back as the number, so there is
        // nothing to gain from telling them apart and a value to lose by guessing wrong.
        assert_eq!(default_clause("42", false, "int"), "'42'");
        // A quote inside a literal is doubled rather than ending it early.
        assert_eq!(default_clause("it's", false, "varchar(20)"), "'it''s'");
    }

    /// A collation goes into DDL bare — MySQL has no quoted form for one — so anything not shaped
    /// like a name is refused rather than escaped.
    #[test]
    fn a_collation_is_refused_unless_it_is_a_plain_name() {
        assert_eq!(
            validated_collation(Some("utf8mb4_unicode_ci")).unwrap(),
            Some("utf8mb4_unicode_ci"),
        );
        // Nothing asked for, in each of the three ways of asking for nothing.
        assert_eq!(validated_collation(None).unwrap(), None);
        assert_eq!(validated_collation(Some("")).unwrap(), None);
        assert_eq!(validated_collation(Some("   ")).unwrap(), None);

        for hostile in ["utf8mb4_bin; DROP TABLE t", "utf8mb4-bin", "utf8mb4_bin'"] {
            assert!(
                validated_collation(Some(hostile)).is_err(),
                "{hostile} was allowed through"
            );
        }
    }

    /// Each index kind and what MySQL will accept on it.
    #[test]
    fn an_index_clause_carries_only_what_its_kind_allows() {
        assert_eq!(
            add_index_clause(&index("index", &["a"])).unwrap(),
            "ADD INDEX (`a`)"
        );
        assert_eq!(
            add_index_clause(&index("unique", &["a", "b"])).unwrap(),
            "ADD UNIQUE INDEX (`a`, `b`)",
        );
        // Case is the frontend's business, not a reason to refuse.
        assert_eq!(
            add_index_clause(&index("FULLTEXT", &["a"])).unwrap(),
            "ADD FULLTEXT INDEX (`a`)"
        );

        // A primary key has no name, and MySQL accepts neither a comment nor USING on it — so a
        // spec carrying both still produces the bare form rather than a statement it would reject.
        let mut spec = index("primary", &["a"]);
        spec.name = "ignored".to_string();
        spec.comment = "ignored".to_string();
        spec.index_type = Some("BTREE".to_string());
        assert_eq!(add_index_clause(&spec).unwrap(), "ADD PRIMARY KEY (`a`)");

        // USING belongs to the two general-purpose kinds only.
        let mut spec = index("index", &["a"]);
        spec.name = "by_a".to_string();
        spec.index_type = Some("hash".to_string());
        spec.comment = "fast".to_string();
        assert_eq!(
            add_index_clause(&spec).unwrap(),
            "ADD INDEX `by_a` (`a`) USING HASH COMMENT 'fast'",
        );

        let mut spec = index("fulltext", &["a"]);
        spec.index_type = Some("BTREE".to_string());
        assert_eq!(add_index_clause(&spec).unwrap(), "ADD FULLTEXT INDEX (`a`)");

        // A prefix length, which a TEXT column cannot be indexed without.
        let mut spec = index("index", &["body"]);
        spec.columns[0].prefix_length = Some(32);
        assert_eq!(add_index_clause(&spec).unwrap(), "ADD INDEX (`body`(32))");
        // Zero is not a prefix; it is the absence of one.
        spec.columns[0].prefix_length = Some(0);
        assert_eq!(add_index_clause(&spec).unwrap(), "ADD INDEX (`body`)");
    }

    /// What an index clause refuses.
    #[test]
    fn an_index_needs_columns_and_a_kind_that_exists() {
        assert!(add_index_clause(&index("index", &[])).is_err());
        assert!(add_index_clause(&index("index", &["  "])).is_err());
        assert!(add_index_clause(&index("clustered", &["a"])).is_err());

        let mut spec = index("index", &["a"]);
        spec.index_type = Some("BRIN".to_string());
        assert!(add_index_clause(&spec).is_err());
    }

    /// The values MariaDB 11.8 actually reports for these declarations, read off a live server.
    /// Each is the source text of the default rather than its value, which is the whole difference
    /// from MySQL.
    #[test]
    fn a_mariadb_default_is_read_as_the_literal_it_is() {
        // A quoted literal comes back as its value, with the quoting undone.
        assert_eq!(
            mariadb_default(Some("'abc'".into())),
            (Some("abc".into()), false)
        );
        assert_eq!(
            mariadb_default(Some("'a''b'".into())),
            (Some("a'b".into()), false)
        );
        // A backslash in the value arrives doubled, the way the server would have to write it.
        assert_eq!(
            mariadb_default(Some(r"'a\\b'".into())),
            (Some(r"a\b".into()), false)
        );
        // And a single one is an escape, which is what doubling it is there to avoid: `\b` is a
        // backspace to MySQL and to MariaDB alike.
        assert_eq!(
            mariadb_default(Some(r"'a\b'".into())),
            (Some("a\u{8}".into()), false)
        );
        assert_eq!(
            mariadb_default(Some("''".into())),
            (Some(String::new()), false)
        );
        // A string that reads like NULL is still a string: it arrives quoted.
        assert_eq!(
            mariadb_default(Some("'NULL'".into())),
            (Some("NULL".into()), false)
        );
        // And a number stays the number it is, rather than becoming an expression.
        assert_eq!(mariadb_default(Some("7".into())), (Some("7".into()), false));
        assert_eq!(
            mariadb_default(Some("-3".into())),
            (Some("-3".into()), false)
        );
        assert_eq!(
            mariadb_default(Some("1.50".into())),
            (Some("1.50".into()), false)
        );
    }

    /// The bare word, which MariaDB writes both for `DEFAULT NULL` and for no default at all. Read
    /// as no default either way — on a nullable column the two mean the same thing, and a column
    /// that is not nullable reports SQL NULL instead.
    #[test]
    fn a_mariadb_null_default_is_no_default() {
        assert_eq!(mariadb_default(Some("NULL".into())), (None, false));
        assert_eq!(mariadb_default(None), (None, false));
    }

    /// What is neither quoted nor a number can only be an expression — which is the distinction
    /// MySQL draws with `DEFAULT_GENERATED` in `EXTRA`, and MariaDB does not draw at all.
    #[test]
    fn a_mariadb_expression_default_is_marked_as_one() {
        assert_eq!(
            mariadb_default(Some("current_timestamp()".into())),
            (Some("current_timestamp()".into()), true)
        );
        assert_eq!(
            mariadb_default(Some("uuid()".into())),
            (Some("uuid()".into()), true)
        );
        assert_eq!(
            mariadb_default(Some("(1 + 1)".into())),
            (Some("(1 + 1)".into()), true)
        );
    }

    /// Whatever is read out of a default has to survive being written back into one, or editing a
    /// column would rewrite the default it was only meant to leave alone.
    ///
    /// `'NULL'` is left out, and not because MariaDB is any trouble: it is read back correctly
    /// here, but [`default_clause`] writes the four characters out as SQL NULL by design — typing
    /// `NULL` into the default box is how a user asks for SQL NULL. MySQL loses that default in
    /// exactly the same way, so it is nothing this reading introduced.
    #[test]
    fn a_default_read_from_mariadb_goes_back_the_way_it_came() {
        for reported in [
            "'abc'",
            "'a''b'",
            "''",
            "7",
            "current_timestamp()",
            "(1 + 1)",
        ] {
            let (value, is_expression) = mariadb_default(Some(reported.into()));
            let value = value.expect("none of these is an absent default");
            let clause = default_clause(&value, is_expression, "varchar(32)");
            // Re-reading the clause the way MariaDB would report it again lands on the same value.
            let (again, _) = mariadb_default(Some(clause));
            assert_eq!(again.as_deref(), Some(value.as_str()), "{reported}");
        }
    }
}
