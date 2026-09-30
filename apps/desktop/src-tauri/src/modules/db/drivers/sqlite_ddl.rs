//! The statements that change a SQLite schema — the write half of the Structure tab.
//!
//! Shorter than `postgres_ddl.rs` for most of it, and not because SQLite is simpler: `ALTER TABLE`
//! here does four things and nothing else — rename the table, rename a column, add a column, drop
//! a column. There is no statement that changes a column's type, its nullability, its default or
//! its collation, and none that adds a primary key to a table that has none.
//!
//! `rebuild_column` is the way round the first four of those: the twelve-step procedure SQLite's
//! own documentation describes — create a new table with the schema wanted, copy the rows across,
//! drop the old one, rename the new one in its place, and put the indexes, triggers and any
//! dependent views back. `modify_column` reaches it only when a rename is not the whole story (a
//! type, a default or a collation actually differs) and only when the column is not part of a
//! table-level constraint, generated, or the table's own rowid alias — those stay refused by name,
//! same as adding a primary key to a table that never had one. See
//! `docs/specs/2026-09-04-sqlite-completion-design.md` (B1-B8) for the decisions this
//! implements.

use super::sqlite::{map_error, quote_ident};
use crate::error::AppError;
use serde::Deserialize;
use sqlx::{Row, SqlitePool};

/// One column as the dialog describes it.
///
/// Three of the fields the dialog sends have no field here to land in, and serde drops them: the
/// comment (SQLite has no column comments), `onUpdateCurrentTimestamp` (a trigger here, not a
/// property of a column) and `autoIncrement` — `AUTOINCREMENT` is only legal inside a
/// `CREATE TABLE`, and only on an `INTEGER PRIMARY KEY`, so there is no way to give an existing
/// column either. Left out rather than accepted and ignored, so that nothing here reads as though
/// it might one day be honoured.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnSpec {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    /// `None` writes no DEFAULT at all.
    #[serde(default)]
    pub default_value: Option<String>,
    /// Emits the default as an expression rather than as a quoted literal — `CURRENT_TIMESTAMP`
    /// rather than `'CURRENT_TIMESTAMP'`.
    #[serde(default)]
    pub default_is_expression: bool,
    #[serde(default)]
    pub collation: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexColumnSpec {
    pub name: String,
}

/// One index as the dialog describes it.
///
/// The comment and the access method are dropped the way `ColumnSpec`'s extras are: SQLite has no
/// index comments, and every SQLite index is a b-tree — which is also why `indexMethods` is empty
/// on the dialect and the dialog offers no choice to send.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexSpec {
    /// Left empty, the index is named after the table and its columns — SQLite requires a name and
    /// has no default of its own.
    #[serde(default)]
    pub name: String,
    /// `index` or `unique`. `primary` never arrives: a primary key is part of `CREATE TABLE` here
    /// and the dialog does not offer it — see `sqliteEditing`.
    pub kind: String,
    pub columns: Vec<IndexColumnSpec>,
}

/// Wraps text as a SQL string literal, doubling the quote that would otherwise end it early.
///
/// A backslash is left alone: a SQLite string literal is not backslash-escaped, so doubling them
/// would store two where the user typed one.
pub(super) fn quote_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// Splits the column/constraint list of a `CREATE TABLE` statement into its clauses (split at
/// top-level commas — depth 0 inside the outermost parens), and returns whatever table-option
/// text follows the closing paren (`WITHOUT ROWID`, `STRICT`, both, or `""`). Not a SQL parser: it
/// tracks quotes (`'`, `"`, `` ` ``, `[...]`), comments (`--`, `/* */`) and paren depth only — see
/// B3 of the design spec.
fn split_column_clauses(create_sql: &str) -> Result<(Vec<String>, String), AppError> {
    let chars: Vec<char> = create_sql.chars().collect();
    let mut i = 0;

    // The table name and any quoting around it — skipped to reach the column list's own `(`. The
    // first unquoted `(` in a `CREATE TABLE` is always this one.
    while i < chars.len() && chars[i] != '(' {
        i = match chars[i] {
            '\'' | '"' | '`' => skip_quoted(&chars, i),
            '[' => skip_bracket(&chars, i),
            _ => i + 1,
        };
    }
    if i >= chars.len() {
        return Err(err!("error.sqliteRebuildParseFailed", table = ""));
    }
    i += 1;
    let mut depth = 1u32;
    // Built up character by character rather than sliced from `chars` between two indices: a
    // comment inside a clause must be gone from its text entirely, not merely skipped over while
    // deciding where the clause boundaries are.
    let mut current = String::new();
    let mut clauses: Vec<String> = Vec::new();
    let mut body_end: Option<usize> = None;

    while i < chars.len() {
        match chars[i] {
            '\'' | '"' | '`' => {
                let start = i;
                i = skip_quoted(&chars, i);
                current.extend(chars[start..i].iter().copied());
            }
            '[' => {
                let start = i;
                i = skip_bracket(&chars, i);
                current.extend(chars[start..i].iter().copied());
            }
            '-' if chars.get(i + 1) == Some(&'-') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                i += 2;
                while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                    i += 1;
                }
                i = (i + 2).min(chars.len());
            }
            '(' => {
                depth += 1;
                current.push('(');
                i += 1;
            }
            ')' => {
                depth -= 1;
                if depth == 0 {
                    clauses.push(current.trim().to_string());
                    body_end = Some(i);
                    i += 1;
                    break;
                }
                current.push(')');
                i += 1;
            }
            ',' if depth == 1 => {
                clauses.push(current.trim().to_string());
                current = String::new();
                i += 1;
            }
            c => {
                current.push(c);
                i += 1;
            }
        }
    }

    if body_end.is_none() {
        return Err(err!("error.sqliteRebuildParseFailed", table = ""));
    }
    let suffix: String = chars[i..].iter().collect::<String>().trim().to_string();
    Ok((clauses, suffix))
}

/// Skips one quoted run starting at `i` (`'`, `"` or `` ` ``), doubling the quote character to
/// escape it — the rule `quote_string` writes going out, mirrored coming in. Returns the index
/// just past the closing quote (or the end of input, for an unterminated one).
fn skip_quoted(chars: &[char], i: usize) -> usize {
    let quote = chars[i];
    let mut j = i + 1;
    while j < chars.len() {
        if chars[j] == quote {
            if chars.get(j + 1) == Some(&quote) {
                j += 2;
                continue;
            }
            return j + 1;
        }
        j += 1;
    }
    j
}

/// Skips a `[bracketed identifier]` starting at the `[`. SQLite has no escape for `]` inside one.
fn skip_bracket(chars: &[char], i: usize) -> usize {
    let mut j = i + 1;
    while j < chars.len() && chars[j] != ']' {
        j += 1;
    }
    (j + 1).min(chars.len())
}

