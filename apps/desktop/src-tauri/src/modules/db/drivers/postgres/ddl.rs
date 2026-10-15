//! Changing the shape of a PostgreSQL database: its tables, their columns and their indexes.
//!
//! The specs come in as `mysql/structure.rs`'s do, because one set of dialogs fills either. What
//! differs is everything about how they are carried out, and three differences shape this module:
//!
//! * **A column is changed one property at a time.** MySQL's `CHANGE COLUMN` restates a column
//!   whole; PostgreSQL has no such statement, so [`modify_column`] reads what the column is now,
//!   compares it with what is being asked for, and emits a clause per difference. That is also what
//!   keeps it from rewriting a large table to set a type the column already has.
//! * **A comment is its own statement.** `COMMENT ON` rather than a clause inside the definition,
//!   so almost everything here runs as several statements — inside a transaction, which PostgreSQL
//!   unlike MySQL will roll DDL back out of.
//! * **An index is not part of its table.** `CREATE INDEX` stands alone and the index lives in a
//!   schema of its own accord, so it is named and dropped separately from the table it covers.

use super::{map_error, qualified_sql, quote_ident, resolve, Pools, FALLBACK_DATABASE};
use crate::error::AppError;
use serde::Deserialize;
use sqlx::{PgPool, Row};

/// What a column is to be declared as — `mysql::structure::ColumnSpec` minus the two fields that
/// mean nothing here, since one dialog fills both.
///
/// Those two are `onUpdateCurrentTimestamp`, a MySQL clause with no counterpart, and `after`, a
/// position PostgreSQL will not put a column in — it appends, always. The dialog does not offer
/// either on a PostgreSQL connection; if one arrived anyway it would be ignored rather than
/// refused, which is serde's default for a field the struct has not got.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnSpec {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    /// `None` writes no DEFAULT at all — and, on an existing column, drops the one it has.
    #[serde(default)]
    pub default_value: Option<String>,
    /// Emits the default as an expression rather than as a quoted literal. Everything PostgreSQL
    /// reports back is an expression — it stores even a literal default cast, as `'new'::text` —
    /// so a column being edited comes back through here unchanged.
    #[serde(default)]
    pub default_is_expression: bool,
    /// An identity column: one the server numbers itself.
    #[serde(default)]
    pub auto_increment: bool,
    #[serde(default)]
    pub collation: Option<String>,
    #[serde(default)]
    pub comment: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexColumnSpec {
    pub name: String,
}

/// What an index is to be created as.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexSpec {
    /// Left empty, PostgreSQL names the index after the table and its columns. Ignored for a
    /// primary key, which is named by the constraint that carries it.
    #[serde(default)]
    pub name: String,
    /// `index`, `unique` or `primary`. MySQL's `fulltext` and `spatial` have no counterpart as
    /// *kinds* here — the nearest thing is an access method, which is what `index_type` picks.
    pub kind: String,
    /// The access method: `btree`, `hash`, `gin`, `gist`, `spgist`, `brin`, or `None` for the
    /// server's default. Not meaningful for a primary key, which is always a btree.
    #[serde(default)]
    pub index_type: Option<String>,
    pub columns: Vec<IndexColumnSpec>,
    #[serde(default)]
    pub comment: String,
}

/// Wraps text as a SQL string literal, doubling the quote that would otherwise end it early.
///
/// A backslash is left alone, unlike MySQL's: in PostgreSQL a plain `'...'` literal is not
/// backslash-escaped, so doubling them would store two where the user typed one.
fn quote_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// The access methods an index may be built with, as PostgreSQL names them.
const INDEX_METHODS: [&str; 6] = ["btree", "hash", "gin", "gist", "spgist", "brin"];

/// The access method as it may be written into `USING`, or `None` for the server's own default.
///
/// Checked against the list rather than quoted, because an access method is named bare in
/// PostgreSQL's grammar and so cannot be escaped into safety.
fn validated_method(index_type: Option<&str>) -> Result<Option<&'static str>, AppError> {
    let Some(index_type) = index_type.map(str::trim).filter(|t| !t.is_empty()) else {
        return Ok(None);
    };
    let lower = index_type.to_lowercase();
    INDEX_METHODS
        .iter()
        .find(|method| **method == lower)
        .copied()
        .map(Some)
        .ok_or_else(|| err!("error.unknownIndexType", type = index_type))
}