/// Whether a clause is a table-level constraint (`PRIMARY KEY`, `UNIQUE`, `CHECK`, `FOREIGN KEY`,
/// `CONSTRAINT ...`) rather than a column definition — checked by a whole-keyword match so a
/// column merely *named* `primary_email` is not mistaken for one (B3 of the design spec).
fn is_table_constraint(clause: &str) -> bool {
    let upper = clause.trim_start().to_ascii_uppercase();
    ["PRIMARY", "UNIQUE", "CHECK", "FOREIGN", "CONSTRAINT"]
        .iter()
        .any(|kw| {
            upper == *kw
                || upper
                    .strip_prefix(kw)
                    .is_some_and(|rest| rest.starts_with(|c: char| c.is_whitespace() || c == '('))
        })
}

/// The column name a column-definition clause declares — its first token, unquoted. `None` for a
/// table-constraint clause (it has no column name of its own — check with [`is_table_constraint`]
/// first) or one this cannot make sense of.
fn clause_column_name(clause: &str) -> Option<String> {
    if is_table_constraint(clause) {
        return None;
    }
    let trimmed = clause.trim_start();
    let first = trimmed.chars().next()?;
    match first {
        '"' | '`' => {
            let body = &trimmed[1..];
            let end = body.find(first)?;
            Some(body[..end].to_string())
        }
        '[' => {
            let body = &trimmed[1..];
            let end = body.find(']')?;
            Some(body[..end].to_string())
        }
        c if c.is_alphabetic() || c == '_' => {
            let name: String = trimmed
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if name.is_empty() {
                None
            } else {
                Some(name)
            }
        }
        _ => None,
    }
}

/// Whether `column` appears as a whole identifier — bare or quoted — anywhere in `clause`. Used to
/// check a table-constraint clause (`PRIMARY KEY (...)`, `FOREIGN KEY (...)`, ...) for the column
/// a rebuild is about to touch (B3 of the design spec). A word-boundary scan, not a SQL parser — a
/// false positive only means an over-cautious refusal, never a silently wrong rebuild.
fn clause_mentions_column(clause: &str, column: &str) -> bool {
    for quote in ['"', '`'] {
        if clause.contains(&format!("{quote}{column}{quote}")) {
            return true;
        }
    }
    if clause.contains(&format!("[{column}]")) {
        return true;
    }
    let chars: Vec<char> = clause.chars().collect();
    let target: Vec<char> = column.chars().collect();
    if target.is_empty() || target.len() > chars.len() {
        return false;
    }
    for i in 0..=(chars.len() - target.len()) {
        if chars[i..i + target.len()] != target[..] {
            continue;
        }
        let before_ok = i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '_');
        let after = i + target.len();
        let after_ok =
            after == chars.len() || !(chars[after].is_alphanumeric() || chars[after] == '_');
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

/// The identifier after a trailing `COLLATE` in a column clause, or `None` when there is none.
/// SQLite only allows `COLLATE name` as a bare keyword-and-identifier pair at the clause's own
/// level — never nested inside a `DEFAULT (...)` expression — so a plain case-insensitive search
/// for the word is enough here, unlike [`is_table_constraint`]'s leading-keyword check.
fn clause_collation(clause: &str) -> Option<String> {
    let upper = clause.to_ascii_uppercase();
    let at = upper.rfind("COLLATE")?;
    let after = clause[at + "COLLATE".len()..].trim_start();
    // Reads the same shape `clause_column_name` reads a leading identifier in — bare or quoted —
    // which is exactly what a collation name is.
    clause_column_name(after)
}

/// Runs statements one after another in a transaction, so a change made of several either lands
/// whole or not at all.
async fn execute_all(pool: &SqlitePool, statements: Vec<String>) -> Result<(), AppError> {
    let mut tx = pool.begin().await.map_err(map_error)?;
    for sql in statements {
        if let Err(e) = sqlx::query(sqlx::AssertSqlSafe(sql))
            .execute(&mut *tx)
            .await
        {
            tx.rollback().await.map_err(map_error)?;
            return Err(map_error(e));
        }
    }
    tx.commit().await.map_err(map_error)?;
    Ok(())
}

/// One column as it is written inside `CREATE TABLE` or after `ADD COLUMN`.
fn column_definition(spec: &ColumnSpec) -> Result<String, AppError> {
    let name = spec.name.trim();
    if name.is_empty() {
        return Err(err!("error.columnNameRequired"));
    }

    let mut sql = quote_ident(name);
    /* A type is optional in SQLite — a column declared with none takes blob affinity — so an empty
    one is written as nothing rather than refused. */
    let data_type = spec.data_type.trim();
    if !data_type.is_empty() {
        sql.push(' ');
        sql.push_str(data_type);
    }
    if let Some(collation) = spec
        .collation
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
    {
        sql.push_str(" COLLATE ");
        sql.push_str(&quote_ident(collation));
    }
    if !spec.nullable {
        sql.push_str(" NOT NULL");
    }
    if let Some(default) = spec.default_value.as_deref() {
        sql.push_str(" DEFAULT ");
        if spec.default_is_expression {
            /* Parenthesised, which is what SQLite requires of anything but a literal or one of the
            `CURRENT_*` keywords — and which those three tolerate. */
            if is_current_keyword(default) {
                sql.push_str(default);
            } else {
                sql.push_str(&format!("({default})"));
            }
        } else {
            sql.push_str(&quote_string(default));
        }
    }
    Ok(sql)
}

/// The three defaults SQLite spells as bare keywords rather than as expressions.
fn is_current_keyword(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_uppercase().as_str(),
        "CURRENT_TIME" | "CURRENT_DATE" | "CURRENT_TIMESTAMP"
    )
}

/// Creates a table with one column: an `INTEGER PRIMARY KEY`, which is SQLite's own rowid under a
/// name. Every other column is added from the Structure tab afterwards.
///
/// `collation` is accepted and ignored. A SQLite table carries no collation of its own — only a
/// column does — which is why `objectCollation` is off on the dialect and the dialog never offers
/// one here.
pub async fn create_table(pool: &SqlitePool, table: &str) -> Result<(), AppError> {
    let table = table.trim();
    if table.is_empty() {
        return Err(err!("error.tableNameRequired"));
    }
    execute_all(
        pool,
        vec![format!(
            "CREATE TABLE {} (\"id\" INTEGER PRIMARY KEY)",
            quote_ident(table)
        )],
    )
    .await
}

/// Renames a table. SQLite rewrites the references to it in other tables' foreign keys as it goes.
pub async fn rename_table(pool: &SqlitePool, table: &str, new_name: &str) -> Result<(), AppError> {
    let (table, new_name) = (table.trim(), new_name.trim());
    if table.is_empty() || new_name.is_empty() {
        return Err(err!("error.tableNameRequired"));
    }
    execute_all(
        pool,
        vec![format!(
            "ALTER TABLE {} RENAME TO {}",
            quote_ident(table),
            quote_ident(new_name)
        )],
    )
    .await
}

pub async fn drop_table(pool: &SqlitePool, table: &str) -> Result<(), AppError> {
    let table = table.trim();
    if table.is_empty() {
        return Err(err!("error.tableNameRequired"));
    }
    execute_all(pool, vec![format!("DROP TABLE {}", quote_ident(table))]).await
}

/// Appends a column.
///
/// Always appends: `ALTER TABLE` has no `FIRST` or `AFTER`, which is why `columnPosition` is off on
/// the dialect and the dialog offers no place to put it.
///
/// SQLite refuses a few shapes here that it would accept inside a `CREATE TABLE` — a `NOT NULL`
/// column with no default, a `UNIQUE` or `PRIMARY KEY` one — and those refusals are passed through
/// as the engine words them rather than pre-empted: it states the rule better than a second copy of
/// it here would.
pub async fn add_column(pool: &SqlitePool, table: &str, spec: &ColumnSpec) -> Result<(), AppError> {
    let definition = column_definition(spec)?;
    execute_all(
        pool,
        vec![format!(
            "ALTER TABLE {} ADD COLUMN {definition}",
            quote_ident(table)
        )],
    )
    .await
}

/// What a column is now, for the comparison [`modify_column`] makes before it refuses.
struct CurrentColumn {
    data_type: String,
    nullable: bool,
    default_value: Option<String>,
    collation: Option<String>,
}

/// The table's own `CREATE TABLE` text, as `sqlite_master` holds it.
async fn table_create_sql(pool: &SqlitePool, table: &str) -> Result<String, AppError> {
    sqlx::query_scalar("select sql from sqlite_master where type = 'table' and name = ?")
        .bind(table)
        .fetch_optional(pool)
        .await
        .map_err(map_error)?
        .ok_or_else(|| err!("error.sqliteRebuildParseFailed", table = table))
}

/// Reads a table's own `CREATE TABLE` text and finds the column's own collation via
/// [`split_column_clauses`] — `pragma_table_xinfo` has no such field (see B5 of the design spec).
/// A parse failure here refuses the whole `modify_column` call, including a plain rename: silently
/// treating an unreadable collation as "unchanged" risks a rebuild that drops a real one.
async fn current_column(
    pool: &SqlitePool,
    table: &str,
    column: &str,
) -> Result<CurrentColumn, AppError> {
    let row = sqlx::query(
        "select type, \"notnull\", dflt_value from pragma_table_xinfo(?) where name = ?",
    )
    .bind(table)
    .bind(column)
    .fetch_optional(pool)
    .await
    .map_err(map_error)?
    .ok_or_else(|| err!("error.unknownColumn", table = table, name = column))?;

    let create_sql = table_create_sql(pool, table).await?;
    let (clauses, _) = split_column_clauses(&create_sql)
        .map_err(|_| err!("error.sqliteRebuildParseFailed", table = table))?;
    let clause = clauses
        .into_iter()
        .find(|c| !is_table_constraint(c) && clause_column_name(c).as_deref() == Some(column))
        .ok_or_else(|| err!("error.sqliteRebuildParseFailed", table = table))?;

    Ok(CurrentColumn {
        data_type: row.get::<String, _>("type"),
        nullable: row.get::<i64, _>("notnull") == 0,
        default_value: super::sqlite::split_default(row.get::<Option<String>, _>("dflt_value")).0,
        collation: clause_collation(&clause),
    })
}

/// Renames a column outright (`ALTER TABLE ... RENAME COLUMN`), or hands off to
/// [`rebuild_column`] when the type, `NOT NULL`, default or collation actually differs. Refuses a
/// rename combined with any of those in one call — see B1 of the design spec — and refuses
/// outright a column [`rebuild_blockers`]/`generated_or_rowid_alias` say cannot be rebuilt at all.
pub async fn modify_column(
    pool: &SqlitePool,
    table: &str,
    name: &str,
    spec: &ColumnSpec,
) -> Result<(), AppError> {
    let new_name = spec.name.trim();
    if new_name.is_empty() {
        return Err(err!("error.columnNameRequired"));
    }
    let current = current_column(pool, table, name).await?;

    /* Compared rather than assumed unchanged: the dialog sends the whole column back whether or not
    anything but the name was touched, so "did anything else change?" is a question about the
    values, not about which fields arrived. The type is compared case-insensitively — SQLite
    stores the declaration verbatim, so `text` and `TEXT` come back different and mean the
    same. */
    let spec_collation = spec
        .collation
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty());
    let anything_else_changed = !current
        .data_type
        .eq_ignore_ascii_case(spec.data_type.trim())
        || current.nullable != spec.nullable
        || current.default_value.as_deref() != spec.default_value.as_deref()
        || current.collation.as_deref() != spec_collation;
    let renamed = new_name != name;

    if renamed && anything_else_changed {
        return Err(err!("error.sqliteRenameWithOtherChanges", column = name));
    }
    if anything_else_changed {
        return rebuild_column(pool, table, name, spec).await;
    }
    if !renamed {
        return Ok(());
    }
    execute_all(
        pool,
        vec![format!(
            "ALTER TABLE {} RENAME COLUMN {} TO {}",
            quote_ident(table),
            quote_ident(name),
            quote_ident(new_name)
        )],
    )
    .await
}

/// Drops a column.
///
/// SQLite refuses to drop one that anything depends on — a primary key, a column an index or a
/// generated column is built from — and says which. Passed through as it words it.
pub async fn drop_column(pool: &SqlitePool, table: &str, column: &str) -> Result<(), AppError> {
    execute_all(
        pool,
        vec![format!(
            "ALTER TABLE {} DROP COLUMN {}",
            quote_ident(table),
            quote_ident(column)
        )],
    )
    .await
}