/// How a DEFAULT reaches the DDL.
///
/// An expression goes in as written — that is what PostgreSQL hands back when asked what a column's
/// default is, so a column being edited round-trips through here untouched. Anything else is a
/// literal and is quoted, except `NULL` typed on its own, which is meant as SQL NULL rather than as
/// the four characters.
fn default_clause(value: &str, is_expression: bool) -> String {
    let trimmed = value.trim();
    if is_expression {
        return trimmed.to_string();
    }
    if trimmed.eq_ignore_ascii_case("NULL") {
        return "NULL".to_string();
    }
    quote_string(value)
}

/// The `COLLATE "..."` of a column, empty when none was asked for.
///
/// Double-quoted, unlike MySQL's bare name: a PostgreSQL collation is an identifier, lives in a
/// schema, and is commonly called something (`en_US.utf8`) that cannot be written unquoted at all.
/// Quoting is also what makes checking the name unnecessary — there is nothing it could break out
/// of.
fn collate_clause(collation: Option<&str>) -> String {
    match collation.map(str::trim).filter(|c| !c.is_empty()) {
        Some(collation) => format!(" COLLATE {}", quote_ident(collation)),
        None => String::new(),
    }
}

/// The `name type ...` of a column, as a CREATE TABLE or ADD COLUMN spells it.
///
/// The type is interpolated as the user wrote it: PostgreSQL's type grammar is far too large to
/// model, and this client runs user-authored SQL by design. The comment is not here — it is a
/// statement of its own; see [`comment_on`].
fn column_definition(spec: &ColumnSpec) -> Result<String, AppError> {
    let name = spec.name.trim();
    if name.is_empty() {
        return Err(err!("error.columnNameRequired"));
    }
    let data_type = spec.data_type.trim();
    if data_type.is_empty() {
        return Err(err!("error.columnTypeRequired"));
    }

    let mut sql = format!("{} {data_type}", quote_ident(name));
    sql.push_str(&collate_clause(spec.collation.as_deref()));
    if spec.auto_increment {
        // An identity column is NOT NULL by definition and may not also carry a DEFAULT, so the
        // two clauses below are not written for one.
        sql.push_str(" GENERATED BY DEFAULT AS IDENTITY");
        return Ok(sql);
    }
    sql.push_str(if spec.nullable { " NULL" } else { " NOT NULL" });
    if let Some(default) = &spec.default_value {
        sql.push_str(&format!(
            " DEFAULT {}",
            default_clause(default, spec.default_is_expression)
        ));
    }
    Ok(sql)
}

/// A `COMMENT ON` statement, or `None` when there is nothing to say and nothing to clear.
///
/// `target` is the whole `COLUMN "s"."t"."c"` or `INDEX "s"."i"` — this only decides how the text
/// goes on the end of it. An empty comment becomes `IS NULL`, which is how PostgreSQL spells
/// "no comment": passing the empty string instead would leave the object commented with nothing.
fn comment_on(target: String, comment: &str) -> String {
    let comment = comment.trim();
    let value = if comment.is_empty() {
        "NULL".to_string()
    } else {
        quote_string(comment)
    };
    format!("COMMENT ON {target} IS {value}")
}

/// Runs statements in the order given, all or none.
///
/// A transaction because most of what this module does takes more than one statement to say — a
/// column and its comment, an index and its comment, a rename and a move — and PostgreSQL, unlike
/// MySQL, will roll DDL back. Half a change is never left behind.
async fn execute_all(pool: &PgPool, statements: Vec<String>) -> Result<(), AppError> {
    if statements.is_empty() {
        return Ok(());
    }
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
    tx.commit().await.map_err(map_error)
}

/// Creates a database.
///
/// `CREATE DATABASE` cannot run inside a transaction, so it is the one statement here sent on its
/// own. The collation the dialog offers is not passed on: a PostgreSQL database's collation is a
/// locale of the host operating system rather than a name from a list, and setting one demands
/// `TEMPLATE template0` and an encoding to go with it. A database created here therefore takes the
/// server's own, which is what `CREATE DATABASE` without a clause means.
pub async fn create_database(pool: &PgPool, name: &str) -> Result<(), AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(err!("error.databaseNameRequired"));
    }
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "CREATE DATABASE {}",
        quote_ident(name)
    )))
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(map_error)
}

/// Drops a database and every table in it.
///
/// Takes the whole connection rather than a pool, because of what has to happen first: PostgreSQL
/// refuses to drop a database anyone is connected to, and this client is very likely one of them —
/// the sidebar opens a pool on a database the moment it is looked at. So the pool for that database
/// is closed and forgotten, and the drop goes out over another one.
///
/// Other sessions are not disturbed. If someone else is connected, the server says so and the drop
/// fails, which is the right answer: their work is not this client's to end.
pub async fn drop_database(pools: &Pools, name: &str) -> Result<(), AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(err!("error.databaseNameRequired"));
    }

    pools.close_pool(name).await;

    // The statement has to run from somewhere else. Normally that is the database the connection
    // was opened on; when that is the one being dropped, it is the maintenance database every
    // server has.
    let default = pools.default_database();
    let dropping_default = default == name;
    let from = if dropping_default {
        FALLBACK_DATABASE
    } else {
        default.as_str()
    };
    let pool = pools.pool(Some(from)).await?;

    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE {}",
        quote_ident(name)
    )))
    .execute(&pool)
    .await
    .map_err(map_error)?;

    // Only once the server has accepted it: what the connection means by "no database named" was
    // the one just dropped, and every later command — the sidebar's own listing among them — would
    // dial a database that is gone.
    if dropping_default {
        pools.forget_default_database();
    }
    Ok(())
}

/// Creates a table with nothing in it but the key every table ends up wanting: an `integer` `id`
/// the server numbers itself. The rest of the columns are added from the Structure tab afterwards.
///
/// The name may carry a schema, exactly as the sidebar writes one — `sales.orders` creates the
/// table in `sales`, and a bare name creates it in `public`. That is the only way to reach another
/// schema from a dialog with one name box in it, and it costs nothing: the same spelling already
/// means the same thing everywhere else in the workspace.
///
/// The collation the dialog offers is not passed on. A PostgreSQL table has no collation of its
/// own — only its individual text columns do, which the column editor sets.
pub async fn create_table(pool: &PgPool, table: &str) -> Result<(), AppError> {
    if table.trim().is_empty() {
        return Err(err!("error.tableNameRequired"));
    }
    let (schema, name) = resolve(table.trim());
    let sql = format!(
        "CREATE TABLE {} (\"id\" integer GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY)",
        qualified_sql(&schema, &name)
    );
    execute_all(pool, vec![sql]).await
}

/// Renames a table, and moves it if the new name carries a different schema.
///
/// Two statements rather than one, since PostgreSQL separates the two halves: `RENAME TO` takes a
/// bare name and cannot cross schemas, and `SET SCHEMA` moves a table without renaming it. In one
/// transaction, so a rename that cannot be moved does not happen either.
pub async fn rename_table(pool: &PgPool, table: &str, new_name: &str) -> Result<(), AppError> {
    if table.trim().is_empty() || new_name.trim().is_empty() {
        return Err(err!("error.tableNameRequired"));
    }
    let (schema, name) = resolve(table.trim());
    let (new_schema, new_bare) = resolve(new_name.trim());

    let mut statements = Vec::new();
    let mut current = qualified_sql(&schema, &name);
    if new_bare != name {
        statements.push(format!(
            "ALTER TABLE {current} RENAME TO {}",
            quote_ident(&new_bare)
        ));
        current = qualified_sql(&schema, &new_bare);
    }
    if new_schema != schema {
        statements.push(format!(
            "ALTER TABLE {current} SET SCHEMA {}",
            quote_ident(&new_schema)
        ));
    }
    execute_all(pool, statements).await
}

/// Drops a table and everything in it. Plain `DROP TABLE`, not `IF EXISTS`: asking to drop
/// something that is not there is worth being told about rather than passing quietly.
pub async fn drop_table(pool: &PgPool, table: &str) -> Result<(), AppError> {
    if table.trim().is_empty() {
        return Err(err!("error.tableNameRequired"));
    }
    let (schema, name) = resolve(table.trim());
    execute_all(
        pool,
        vec![format!("DROP TABLE {}", qualified_sql(&schema, &name))],
    )
    .await
}