/// The `CREATE INDEX` one spec describes.
fn create_index(table: &str, spec: &IndexSpec) -> Result<String, AppError> {
    if spec.kind == "primary" {
        /* Never reached from the dialog, which does not offer the kind — but a primary key in
        SQLite is part of `CREATE TABLE` and there is no statement that adds one afterwards, so
        it is refused here rather than sent and failed on. */
        return Err(err!("error.sqliteNoPrimaryKeyAfterwards"));
    }
    let columns: Vec<&str> = spec
        .columns
        .iter()
        .map(|c| c.name.trim())
        .filter(|c| !c.is_empty())
        .collect();
    if columns.is_empty() {
        return Err(err!("error.indexNeedsColumn"));
    }

    // SQLite requires a name and has none of its own to fall back on, unlike MySQL.
    let name = if spec.name.trim().is_empty() {
        format!("{table}_{}", columns.join("_"))
    } else {
        spec.name.trim().to_string()
    };
    let unique = if spec.kind == "unique" { "UNIQUE " } else { "" };
    Ok(format!(
        "CREATE {unique}INDEX {} ON {} ({})",
        quote_ident(&name),
        quote_ident(table),
        columns
            .iter()
            .map(|c| quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

pub async fn add_index(pool: &SqlitePool, table: &str, spec: &IndexSpec) -> Result<(), AppError> {
    execute_all(pool, vec![create_index(table, spec)?]).await
}

/// Replaces an index: the old one dropped and the new one created in one transaction, so the table
/// is never seen without it.
pub async fn modify_index(
    pool: &SqlitePool,
    table: &str,
    name: &str,
    spec: &IndexSpec,
) -> Result<(), AppError> {
    let created = create_index(table, spec)?;
    execute_all(pool, vec![drop_index_sql(pool, name).await?, created]).await
}

pub async fn drop_index(pool: &SqlitePool, _table: &str, name: &str) -> Result<(), AppError> {
    let sql = drop_index_sql(pool, name).await?;
    execute_all(pool, vec![sql]).await
}

/// The `DROP INDEX` for one index, having first made sure it is one that can be dropped.
///
/// Two cannot. An index SQLite built for a `PRIMARY KEY` or a `UNIQUE` constraint belongs to the
/// constraint rather than to the schema — `DROP INDEX` on it fails — and the primary key of a rowid
/// table has no index at all, only the row it is shown under. Both are refused here, where the
/// reason can be given, rather than sent for SQLite to reject as a name it does not know.
async fn drop_index_sql(pool: &SqlitePool, name: &str) -> Result<String, AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(err!("error.indexNameRequired"));
    }
    let origin: Option<String> = sqlx::query_scalar(
        "select il.origin from sqlite_master m \
         join pragma_index_list(m.tbl_name) il on il.name = m.name \
         where m.type = 'index' and m.name = ?",
    )
    .bind(name)
    .fetch_optional(pool)
    .await
    .map_err(map_error)?;

    match origin.as_deref() {
        // `c` — someone wrote a CREATE INDEX for it, so it can be dropped.
        Some("c") => {}
        // `pk`/`u` — the index behind a constraint, which owns it.
        Some(_) => return Err(err!("error.sqliteIndexBelongsToConstraint", index = name)),
        // Not in `sqlite_master` at all: the synthesised primary key of a rowid table.
        None => return Err(err!("error.sqliteIndexBelongsToConstraint", index = name)),
    }

    Ok(format!("DROP INDEX {}", quote_ident(name)))
}

/// Refuses a rebuild up front when `column` is named inside a table-level constraint clause
/// (`PRIMARY KEY (...)`, `UNIQUE (...)`, `FOREIGN KEY (...)`, ...) — rewriting those is out of
/// scope (B3/B6 of the design spec).
fn rebuild_blockers(clauses: &[String], column: &str) -> Result<(), AppError> {
    for clause in clauses {
        if is_table_constraint(clause) && clause_mentions_column(clause, column) {
            return Err(err!("error.sqliteColumnInTableConstraint", column = column));
        }
    }
    Ok(())
}

/// Whether `column` is a generated column (`hidden` 2 or 3) or the `INTEGER PRIMARY KEY` rowid
/// alias — the same `hidden`/`pk` reading `sqlite_structure.rs::structure_columns` does, narrowed
/// to one column (B6 of the design spec).
async fn generated_or_rowid_alias(
    pool: &SqlitePool,
    table: &str,
    column: &str,
) -> Result<(bool, bool), AppError> {
    let rows = sqlx::query("select name, type, pk, hidden from pragma_table_xinfo(?)")
        .bind(table)
        .fetch_all(pool)
        .await
        .map_err(map_error)?;
    let key_count = rows.iter().filter(|r| r.get::<i64, _>("pk") > 0).count();
    let Some(row) = rows.iter().find(|r| r.get::<String, _>("name") == column) else {
        return Ok((false, false));
    };
    let hidden = row.get::<i64, _>("hidden");
    let generated = hidden == 2 || hidden == 3;
    let pk = row.get::<i64, _>("pk");
    let data_type = row.get::<String, _>("type");
    let rowid_alias = key_count == 1 && pk == 1 && data_type.eq_ignore_ascii_case("integer");
    Ok((generated, rowid_alias))
}

/// Rebuilds `table` around a new definition of `column` — the 12-step procedure SQLite's own docs
/// describe for changing anything `ALTER TABLE` cannot (B2/B7 of the design spec). Refuses up
/// front, before touching anything, when the column cannot be rebuilt this way (B6).
async fn rebuild_column(
    pool: &SqlitePool,
    table: &str,
    name: &str,
    spec: &ColumnSpec,
) -> Result<(), AppError> {
    let create_sql = table_create_sql(pool, table).await?;
    let (clauses, suffix) = split_column_clauses(&create_sql)
        .map_err(|_| err!("error.sqliteRebuildParseFailed", table = table))?;
    rebuild_blockers(&clauses, name)?;

    let target_index = clauses
        .iter()
        .position(|c| !is_table_constraint(c) && clause_column_name(c).as_deref() == Some(name))
        .ok_or_else(|| err!("error.sqliteRebuildParseFailed", table = table))?;

    let (generated, rowid_alias) = generated_or_rowid_alias(pool, table, name).await?;
    if generated {
        return Err(err!("error.sqliteColumnGenerated", column = name));
    }
    if rowid_alias {
        return Err(err!("error.sqliteColumnIsPrimaryKey", column = name));
    }

    let mut new_clauses = clauses.clone();
    new_clauses[target_index] = column_definition(spec)?;
    let suffix_sql = if suffix.is_empty() {
        String::new()
    } else {
        format!(" {suffix}")
    };

    let temp_name = temp_table_name(pool, table).await?;
    let create_new = format!(
        "CREATE TABLE {} ({}){}",
        quote_ident(&temp_name),
        new_clauses.join(", "),
        suffix_sql
    );

    let columns = super::sqlite_dump::data_columns(pool, table).await?;
    let column_list = columns
        .iter()
        .map(|c| quote_ident(c))
        .collect::<Vec<_>>()
        .join(", ");
    let insert_select = format!(
        "INSERT INTO {} ({column_list}) SELECT {column_list} FROM {}",
        quote_ident(&temp_name),
        quote_ident(table)
    );

    let dependent: Vec<String> = sqlx::query_scalar(
        "select sql from sqlite_master where tbl_name = ? and type in ('index', 'trigger') and sql is not null order by rowid",
    )
    .bind(table)
    .fetch_all(pool)
    .await
    .map_err(map_error)?;

    /* A view that mentions `table` has to be gone for the moment `table` itself is — SQLite's own
    `ALTER TABLE ... RENAME TO` re-validates every view in the schema as part of the rename, and
    fails on one whose base table does not exist in the instant between the old table's `DROP`
    and the new one's `RENAME`. Not in the original design spec's B7 — found while running this
    task's own tests against the fixture's `recent` view. Dropped before that window opens,
    recreated verbatim (same SQL text, so the exact same view) once it closes. */
    let views: Vec<(String, String)> = sqlx::query(
        "select name, sql from sqlite_master where type = 'view' and sql is not null order by rowid",
    )
    .fetch_all(pool)
    .await
    .map_err(map_error)?
    .into_iter()
    .map(|row| (row.get::<String, _>("name"), row.get::<String, _>("sql")))
    .filter(|(_, sql)| clause_mentions_column(sql, table))
    .collect();

    let mut conn = pool.acquire().await.map_err(map_error)?;
    let fk_pragma: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&mut *conn)
        .await
        .map_err(map_error)?;
    let fk_was_on = fk_pragma != 0;
    if fk_was_on {
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *conn)
            .await
            .map_err(map_error)?;
    }

    let plan = RebuildPlan {
        create_new: &create_new,
        insert_select: &insert_select,
        table,
        temp_name: &temp_name,
        dependent: &dependent,
        views: &views,
        check_foreign_keys: fk_was_on,
    };
    let result = run_rebuild_transaction(&mut conn, &plan).await;

    if fk_was_on {
        let _ = sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&mut *conn)
            .await;
    }
    result
}

/// Everything [`run_rebuild_transaction`] needs, gathered into one value rather than passed as
/// separate arguments — purely to keep the function's own signature short; each field is read
/// exactly where [`rebuild_column`] built it.
struct RebuildPlan<'a> {
    create_new: &'a str,
    insert_select: &'a str,
    table: &'a str,
    temp_name: &'a str,
    dependent: &'a [String],
    views: &'a [(String, String)],
    check_foreign_keys: bool,
}

/// Steps 4-11 of the rebuild — everything inside the one transaction. Split out from
/// [`rebuild_column`] so the `PRAGMA foreign_keys` restore (step 12) always runs after this
/// returns, whether `Ok` or `Err`.
async fn run_rebuild_transaction(
    conn: &mut sqlx::sqlite::SqliteConnection,
    plan: &RebuildPlan<'_>,
) -> Result<(), AppError> {
    use sqlx::Connection;

    let mut tx = conn.begin().await.map_err(map_error)?;

    sqlx::query(sqlx::AssertSqlSafe(plan.create_new.to_string()))
        .execute(&mut *tx)
        .await
        .map_err(map_error)?;
    sqlx::query(sqlx::AssertSqlSafe(plan.insert_select.to_string()))
        .execute(&mut *tx)
        .await
        .map_err(map_error)?;

    // Gone before `table` itself is, so SQLite's rename validation never sees one referencing a
    // table that momentarily does not exist — see the note where `views` is read.
    for (name, _) in plan.views {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP VIEW {}",
            quote_ident(name)
        )))
        .execute(&mut *tx)
        .await
        .map_err(map_error)?;
    }

    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP TABLE {}",
        quote_ident(plan.table)
    )))
    .execute(&mut *tx)
    .await
    .map_err(map_error)?;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "ALTER TABLE {} RENAME TO {}",
        quote_ident(plan.temp_name),
        quote_ident(plan.table)
    )))
    .execute(&mut *tx)
    .await
    .map_err(map_error)?;

    for statement in plan.dependent {
        sqlx::query(sqlx::AssertSqlSafe(statement.clone()))
            .execute(&mut *tx)
            .await
            .map_err(map_error)?;
    }

    // Recreated verbatim, same SQL text as before — the table they select from exists again by now.
    for (_, sql) in plan.views {
        sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
            .execute(&mut *tx)
            .await
            .map_err(map_error)?;
    }

    if plan.check_foreign_keys {
        let violations = sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut *tx)
            .await
            .map_err(map_error)?;
        if !violations.is_empty() {
            return Err(err!(
                "error.sqliteRebuildForeignKeyViolation",
                table = plan.table
            ));
        }
    }

    tx.commit().await.map_err(map_error)?;
    Ok(())
}

/// A table name for the rebuild's temporary stand-in that does not collide with anything already
/// in the schema (step 5 of B7 in the design spec).
async fn temp_table_name(pool: &SqlitePool, table: &str) -> Result<String, AppError> {
    let base = format!("__mixlab_rebuild_{table}");
    let mut candidate = base.clone();
    let mut suffix = 0u32;
    loop {
        let exists: Option<String> =
            sqlx::query_scalar("select name from sqlite_master where name = ?")
                .bind(&candidate)
                .fetch_optional(pool)
                .await
                .map_err(map_error)?;
        if exists.is_none() {
            return Ok(candidate);
        }
        suffix += 1;
        candidate = format!("{base}_{suffix}");
    }
}

#[cfg(test)]
mod tests {
    use super::super::sqlite::tests::Fixture;
    use super::super::sqlite_structure::table_structure;
    use super::*;

    fn column(name: &str, data_type: &str) -> ColumnSpec {
        ColumnSpec {
            name: name.to_string(),
            data_type: data_type.to_string(),
            nullable: true,
            default_value: None,
            default_is_expression: false,
            collation: None,
        }
    }

    fn index(name: &str, kind: &str, columns: &[&str]) -> IndexSpec {
        IndexSpec {
            name: name.to_string(),
            kind: kind.to_string(),
            columns: columns
                .iter()
                .map(|c| IndexColumnSpec {
                    name: (*c).to_string(),
                })
                .collect(),
        }
    }

    #[tokio::test]
    async fn a_new_table_has_the_one_column_the_rest_are_added_to() {
        let (_fixture, pool) = Fixture::open().await;
        create_table(&pool, "note").await.unwrap();
        let structure = table_structure(&pool, "note").await.unwrap();
        assert_eq!(structure.columns.len(), 1);
        assert_eq!(structure.columns[0].name, "id");
        // The rowid under a name, which is what makes it fill itself in.
        assert!(structure.columns[0].auto_increment);
    }