pub async fn add_column(pool: &PgPool, table: &str, spec: &ColumnSpec) -> Result<(), AppError> {
    let (schema, name) = resolve(table);
    let qualified = qualified_sql(&schema, &name);
    let definition = column_definition(spec)?;
    let column = quote_ident(spec.name.trim());

    let mut statements = vec![format!("ALTER TABLE {qualified} ADD COLUMN {definition}")];
    if !spec.comment.trim().is_empty() {
        statements.push(comment_on(
            format!("COLUMN {qualified}.{column}"),
            &spec.comment,
        ));
    }
    execute_all(pool, statements).await
}

/// What a column is now, as far as [`modify_column`] has to care.
struct CurrentColumn {
    data_type: String,
    nullable: bool,
    default_value: Option<String>,
    identity: bool,
    collation: Option<String>,
    comment: String,
}

async fn current_column(
    pool: &PgPool,
    schema: &str,
    table: &str,
    column: &str,
) -> Result<CurrentColumn, AppError> {
    let row = sqlx::query(
        "SELECT format_type(a.atttypid, a.atttypmod) AS data_type,
                NOT a.attnotnull AS nullable,
                pg_get_expr(d.adbin, d.adrelid) AS default_value,
                a.attidentity <> '' AS identity,
                coll.collname AS collation,
                COALESCE(col_description(a.attrelid, a.attnum), '') AS comment
         FROM pg_attribute a
         JOIN pg_class c ON c.oid = a.attrelid
         JOIN pg_namespace n ON n.oid = c.relnamespace
         JOIN pg_type tp ON tp.oid = a.atttypid
         LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
         -- Only an explicit collation, matching what the Structure tab reported and so what the
         -- dialog was filled from. See `structure::structure_columns`.
         LEFT JOIN pg_collation coll
             ON coll.oid = a.attcollation
             AND a.attcollation NOT IN (0, tp.typcollation)
         WHERE n.nspname = $1 AND c.relname = $2 AND a.attname = $3
           AND a.attnum > 0 AND NOT a.attisdropped",
    )
    .bind(schema)
    .bind(table)
    .bind(column)
    .fetch_optional(pool)
    .await
    .map_err(map_error)?
    .ok_or_else(|| err!("error.unknownColumn", table = table, name = column))?;

    Ok(CurrentColumn {
        data_type: row.get("data_type"),
        nullable: row.get("nullable"),
        default_value: row.get("default_value"),
        identity: row.get("identity"),
        collation: row.get("collation"),
        comment: row.get("comment"),
    })
}