    #[tokio::test]
    async fn a_column_is_added_with_its_default_and_its_collation() {
        let (_fixture, pool) = Fixture::open().await;
        let mut spec = column("nickname", "TEXT");
        spec.default_value = Some("nobody".to_string());
        spec.collation = Some("NOCASE".to_string());
        add_column(&pool, "author", &spec).await.unwrap();

        let structure = table_structure(&pool, "author").await.unwrap();
        let added = structure
            .columns
            .iter()
            .find(|c| c.name == "nickname")
            .unwrap();
        // Quoted going in and unquoted coming back — see `split_default`.
        assert_eq!(added.default_value.as_deref(), Some("nobody"));
        assert!(!added.default_is_expression);
    }

    #[tokio::test]
    async fn an_expression_default_cannot_be_added_and_sqlite_says_so() {
        let (_fixture, pool) = Fixture::open().await;
        let mut spec = column("seen_at", "TEXT");
        spec.default_value = Some("CURRENT_TIMESTAMP".to_string());
        spec.default_is_expression = true;

        /* A rule of `ADD COLUMN` rather than of the column: SQLite will not add one whose default
        is not a constant, though a `CREATE TABLE` may declare exactly that. The definition
        written here is right, and the engine's own refusal is what reaches the user — it states
        the rule better than a second copy of it here would. */
        let error = add_column(&pool, "author", &spec)
            .await
            .expect_err("should refuse");
        assert_eq!(error.code, "error.sqlite");
        assert!(
            error.params["message"].contains("non-constant default"),
            "unexpected message: {}",
            error.params["message"]
        );
    }

    #[tokio::test]
    async fn renaming_a_column_is_the_one_edit_that_goes_through() {
        let (_fixture, pool) = Fixture::open().await;
        let mut spec = column("biography", "TEXT");
        spec.default_value = Some("anonymous".to_string());
        modify_column(&pool, "author", "bio", &spec).await.unwrap();

        let structure = table_structure(&pool, "author").await.unwrap();
        assert!(structure.columns.iter().any(|c| c.name == "biography"));
        assert!(!structure.columns.iter().any(|c| c.name == "bio"));
    }

    #[tokio::test]
    async fn a_type_that_differs_only_in_case_is_the_same_type() {
        let (_fixture, pool) = Fixture::open().await;
        // SQLite stores the declaration verbatim, so `text` and `TEXT` come back different and
        // mean the same — comparing them as written would refuse a plain rename.
        let mut spec = column("biography", "text");
        spec.default_value = Some("anonymous".to_string());
        modify_column(&pool, "author", "bio", &spec).await.unwrap();
    }

    #[tokio::test]
    async fn an_index_left_unnamed_is_named_after_what_it_covers() {
        let (_fixture, pool) = Fixture::open().await;
        // MySQL names one after its first column; SQLite requires a name and offers none.
        add_index(&pool, "post", &index("", "index", &["views"]))
            .await
            .unwrap();
        let structure = table_structure(&pool, "post").await.unwrap();
        assert!(structure.indexes.iter().any(|i| i.name == "post_views"));
    }

    #[tokio::test]
    async fn a_unique_index_is_created_unique() {
        let (_fixture, pool) = Fixture::open().await;
        add_index(&pool, "author", &index("author_name", "unique", &["name"]))
            .await
            .unwrap();
        let structure = table_structure(&pool, "author").await.unwrap();
        let added = structure
            .indexes
            .iter()
            .find(|i| i.name == "author_name")
            .unwrap();
        assert!(added.unique && !added.primary);
    }

    #[tokio::test]
    async fn a_primary_key_cannot_be_added_afterwards() {
        let (_fixture, pool) = Fixture::open().await;
        // Refused here rather than sent: there is no statement that would do it.
        assert_eq!(
            add_index(&pool, "loose", &index("", "primary", &["label"]))
                .await
                .expect_err("should refuse")
                .code,
            "error.sqliteNoPrimaryKeyAfterwards"
        );
    }

    #[tokio::test]
    async fn replacing_an_index_leaves_the_table_with_the_new_one() {
        let (_fixture, pool) = Fixture::open().await;
        modify_index(
            &pool,
            "post",
            "post_author",
            &index("post_author", "index", &["views"]),
        )
        .await
        .unwrap();
        let structure = table_structure(&pool, "post").await.unwrap();
        let replaced = structure
            .indexes
            .iter()
            .find(|i| i.name == "post_author")
            .unwrap();
        assert_eq!(replaced.columns[0].name.as_deref(), Some("views"));
    }

    #[tokio::test]
    async fn an_index_a_constraint_owns_cannot_be_dropped_on_its_own() {
        let (_fixture, pool) = Fixture::open().await;

        // `tag`'s key is two columns, so SQLite built an index for it — one that belongs to the
        // constraint, and that `DROP INDEX` refuses.
        let structure = table_structure(&pool, "tag").await.unwrap();
        let owned = structure.indexes[0].name.clone();
        assert_eq!(
            drop_index(&pool, "tag", &owned)
                .await
                .expect_err("owned")
                .code,
            "error.sqliteIndexBelongsToConstraint"
        );

        // And the primary key of a rowid table, which is not an index at all — it is the row the
        // Structure tab synthesises.
        assert_eq!(
            drop_index(&pool, "post", "PRIMARY")
                .await
                .expect_err("implicit")
                .code,
            "error.sqliteIndexBelongsToConstraint"
        );
    }

    #[tokio::test]
    async fn a_created_index_can_be_dropped() {
        let (_fixture, pool) = Fixture::open().await;
        drop_index(&pool, "post", "post_author").await.unwrap();
        let structure = table_structure(&pool, "post").await.unwrap();
        assert!(!structure.indexes.iter().any(|i| i.name == "post_author"));
    }

    #[tokio::test]
    async fn a_table_is_renamed_and_dropped() {
        let (_fixture, pool) = Fixture::open().await;
        rename_table(&pool, "tag", "label").await.unwrap();
        assert!(table_structure(&pool, "label").await.unwrap().columns.len() == 2);
        drop_table(&pool, "label").await.unwrap();
        assert!(table_structure(&pool, "label")
            .await
            .unwrap()
            .columns
            .is_empty());
    }

    #[tokio::test]
    async fn a_column_is_dropped() {
        let (_fixture, pool) = Fixture::open().await;
        drop_column(&pool, "author", "bio").await.unwrap();
        let structure = table_structure(&pool, "author").await.unwrap();
        assert!(!structure.columns.iter().any(|c| c.name == "bio"));
    }

    #[test]
    fn splits_simple_columns() {
        let (clauses, suffix) = split_column_clauses("CREATE TABLE t (a INTEGER, b TEXT)").unwrap();
        assert_eq!(clauses, vec!["a INTEGER", "b TEXT"]);
        assert_eq!(suffix, "");
    }

    #[test]
    fn does_not_split_a_comma_inside_a_type_arg() {
        let (clauses, _) =
            split_column_clauses("CREATE TABLE t (price DECIMAL(10,2), b TEXT)").unwrap();
        assert_eq!(clauses, vec!["price DECIMAL(10,2)", "b TEXT"]);
    }

    #[test]
    fn does_not_split_a_comma_inside_a_default_expression() {
        let (clauses, _) =
            split_column_clauses("CREATE TABLE t (a INTEGER DEFAULT (max(0, 1)))").unwrap();
        assert_eq!(clauses, vec!["a INTEGER DEFAULT (max(0, 1))"]);
    }

    #[test]
    fn does_not_split_a_comma_inside_a_line_comment() {
        let (clauses, _) =
            split_column_clauses("CREATE TABLE t (a INTEGER, -- note, with a comma\n  b TEXT)")
                .unwrap();
        assert_eq!(clauses.len(), 2);
        assert!(clauses[0].starts_with("a INTEGER"));
        assert_eq!(clauses[1], "b TEXT");
    }

    #[test]
    fn does_not_split_a_comma_inside_a_block_comment() {
        let (clauses, _) =
            split_column_clauses("CREATE TABLE t (a INTEGER /* x, y */, b TEXT)").unwrap();
        assert_eq!(clauses.len(), 2);
    }

    #[test]
    fn reads_a_quoted_column_name_with_a_space_in_it() {
        let (clauses, _) =
            split_column_clauses("CREATE TABLE t (\"full name\" TEXT, b TEXT)").unwrap();
        assert_eq!(clauses[0], "\"full name\" TEXT");
    }

    #[test]
    fn captures_the_table_constraint_clause() {
        let (clauses, _) = split_column_clauses(
            "CREATE TABLE t (id INTEGER, label TEXT, PRIMARY KEY (id, label))",
        )
        .unwrap();
        assert_eq!(clauses[2], "PRIMARY KEY (id, label)");
    }

    #[test]
    fn captures_the_table_options_suffix() {
        let (_, suffix) =
            split_column_clauses("CREATE TABLE t (a INTEGER PRIMARY KEY) WITHOUT ROWID").unwrap();
        assert_eq!(suffix, "WITHOUT ROWID");
    }

    #[test]
    fn no_suffix_is_an_empty_string() {
        let (_, suffix) = split_column_clauses("CREATE TABLE t (a INTEGER)").unwrap();
        assert_eq!(suffix, "");
    }

    #[test]
    fn an_unbalanced_statement_is_a_parse_failure() {
        assert!(split_column_clauses("CREATE TABLE t (a INTEGER").is_err());
    }

    #[test]
    fn recognises_every_table_constraint_keyword() {
        assert!(is_table_constraint("PRIMARY KEY (id)"));
        assert!(is_table_constraint("UNIQUE (a, b)"));
        assert!(is_table_constraint("CHECK(x > 0)"));
        assert!(is_table_constraint("FOREIGN KEY (a) REFERENCES t (id)"));
        assert!(is_table_constraint(
            "CONSTRAINT fk FOREIGN KEY (a) REFERENCES t (id)"
        ));
    }

    #[test]
    fn a_column_whose_name_starts_with_a_keyword_is_not_a_table_constraint() {
        assert!(!is_table_constraint("primary_email TEXT"));
        assert!(!is_table_constraint("checked_out INTEGER"));
    }

    #[test]
    fn reads_the_bare_and_quoted_column_name() {
        assert_eq!(
            clause_column_name("id INTEGER PRIMARY KEY"),
            Some("id".to_string())
        );
        assert_eq!(
            clause_column_name("\"full name\" TEXT"),
            Some("full name".to_string())
        );
        assert_eq!(
            clause_column_name("`weird` TEXT"),
            Some("weird".to_string())
        );
        assert_eq!(
            clause_column_name("[bracketed] TEXT"),
            Some("bracketed".to_string())
        );
    }

    #[test]
    fn a_table_constraint_clause_has_no_column_name_of_its_own() {
        assert_eq!(clause_column_name("PRIMARY KEY (id, label)"), None);
    }

    #[test]
    fn finds_a_column_named_in_a_constraint_clause() {
        assert!(clause_mentions_column("PRIMARY KEY (id, label)", "label"));
        assert!(clause_mentions_column(
            "FOREIGN KEY (\"author_id\") REFERENCES author (id)",
            "author_id"
        ));
    }

    #[test]
    fn does_not_match_a_column_name_that_is_only_a_substring() {
        assert!(!clause_mentions_column("PRIMARY KEY (id)", "i"));
        assert!(!clause_mentions_column("UNIQUE (author_id)", "author"));
    }

    #[test]
    fn reads_a_trailing_collate() {
        assert_eq!(
            clause_collation("bio TEXT COLLATE NOCASE"),
            Some("NOCASE".to_string())
        );
        assert_eq!(
            clause_collation("bio TEXT DEFAULT 'x' COLLATE NOCASE"),
            Some("NOCASE".to_string())
        );
    }

    #[test]
    fn no_collate_is_none() {
        assert_eq!(clause_collation("bio TEXT"), None);
    }

    #[tokio::test]
    async fn current_column_reads_a_real_collation() {
        let (_fixture, pool) = Fixture::open().await;
        sqlx::raw_sql("ALTER TABLE author ADD COLUMN nickname TEXT COLLATE NOCASE")
            .execute(&pool)
            .await
            .unwrap();
        let current = current_column(&pool, "author", "nickname").await.unwrap();
        assert_eq!(current.collation.as_deref(), Some("NOCASE"));
    }

    #[tokio::test]
    async fn current_column_reports_no_collation_when_there_is_none() {
        let (_fixture, pool) = Fixture::open().await;
        let current = current_column(&pool, "author", "bio").await.unwrap();
        assert_eq!(current.collation, None);
    }

    #[test]
    fn rebuild_blockers_refuses_a_column_named_in_a_table_constraint() {
        let (clauses, _) = split_column_clauses(
            "CREATE TABLE t (id INTEGER, label TEXT, PRIMARY KEY (id, label))",
        )
        .unwrap();
        let error = rebuild_blockers(&clauses, "label").expect_err("should refuse");
        assert_eq!(error.code, "error.sqliteColumnInTableConstraint");
    }

    #[test]
    fn rebuild_blockers_allows_a_column_outside_any_table_constraint() {
        let (clauses, _) =
            split_column_clauses("CREATE TABLE t (id INTEGER, label TEXT, note TEXT)").unwrap();
        rebuild_blockers(&clauses, "note").unwrap();
    }