/// Redefines an existing column, `name` being what it is called now — so this is also how a column
/// is renamed.
///
/// PostgreSQL has no statement that restates a column whole, so what the column is now is read
/// first and only the differences are written. That is not merely tidier: `ALTER COLUMN ... TYPE`
/// rewrites every row of the table and holds a lock over the whole of it, and emitting one
/// unconditionally would make saving a comment cost as much as changing the type.
///
/// The order the clauses go out in matters. The rename is first, so everything after it can use
/// the new name. Identity is dropped before the default is touched and added after the NOT NULL is
/// in place, because PostgreSQL will not let a column have both an identity and a default, and will
/// not make a nullable column an identity at all.
pub async fn modify_column(
    pool: &PgPool,
    table: &str,
    name: &str,
    spec: &ColumnSpec,
) -> Result<(), AppError> {
    let old_name = name.trim();
    let new_name = spec.name.trim();
    if old_name.is_empty() || new_name.is_empty() {
        return Err(err!("error.columnNameRequired"));
    }
    let data_type = spec.data_type.trim();
    if data_type.is_empty() {
        return Err(err!("error.columnTypeRequired"));
    }

    let (schema, table_name) = resolve(table);
    let qualified = qualified_sql(&schema, &table_name);
    let current = current_column(pool, &schema, &table_name, old_name).await?;
    let column = quote_ident(new_name);

    let mut statements = Vec::new();
    if new_name != old_name {
        statements.push(format!(
            "ALTER TABLE {qualified} RENAME COLUMN {} TO {column}",
            quote_ident(old_name)
        ));
    }

    let collation = spec
        .collation
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty());
    // A type that reads the same as the one the column has is not a change — `format_type` is how
    // PostgreSQL itself spells a type, and the dialog was filled from it.
    if data_type != current.data_type || collation != current.collation.as_deref() {
        // `USING` is what carries the existing values across. Without one PostgreSQL only accepts
        // the conversions it considers implicit, which rules out most of the changes worth making
        // — text to integer among them.
        statements.push(format!(
            "ALTER TABLE {qualified} ALTER COLUMN {column} TYPE {data_type}{} USING {column}::{data_type}",
            collate_clause(collation)
        ));
    }

    if current.identity && !spec.auto_increment {
        statements.push(format!(
            "ALTER TABLE {qualified} ALTER COLUMN {column} DROP IDENTITY"
        ));
    }

    // An identity column is NOT NULL whatever the dialog says, since PostgreSQL will not have it
    // otherwise.
    let nullable = spec.nullable && !spec.auto_increment;
    if nullable != current.nullable {
        statements.push(format!(
            "ALTER TABLE {qualified} ALTER COLUMN {column} {} NOT NULL",
            if nullable { "DROP" } else { "SET" }
        ));
    }

    // A `serial` column numbers itself without being an identity column: what does it is an
    // ordinary default drawing from a sequence. Reading it as neither is what would drop that
    // default and add an identity beside it — a second sequence, starting at 1, so the next insert
    // collides with rows that are already there. And it would happen on any edit at all, a comment
    // among them, since every statement below is written from the difference between the dialog and
    // the column.
    let serial = !current.identity
        && current
            .default_value
            .as_deref()
            .is_some_and(|d| d.starts_with("nextval("));

    // An identity column carries no default of its own: the sequence is its default.
    let wanted_default = if spec.auto_increment {
        None
    } else {
        spec.default_value
            .as_deref()
            .map(|value| default_clause(value, spec.default_is_expression))
    };
    match wanted_default {
        Some(default) => statements.push(format!(
            "ALTER TABLE {qualified} ALTER COLUMN {column} SET DEFAULT {default}"
        )),
        // Only where there is one to drop — on a column that has none, this would be a statement
        // that says nothing, and on an identity column PostgreSQL refuses it outright. A `serial`
        // the dialog still wants numbered keeps its default too: that default *is* the numbering.
        None if current.default_value.is_some()
            && !current.identity
            && !(serial && spec.auto_increment) =>
        {
            statements.push(format!(
                "ALTER TABLE {qualified} ALTER COLUMN {column} DROP DEFAULT"
            ))
        }
        None => {}
    }

    if spec.auto_increment && !current.identity && !serial {
        statements.push(format!(
            "ALTER TABLE {qualified} ALTER COLUMN {column} ADD GENERATED BY DEFAULT AS IDENTITY"
        ));
    }

    if spec.comment.trim() != current.comment {
        statements.push(comment_on(
            format!("COLUMN {qualified}.{column}"),
            &spec.comment,
        ));
    }

    execute_all(pool, statements).await
}

pub async fn drop_column(pool: &PgPool, table: &str, name: &str) -> Result<(), AppError> {
    if name.trim().is_empty() {
        return Err(err!("error.columnNameRequired"));
    }
    let (schema, table_name) = resolve(table);
    execute_all(
        pool,
        vec![format!(
            "ALTER TABLE {} DROP COLUMN {}",
            qualified_sql(&schema, &table_name),
            quote_ident(name.trim())
        )],
    )
    .await
}

/// The statements that create one index — an `ALTER TABLE` for a primary key, a `CREATE INDEX` for
/// everything else, and a `COMMENT ON` after it when there is one.
///
/// An index lives in the schema of the table it covers, so the name is never given a schema of its
/// own: PostgreSQL rejects one on `CREATE INDEX`, and puts the index beside its table regardless.
fn create_index_statements(
    schema: &str,
    table: &str,
    spec: &IndexSpec,
) -> Result<Vec<String>, AppError> {
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
            Ok(quote_ident(column.name.trim()))
        })
        .collect::<Result<Vec<_>, AppError>>()?
        .join(", ");

    let qualified = qualified_sql(schema, table);
    let name = spec.name.trim();
    let kind = spec.kind.to_lowercase();

    if kind == "primary" {
        // A primary key is a constraint rather than an index asked for directly; the index behind
        // it is made by the server, always a btree, and named after the constraint.
        let named = if name.is_empty() {
            String::new()
        } else {
            format!("CONSTRAINT {} ", quote_ident(name))
        };
        let mut statements = vec![format!(
            "ALTER TABLE {qualified} ADD {named}PRIMARY KEY ({columns})"
        )];
        if !name.is_empty() && !spec.comment.trim().is_empty() {
            statements.push(comment_on(
                format!("INDEX {}", qualified_sql(schema, name)),
                &spec.comment,
            ));
        }
        return Ok(statements);
    }

    let unique = match kind.as_str() {
        "index" => "",
        "unique" => "UNIQUE ",
        other => return Err(err!("error.unknownIndexKind", kind = other)),
    };
    // Left unnamed, PostgreSQL names the index after its table and columns — which is what a client
    // making one up would have had to guess at anyway.
    let named = if name.is_empty() {
        String::new()
    } else {
        format!("{} ", quote_ident(name))
    };
    let using = match validated_method(spec.index_type.as_deref())? {
        Some(method) => format!(" USING {method}"),
        None => String::new(),
    };

    let mut statements = vec![format!(
        "CREATE {unique}INDEX {named}ON {qualified}{using} ({columns})"
    )];
    // An unnamed index has no name to comment on until the server has picked one, and looking it
    // up afterwards would be guesswork; a comment therefore needs a name to hang on.
    if !name.is_empty() && !spec.comment.trim().is_empty() {
        statements.push(comment_on(
            format!("INDEX {}", qualified_sql(schema, name)),
            &spec.comment,
        ));
    }
    Ok(statements)
}

/// The statement that removes an index by name.
///
/// Which statement it is depends on how the index came about. One made by a `PRIMARY KEY` or a
/// `UNIQUE` constraint belongs to that constraint, and PostgreSQL refuses to drop it directly —
/// it has to go through the constraint. One made by `CREATE INDEX` has no constraint and is
/// dropped as itself. So the catalogue is asked which it is rather than guessed at.
///
/// What it is asked is whether a constraint *owns this index* — `con.conindid` — rather than
/// whether one merely happens to be called the same thing. The two names live in different places:
/// an index is a `pg_class` row, unique within its schema, and a constraint a `pg_constraint` row,
/// unique within its table. Nothing stops a table carrying a `CHECK` named exactly what one of its
/// indexes is, and matching on the name alone would answer that dropping the index means dropping
/// that check — leaving the check gone and the index still there.
async fn drop_index_statement(
    pool: &PgPool,
    schema: &str,
    table: &str,
    name: &str,
) -> Result<String, AppError> {
    let constrained: bool = sqlx::query_scalar(
        "SELECT EXISTS (
             SELECT 1
             FROM pg_constraint con
             JOIN pg_class c ON c.oid = con.conrelid
             JOIN pg_namespace n ON n.oid = c.relnamespace
             JOIN pg_class ic ON ic.oid = con.conindid
             WHERE n.nspname = $1 AND c.relname = $2 AND ic.relname = $3
         )",
    )
    .bind(schema)
    .bind(table)
    .bind(name)
    .fetch_one(pool)
    .await
    .map_err(map_error)?;

    Ok(if constrained {
        format!(
            "ALTER TABLE {} DROP CONSTRAINT {}",
            qualified_sql(schema, table),
            quote_ident(name)
        )
    } else {
        format!("DROP INDEX {}", qualified_sql(schema, name))
    })
}

pub async fn add_index(pool: &PgPool, table: &str, spec: &IndexSpec) -> Result<(), AppError> {
    let (schema, name) = resolve(table);
    let statements = create_index_statements(&schema, &name, spec)?;
    execute_all(pool, statements).await
}

/// Replaces an index. PostgreSQL cannot alter one in place, so the old index is dropped and the new
/// one created — both inside the one transaction, which is what keeps the table from being seen
/// without either.
pub async fn modify_index(
    pool: &PgPool,
    table: &str,
    name: &str,
    spec: &IndexSpec,
) -> Result<(), AppError> {
    let old_name = name.trim();
    if old_name.is_empty() {
        return Err(err!("error.indexNameRequired"));
    }
    let (schema, table_name) = resolve(table);
    let mut statements = vec![drop_index_statement(pool, &schema, &table_name, old_name).await?];
    statements.extend(create_index_statements(&schema, &table_name, spec)?);
    execute_all(pool, statements).await
}