    #[tokio::test]
    async fn generated_or_rowid_alias_flags_a_generated_column() {
        let (_fixture, pool) = Fixture::open().await;
        let (generated, rowid) = generated_or_rowid_alias(&pool, "post", "slug")
            .await
            .unwrap();
        assert!(generated);
        assert!(!rowid);
    }

    #[tokio::test]
    async fn generated_or_rowid_alias_flags_the_rowid_alias() {
        let (_fixture, pool) = Fixture::open().await;
        let (generated, rowid) = generated_or_rowid_alias(&pool, "author", "id")
            .await
            .unwrap();
        assert!(!generated);
        assert!(rowid);
    }

    #[tokio::test]
    async fn a_two_column_primary_key_is_not_a_rowid_alias() {
        let (_fixture, pool) = Fixture::open().await;
        // `tag.id` is declared INTEGER and leads the key, but the key is two columns — the exact
        // shape that is *not* a rowid alias (same fixture note `sqlite_structure.rs` already
        // relies on).
        let (generated, rowid) = generated_or_rowid_alias(&pool, "tag", "id").await.unwrap();
        assert!(!generated);
        assert!(!rowid);
    }

    #[tokio::test]
    async fn generated_or_rowid_alias_flags_neither_for_a_plain_column() {
        let (_fixture, pool) = Fixture::open().await;
        let (generated, rowid) = generated_or_rowid_alias(&pool, "author", "name")
            .await
            .unwrap();
        assert!(!generated);
        assert!(!rowid);
    }

    #[tokio::test]
    async fn rebuild_changes_a_columns_type_and_keeps_its_data() {
        let (_fixture, pool) = Fixture::open().await;
        // `views` holds numbers stored as INTEGER already — retype to TEXT is round-trippable
        // without loss, and lets the test assert on the affinity actually applied.
        let spec = column("views", "TEXT");
        modify_column(&pool, "post", "views", &spec).await.unwrap();
        let structure = table_structure(&pool, "post").await.unwrap();
        let views = structure
            .columns
            .iter()
            .find(|c| c.name == "views")
            .unwrap();
        assert_eq!(views.data_type, "TEXT");

        let value: String =
            sqlx::query_scalar("select views from post where title = 'Hello world'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(value, "7");
    }

    #[tokio::test]
    async fn rebuild_adds_not_null_when_no_row_violates_it() {
        let (_fixture, pool) = Fixture::open().await;
        // `post.created_at` is nullable in the fixture but every row's value came from its own
        // `DEFAULT CURRENT_TIMESTAMP` — no row is actually NULL, so NOT NULL is safe to add.
        // `author.name` was not picked here: it is already `NOT NULL` in the fixture, so setting
        // `nullable = false` on it would not change anything and would not exercise rebuild at all.
        let mut spec = column("created_at", "TEXT");
        spec.nullable = false;
        spec.default_value = Some("CURRENT_TIMESTAMP".to_string());
        spec.default_is_expression = true;
        modify_column(&pool, "post", "created_at", &spec)
            .await
            .unwrap();
        let structure = table_structure(&pool, "post").await.unwrap();
        assert!(
            !structure
                .columns
                .iter()
                .find(|c| c.name == "created_at")
                .unwrap()
                .nullable
        );
    }

    #[tokio::test]
    async fn rebuild_refuses_not_null_when_a_row_would_violate_it_and_leaves_the_table_untouched() {
        let (_fixture, pool) = Fixture::open().await;
        // `author.bio` has a NULL row (Grace) in the fixture.
        let mut spec = column("bio", "TEXT");
        spec.nullable = false;
        spec.default_value = Some("anonymous".to_string());
        let error = modify_column(&pool, "author", "bio", &spec)
            .await
            .expect_err("should refuse");
        assert_eq!(error.code, "error.sqlite");
        assert!(
            error.params["message"]
                .to_ascii_uppercase()
                .contains("NOT NULL"),
            "unexpected message: {}",
            error.params["message"]
        );

        // Rolled back cleanly: the table is exactly as it was.
        let structure = table_structure(&pool, "author").await.unwrap();
        assert!(
            structure
                .columns
                .iter()
                .find(|c| c.name == "bio")
                .unwrap()
                .nullable
        );
        let count: i64 = sqlx::query_scalar("select count(*) from author")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn rebuild_changes_a_default() {
        let (_fixture, pool) = Fixture::open().await;
        let mut spec = column("bio", "TEXT");
        spec.default_value = Some("unknown".to_string());
        modify_column(&pool, "author", "bio", &spec).await.unwrap();
        let structure = table_structure(&pool, "author").await.unwrap();
        assert_eq!(
            structure
                .columns
                .iter()
                .find(|c| c.name == "bio")
                .unwrap()
                .default_value
                .as_deref(),
            Some("unknown")
        );
    }

    #[tokio::test]
    async fn rebuild_adds_a_collation() {
        let (_fixture, pool) = Fixture::open().await;
        let mut spec = column("bio", "TEXT");
        spec.default_value = Some("anonymous".to_string());
        spec.collation = Some("NOCASE".to_string());
        modify_column(&pool, "author", "bio", &spec).await.unwrap();

        let current = current_column(&pool, "author", "bio").await.unwrap();
        assert_eq!(current.collation.as_deref(), Some("NOCASE"));
    }

    #[tokio::test]
    async fn rebuild_keeps_the_tables_indexes() {
        let (_fixture, pool) = Fixture::open().await;
        let spec = column("views", "TEXT");
        modify_column(&pool, "post", "views", &spec).await.unwrap();
        let structure = table_structure(&pool, "post").await.unwrap();
        let names: Vec<&str> = structure.indexes.iter().map(|i| i.name.as_str()).collect();
        assert!(names.contains(&"post_author"));
        assert!(names.contains(&"post_title"));
    }

    #[tokio::test]
    async fn rebuild_keeps_without_rowid() {
        let (_fixture, pool) = Fixture::open().await;
        sqlx::raw_sql("CREATE TABLE settings (name TEXT PRIMARY KEY, value TEXT) WITHOUT ROWID")
            .execute(&pool)
            .await
            .unwrap();
        let mut spec = column("value", "TEXT");
        spec.nullable = false;
        spec.default_value = Some("".to_string());
        modify_column(&pool, "settings", "value", &spec)
            .await
            .unwrap();

        let create_sql: String =
            sqlx::query_scalar("select sql from sqlite_master where name = 'settings'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(create_sql.to_ascii_uppercase().contains("WITHOUT ROWID"));
    }
}