pub async fn drop_index(pool: &PgPool, table: &str, name: &str) -> Result<(), AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(err!("error.indexNameRequired"));
    }
    let (schema, table_name) = resolve(table);
    let statement = drop_index_statement(pool, &schema, &table_name, name).await?;
    execute_all(pool, vec![statement]).await
}

/// What is decided here, rather than by a server's answer.
#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str, data_type: &str) -> ColumnSpec {
        ColumnSpec {
            name: name.to_string(),
            data_type: data_type.to_string(),
            nullable: true,
            default_value: None,
            default_is_expression: false,
            auto_increment: false,
            collation: None,
            comment: String::new(),
        }
    }

    #[test]
    fn a_plain_column_is_named_typed_and_nullable() {
        assert_eq!(
            column_definition(&spec("title", "text")).unwrap(),
            "\"title\" text NULL"
        );
    }

    #[test]
    fn an_identity_column_takes_neither_null_nor_default() {
        let mut spec = spec("id", "integer");
        spec.auto_increment = true;
        spec.default_value = Some("7".to_string());
        assert_eq!(
            column_definition(&spec).unwrap(),
            "\"id\" integer GENERATED BY DEFAULT AS IDENTITY"
        );
    }

    #[test]
    fn a_collation_is_quoted_as_the_identifier_it_is() {
        let mut spec = spec("name", "text");
        spec.collation = Some("en_US.utf8".to_string());
        assert_eq!(
            column_definition(&spec).unwrap(),
            "\"name\" text COLLATE \"en_US.utf8\" NULL"
        );
    }

    #[test]
    fn a_literal_default_is_quoted_and_an_expression_is_not() {
        assert_eq!(default_clause("new", false), "'new'");
        assert_eq!(default_clause("it's", false), "'it''s'");
        assert_eq!(default_clause("now()", true), "now()");
        assert_eq!(default_clause("NULL", false), "NULL");
    }

    #[test]
    fn a_backslash_in_a_literal_stays_one() {
        // Unlike MySQL: a plain PostgreSQL literal is not backslash-escaped, so doubling it here
        // would store two where the user typed one.
        assert_eq!(default_clause("a\\b", false), "'a\\b'");
    }

    #[test]
    fn an_empty_comment_clears_rather_than_sets() {
        assert_eq!(
            comment_on("COLUMN \"public\".\"t\".\"c\"".to_string(), "  "),
            "COMMENT ON COLUMN \"public\".\"t\".\"c\" IS NULL"
        );
    }

    #[test]
    fn only_a_known_access_method_reaches_the_sql() {
        assert_eq!(validated_method(Some("GIN")).unwrap(), Some("gin"));
        assert_eq!(validated_method(Some("  ")).unwrap(), None);
        assert!(validated_method(Some("btree; DROP")).is_err());
    }

    #[test]
    fn a_unique_index_names_its_method_and_columns() {
        let spec = IndexSpec {
            name: "orders_customer".to_string(),
            kind: "unique".to_string(),
            index_type: Some("btree".to_string()),
            columns: vec![
                IndexColumnSpec {
                    name: "customer".to_string(),
                },
                IndexColumnSpec {
                    name: "placed".to_string(),
                },
            ],
            comment: String::new(),
        };
        assert_eq!(
            create_index_statements("sales", "orders", &spec).unwrap(),
            vec![
                "CREATE UNIQUE INDEX \"orders_customer\" ON \"sales\".\"orders\" USING btree (\"customer\", \"placed\")"
            ]
        );
    }

    #[test]
    fn a_primary_key_is_added_to_the_table_rather_than_created() {
        let spec = IndexSpec {
            name: String::new(),
            kind: "primary".to_string(),
            index_type: Some("btree".to_string()),
            columns: vec![IndexColumnSpec {
                name: "id".to_string(),
            }],
            comment: String::new(),
        };
        assert_eq!(
            create_index_statements("public", "t", &spec).unwrap(),
            vec!["ALTER TABLE \"public\".\"t\" ADD PRIMARY KEY (\"id\")"]
        );
    }

    #[test]
    fn an_index_kind_postgres_has_not_is_refused() {
        let spec = IndexSpec {
            name: String::new(),
            kind: "fulltext".to_string(),
            index_type: None,
            columns: vec![IndexColumnSpec {
                name: "body".to_string(),
            }],
            comment: String::new(),
        };
        assert!(create_index_statements("public", "t", &spec).is_err());
    }
}
