//! Dumping a SQL Server database to a `.sql` file and replaying one back in — no external tool
//! (D10): Microsoft ships nothing free and pinnable equivalent to `pg_dump`/`mysqldump`, so this
//! regenerates `CREATE TABLE`/`CREATE INDEX` from the same catalogue reads Plan 2's Structure tab
//! already makes (`mssql_structure::table_structure`) and reuses Plan 6's statement builders
//! (`mssql_ddl::column_definition`/`create_index_statements`) rather than inventing a second way to
//! spell either. Closest in shape to `sqlite_dump.rs`, not `clickhouse_dump.rs`: both this file and
//! SQLite's read rows one Rust value at a time through a normal driver, where ClickHouse's
//! `FORMAT SQLInsert` has the server generate the `INSERT` text itself — so this file, like
//! `sqlite_dump.rs`, writes its own value-to-literal conversion (see [`sql_literal`]).

use super::dump::{self, Tracker};
use super::mssql::{
    column_value, is_binary_type, map_error, primary_key, quote_ident, read_uncommitted, resolve,
    select_expr, table_columns, three_part, Pool, DEFAULT_SCHEMA,
};
use super::mssql_ddl::{column_definition, comment_statement, quote_string, ColumnSpec};
use super::mssql_script;
use super::mssql_structure::{self, StructureColumn, TableIndex};
use crate::error::AppError;
use std::collections::HashMap;

/// A table named `schema.table` for the *text this file writes into the dump*, deliberately not
/// `mssql::three_part` — that helper bakes in a specific database name, which is exactly right for
/// live Structure-tab DDL (always run against the database currently open) and exactly wrong for a
/// dump file, which restores under `mssql_script::run`'s own `USE {target}` and must not fight it
/// with a hardcoded reference back to the database it was dumped from. Confirmed against the live
/// test server (see this plan's Task 7): a first cut of this file used `three_part` here, and a
/// restore into a fresh database silently re-ran every `CREATE TABLE`/`INSERT` against the
/// *original* source database instead — no error until a table happened to already exist there.
/// Every table this file names is written this way; only the catalogue reads below (`{db}.sys.*`)
/// still carry a database, because those really do cross databases from one pooled connection.
fn qualified_ident(schema: &str, table: &str) -> String {
    format!("{}.{}", quote_ident(schema), quote_ident(table))
}

/// The statement that recreates one already-existing index verbatim, from what
/// `mssql_structure::table_structure` read back — not `mssql_ddl::create_index_statements`, which
/// is built for a dialog's freshly-typed `IndexSpec` (an empty name still to be generated, an index
/// type that still needs validating) and, more importantly, always qualifies through a specific
/// database name (the same [`qualified_ident`] problem this file works around everywhere else).
/// Every field read off a real index already has a name and a concrete `CLUSTERED`/`NONCLUSTERED`
/// type — SQL Server never leaves either blank on an existing one — so none of that dialog-facing
/// machinery is needed to just spell it back out.
/// A `CREATE SCHEMA`, guarded to run only when the schema is not already there. Found necessary by
/// the live verification in this plan's Task 7, not anticipated by the plan itself: the plan's
/// draft treated "restores only into a database where the schema already exists" as an accepted
/// non-goal, on the assumption the test database used only `dbo` — which turned out to be wrong, so
/// the limitation is fixed here instead of left in.
///
/// `EXEC('...')` rather than a plain `CREATE SCHEMA`: T-SQL only allows `CREATE SCHEMA` as the sole
/// statement of a batch, and every other line in this file's output sits one-per-line inside a
/// single batch `mssql_script::run` (Task 5) splits on `;` — wrapping it as dynamic SQL runs it as a
/// batch of its own inline, without this file needing to know anything about batch boundaries.
fn create_schema_statement(schema: &str) -> String {
    let literal = format!("N{}", quote_string(schema));
    let create = format!("CREATE SCHEMA {}", quote_ident(schema)).replace('\'', "''");
    format!("IF NOT EXISTS (SELECT 1 FROM sys.schemas WHERE name = {literal}) EXEC('{create}')")
}

fn dump_index_statement(schema: &str, table: &str, index: &TableIndex) -> String {
    let qualified = qualified_ident(schema, table);
    let columns = index
        .columns
        .iter()
        .filter_map(|c| c.name.as_deref())
        .map(quote_ident)
        .collect::<Vec<_>>()
        .join(", ");
    let using = format!("{} ", index.index_type.to_uppercase());
    if index.primary {
        return format!(
            "ALTER TABLE {qualified} ADD CONSTRAINT {} PRIMARY KEY {using}({columns})",
            quote_ident(&index.name)
        );
    }
    let unique = if index.unique { "UNIQUE " } else { "" };
    format!(
        "CREATE {unique}{using}INDEX {} ON {qualified} ({columns})",
        quote_ident(&index.name)
    )
}

/// Turns a read-back [`StructureColumn`] into the [`ColumnSpec`] shape `column_definition` (Plan 6)
/// was written for — the two structs carry the same fields under the same names by construction
/// (`mssql_ddl::ColumnSpec` is `structure_columns`' write-side counterpart), so this is a plain
/// field-for-field copy, not a translation.
///
/// Never called for a `generated` column — see [`computed_column_definition`], which a computed
/// column's entry in `TableStructure::columns` is routed to instead. `default_value`/
/// `default_is_expression` on a computed column do not carry its expression (Beyond the letter of
/// the spec #4 of this plan) and would produce a plain column with a wrong or missing default if
/// this ran on one.
fn column_spec_from(column: &StructureColumn) -> ColumnSpec {
    ColumnSpec {
        name: column.name.clone(),
        data_type: column.data_type.clone(),
        nullable: column.nullable,
        default_value: column.default_value.clone(),
        default_is_expression: column.default_is_expression,
        auto_increment: column.auto_increment,
        collation: column.collation.clone(),
        comment: column.comment.clone(),
    }
}

/// One computed column's expression, read separately from `mssql_structure::structure_columns`
/// because that read's `default_value` comes from `sys.default_constraints` — which a computed
/// column has no row in at all. The expression lives in `sys.computed_columns.definition` instead.
struct ComputedDefinition {
    definition: String,
    persisted: bool,
}

async fn computed_definitions(
    pool: &Pool,
    database: &str,
    schema: &str,
    table: &str,
) -> Result<HashMap<String, ComputedDefinition>, AppError> {
    let db = quote_ident(database);
    let sql = read_uncommitted(&format!(
        "SELECT c.name, cc.definition, cc.is_persisted
         FROM {db}.sys.computed_columns cc
         JOIN {db}.sys.objects o ON o.object_id = cc.object_id
         JOIN {db}.sys.schemas s ON s.schema_id = o.schema_id
         JOIN {db}.sys.columns c ON c.object_id = cc.object_id AND c.column_id = cc.column_id
         WHERE s.name = @P1 AND o.name = @P2"
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

    Ok(rows
        .iter()
        .filter_map(|row| {
            let name: &str = row.get("name")?;
            let definition: &str = row.get("definition")?;
            Some((
                name.to_string(),
                ComputedDefinition {
                    definition: definition.to_string(),
                    persisted: row.get("is_persisted").unwrap_or(false),
                },
            ))
        })
        .collect())
}

/// A computed column's declaration: `name AS (expression)`, `PERSISTED` appended when the server
/// stores rather than recomputes it — the one distinction the spec's top-level non-goals say the
/// Structure tab does not let a user change, but a dump still has to spell correctly to reproduce
/// the table as it is.
fn computed_column_definition(name: &str, computed: &ComputedDefinition) -> String {
    let mut sql = format!("{} AS ({})", quote_ident(name), computed.definition);
    if computed.persisted {
        sql.push_str(" PERSISTED");
    }
    sql
}

/// Writes every base table's `CREATE TABLE` (columns and computed columns), its indexes, and — once
/// every table is written — every foreign key as a trailing `ALTER TABLE ... ADD CONSTRAINT` (a
/// later task in this plan fills [`write_foreign_keys`] in; this task leaves the call site for it).
///
/// Base tables only (`sys.objects.type = 'U'`, via [`mssql_structure::table_stats`], which already
/// excludes views for the same reason the Statistics tab does). Every non-`dbo` schema a dumped
/// table lives in gets a guarded `CREATE SCHEMA` up front (see [`create_schema_statement`]) — a
/// restore into a fresh database otherwise fails outright the first time a table sits outside `dbo`.
pub async fn dump_structure(
    pool: &Pool,
    database: &str,
    path: &str,
    watch: &dump::Watch<'_>,
) -> Result<(), AppError> {
    use std::io::Write;

    let tables = mssql_structure::table_stats(pool, database).await?;
    let weights: Vec<(String, u64)> = tables.iter().map(|t| (t.name.clone(), 1)).collect();
    let mut tracker = Tracker::new(&weights, path, false);

    let mut file = std::fs::File::create(path)
        .map_err(|e| err!("error.cannotWriteFile", path = path, message = e))?;
    write!(file, "-- MixLab structure dump\n\n")
        .map_err(|e| err!("error.cannotWriteFile", path = path, message = e))?;

    let mut schemas: Vec<String> = tables.iter().map(|t| resolve(&t.name).0).collect();
    schemas.sort();
    schemas.dedup();
    for schema in &schemas {
        if schema == DEFAULT_SCHEMA {
            continue;
        }
        writeln!(file, "{};", create_schema_statement(schema))
            .map_err(|e| err!("error.cannotWriteFile", path = path, message = e))?;
    }
    if schemas.iter().any(|s| s != DEFAULT_SCHEMA) {
        writeln!(file).map_err(|e| err!("error.cannotWriteFile", path = path, message = e))?;
    }

    for table in &tables {
        if (watch.cancel)() {
            return Err(err!("error.transferCancelled", tool = "SQL Server dump"));
        }
        let (schema, name) = resolve(&table.name);
        let qualified = qualified_ident(&schema, &name);
        let structure = mssql_structure::table_structure(pool, database, &table.name).await?;
        let computed = computed_definitions(pool, database, &schema, &name).await?;

        let mut column_lines = Vec::with_capacity(structure.columns.len());
        for column in &structure.columns {
            if column.generated {
                let Some(definition) = computed.get(&column.name) else {
                    // A computed column the catalogue no longer explains — dropped between the two
                    // reads, or a flavour `sys.computed_columns` does not cover. Skipped with the
                    // rest of the table intact rather than failing the whole dump over one column.
                    continue;
                };
                column_lines.push(computed_column_definition(&column.name, definition));
            } else {
                column_lines.push(column_definition(&column_spec_from(column))?);
            }
        }
        write!(
            file,
            "CREATE TABLE {qualified} (\n  {}\n);\n",
            column_lines.join(",\n  ")
        )
        .map_err(|e| err!("error.cannotWriteFile", path = path, message = e))?;

        for column in &structure.columns {
            if let Some(stmt) =
                comment_statement(&schema, &name, &column.name, &column.comment, false)
            {
                writeln!(file, "{stmt};")
                    .map_err(|e| err!("error.cannotWriteFile", path = path, message = e))?;
            }
        }
        for index in &structure.indexes {
            writeln!(file, "{};", dump_index_statement(&schema, &name, index))
                .map_err(|e| err!("error.cannotWriteFile", path = path, message = e))?;
        }
        writeln!(file).map_err(|e| err!("error.cannotWriteFile", path = path, message = e))?;

        tracker.reached(&table.name);
        (watch.report)(tracker.progress());
    }

    write_views(pool, database, &mut file, path, watch).await?;
    write_foreign_keys(pool, database, &mut file, path).await
}

/// Every view's original `CREATE VIEW` text, oldest first — a view built on top of another one the
/// same session created earlier is the common case for nesting, and `create_date` approximates that
/// dependency order the same way `sqlite_dump::dump_structure` relies on `sqlite_master`'s `rowid`
/// (creation order) rather than trying to parse real dependencies out of the view's own SQL text.
async fn view_definitions(pool: &Pool, database: &str) -> Result<Vec<String>, AppError> {
    let db = quote_ident(database);
    let sql = read_uncommitted(&format!(
        "SELECT m.definition
         FROM {db}.sys.views v
         JOIN {db}.sys.sql_modules m ON m.object_id = v.object_id
         ORDER BY v.create_date"
    ));
    let mut client = pool
        .get()
        .await
        .map_err(|e| err!("error.mssql", message = e))?;
    let rows = client
        .query(sql, &[])
        .await
        .map_err(map_error)?
        .into_first_result()
        .await
        .map_err(map_error)?;

    Ok(rows
        .iter()
        .filter_map(|row| row.get::<&str, _>("definition"))
        .map(str::to_string)
        .collect())
}

/// Writes every view's `CREATE VIEW` text verbatim, after every table so a view selecting from one
/// has something to select from. Not part of [`mssql_structure::table_stats`]'s table list — that
/// list is exactly what the Statistics tab shows, which leaves views out on every engine here
/// because a view stores nothing of its own to weigh — but a dump's job is different: a database
/// dumped and restored is missing part of its schema if a view silently drops out, which is what
/// happened before this was added (see this plan's Task 9).
///
/// Copied as-is rather than regenerated, the same choice `sqlite_dump.rs` makes for every `CREATE`
/// it writes: `sys.sql_modules.definition` is the view's own original text, so whatever schema
/// qualification (or lack of it) the author wrote is what comes back out. No `data_size`/`rows`
/// weight applies to a view (it holds none), so this does not update `dump_structure`'s `Tracker`.
async fn write_views(
    pool: &Pool,
    database: &str,
    file: &mut std::fs::File,
    path: &str,
    watch: &dump::Watch<'_>,
) -> Result<(), AppError> {
    use std::io::Write;

    for definition in view_definitions(pool, database).await? {
        if (watch.cancel)() {
            return Err(err!("error.transferCancelled", tool = "SQL Server dump"));
        }
        write!(file, "{};\n\n", definition.trim())
            .map_err(|e| err!("error.cannotWriteFile", path = path, message = e))?;
    }
    Ok(())
}

/// One foreign key constraint in full — every column of a composite key, and the referential
/// action either side declares — read once for the whole database rather than once per table:
/// `mssql::foreign_keys` (used to decorate the Data tab's grid) answers "what does this column
/// point at", one row per column, which loses which columns of a composite key belong together and
/// drops the constraint's name and its `ON DELETE`/`ON UPDATE` entirely.
struct ForeignKeyConstraint {
    name: String,
    schema: String,
    table: String,
    columns: Vec<String>,
    ref_schema: String,
    ref_table: String,
    ref_columns: Vec<String>,
    on_delete: String,
    on_update: String,
}

async fn foreign_key_constraints(
    pool: &Pool,
    database: &str,
) -> Result<Vec<ForeignKeyConstraint>, AppError> {
    let db = quote_ident(database);
    let sql = read_uncommitted(&format!(
        "SELECT fk.name AS constraint_name, ps.name AS schema_name, po.name AS table_name,
                rs.name AS ref_schema_name, ro.name AS ref_table_name,
                pc.name AS column_name, rc.name AS ref_column_name,
                fk.delete_referential_action_desc, fk.update_referential_action_desc
         FROM {db}.sys.foreign_keys fk
         JOIN {db}.sys.foreign_key_columns fkc ON fkc.constraint_object_id = fk.object_id
         JOIN {db}.sys.objects po ON po.object_id = fk.parent_object_id
         JOIN {db}.sys.schemas ps ON ps.schema_id = po.schema_id
         JOIN {db}.sys.columns pc
             ON pc.object_id = fkc.parent_object_id AND pc.column_id = fkc.parent_column_id
         JOIN {db}.sys.objects ro ON ro.object_id = fk.referenced_object_id
         JOIN {db}.sys.schemas rs ON rs.schema_id = ro.schema_id
         JOIN {db}.sys.columns rc
             ON rc.object_id = fkc.referenced_object_id AND rc.column_id = fkc.referenced_column_id
         ORDER BY fk.name, fkc.constraint_column_id"
    ));
    let mut client = pool
        .get()
        .await
        .map_err(|e| err!("error.mssql", message = e))?;
    let rows = client
        .query(sql, &[])
        .await
        .map_err(map_error)?
        .into_first_result()
        .await
        .map_err(map_error)?;

    let mut constraints: Vec<ForeignKeyConstraint> = Vec::new();
    for row in &rows {
        let Some(name) = row.get::<&str, _>("constraint_name") else {
            continue;
        };
        if constraints
            .last()
            .map(|last| last.name != name)
            .unwrap_or(true)
        {
            constraints.push(ForeignKeyConstraint {
                name: name.to_string(),
                schema: row.get::<&str, _>("schema_name").unwrap_or("").to_string(),
                table: row.get::<&str, _>("table_name").unwrap_or("").to_string(),
                ref_schema: row
                    .get::<&str, _>("ref_schema_name")
                    .unwrap_or("")
                    .to_string(),
                ref_table: row
                    .get::<&str, _>("ref_table_name")
                    .unwrap_or("")
                    .to_string(),
                columns: Vec::new(),
                ref_columns: Vec::new(),
                on_delete: row
                    .get::<&str, _>("delete_referential_action_desc")
                    .unwrap_or("NO_ACTION")
                    .to_string(),
                on_update: row
                    .get::<&str, _>("update_referential_action_desc")
                    .unwrap_or("NO_ACTION")
                    .to_string(),
            });
        }
        if let Some(fk) = constraints.last_mut() {
            if let Some(col) = row.get::<&str, _>("column_name") {
                fk.columns.push(col.to_string());
            }
            if let Some(col) = row.get::<&str, _>("ref_column_name") {
                fk.ref_columns.push(col.to_string());
            }
        }
    }
    Ok(constraints)
}

/// `sys.foreign_keys`' own spelling (`NO_ACTION`, `CASCADE`, `SET_NULL`, `SET_DEFAULT`) turned into
/// what `ON DELETE`/`ON UPDATE` actually takes — an underscore standing in for the one space
/// `NO ACTION` needs, and a name this file does not recognise falling back to `NO ACTION` rather
/// than failing the dump over a value only a SQL Server version newer than this code knows about.
fn referential_action(desc: &str) -> &'static str {
    match desc {
        "CASCADE" => "CASCADE",
        "SET_NULL" => "SET NULL",
        "SET_DEFAULT" => "SET DEFAULT",
        _ => "NO ACTION",
    }
}

fn foreign_key_statement(fk: &ForeignKeyConstraint) -> String {
    let qualified = qualified_ident(&fk.schema, &fk.table);
    let ref_qualified = qualified_ident(&fk.ref_schema, &fk.ref_table);
    let columns = fk
        .columns
        .iter()
        .map(|c| quote_ident(c))
        .collect::<Vec<_>>()
        .join(", ");
    let ref_columns = fk
        .ref_columns
        .iter()
        .map(|c| quote_ident(c))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "ALTER TABLE {qualified} ADD CONSTRAINT {} FOREIGN KEY ({columns}) \
         REFERENCES {ref_qualified} ({ref_columns}) ON DELETE {} ON UPDATE {}",
        quote_ident(&fk.name),
        referential_action(&fk.on_delete),
        referential_action(&fk.on_update)
    )
}

/// Every foreign key of the database, added after every table exists — no topological sort of
/// tables by dependency (a self-referencing table or a two-table cycle would deadlock one), the
/// same trade dump tools that defer constraints to the end rather than order `CREATE TABLE` by
/// dependency already make.
async fn write_foreign_keys(
    pool: &Pool,
    database: &str,
    file: &mut std::fs::File,
    path: &str,
) -> Result<(), AppError> {
    use std::io::Write;

    let constraints = foreign_key_constraints(pool, database).await?;
    if constraints.is_empty() {
        return Ok(());
    }
    write!(file, "-- Foreign keys\n\n")
        .map_err(|e| err!("error.cannotWriteFile", path = path, message = e))?;
    for fk in &constraints {
        writeln!(file, "{};", foreign_key_statement(fk))
            .map_err(|e| err!("error.cannotWriteFile", path = path, message = e))?;
    }
    Ok(())
}

/// Turns one value `mssql::column_value` already decoded to JSON into the literal `dump_data`
/// writes into an `INSERT` — reusing that decode rather than matching on `tiberius::ColumnData` a
/// second time, because `mssql::select_expr` (used for the `SELECT` list below, same as
/// `mssql::table_data`) has already cast the awkward types — `money`, `xml`, `sql_variant`,
/// `geography`/`geometry`, `hierarchyid` — to text on the server side, so there is nothing left
/// here that needs the raw driver type.
///
/// `data_type` decides the quoting: a `mssql::is_binary_type` column comes back as base64 and is
/// decoded and re-written as a `0x...` hex literal (SQL Server's own binary literal syntax, not
/// base64 — base64 was only ever this app's wire format to the grid); a Unicode text type is
/// prefixed `N'...'` so non-ASCII data restores intact even into a non-Unicode-default collation;
/// everything else that is JSON `String` is a plain `'...'` literal, and a lone quote inside it is
/// doubled the way `quote_string` doubles one everywhere else in this driver.
fn sql_literal(value: &serde_json::Value, data_type: &str) -> String {
    use serde_json::Value;
    match value {
        Value::Null => "NULL".to_string(),
        Value::Bool(b) => if *b { "1" } else { "0" }.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => {
            if is_binary_type(data_type) {
                use base64::Engine;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(s)
                    .unwrap_or_default();
                let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                format!("0x{hex}")
            } else if is_unicode_text_type(data_type) {
                format!("N{}", quote_string(s))
            } else {
                quote_string(s)
            }
        }
        // An array or object never reaches here — no `ColumnData` arm produces one (D11).
        _ => "NULL".to_string(),
    }
}

/// Whether a column's text needs the `N` prefix to survive a restore intact. Mirrors the base-type
/// check `mssql::is_binary_type` already does (strip a `(...)` length before comparing) rather than
/// matching the full declared type, so `nvarchar(255)` and `nvarchar(max)` are both caught.
fn is_unicode_text_type(data_type: &str) -> bool {
    let base = data_type
        .split('(')
        .next()
        .unwrap_or(data_type)
        .trim()
        .to_ascii_lowercase();
    matches!(base.as_str(), "nchar" | "nvarchar" | "ntext" | "xml")
}

/// The `ORDER BY` a dump's paged `SELECT` needs (`OFFSET`/`FETCH` requires one). Unlike
/// `mssql::table_data`'s `(SELECT NULL)` fallback for "no order the user asked for", two pages of
/// the *same* dump have to agree with each other — SQL Server does not promise the same physical
/// order across two separate `OFFSET`/`FETCH` calls without one. The primary key sorts
/// unambiguously when the table has one; every selected column is the fallback for a heap table
/// without one, the same "no key at all, so use the whole row" trade `update_row`'s no-primary-key
/// path already accepts.
fn dump_order_by(primary_key: &[String], columns: &[String]) -> String {
    let keys: &[String] = if primary_key.is_empty() {
        columns
    } else {
        primary_key
    };
    keys.iter()
        .map(|c| quote_ident(c))
        .collect::<Vec<_>>()
        .join(", ")
}

/// How many rows one `INSERT ... VALUES` statement carries: SQL Server's own grammar limit on a
/// multi-row `VALUES` list, independent of column count since nothing here binds a parameter —
/// every value is a literal.
const INSERT_BATCH_ROWS: usize = 1000;
/// How many rows one `SELECT ... OFFSET/FETCH` page reads at a time while dumping — independent of
/// [`INSERT_BATCH_ROWS`], and larger, since a read page is chunked again into `INSERT` batches
/// after it arrives.
const READ_PAGE_ROWS: i64 = 5000;

/// Streams every base table's rows into `path` as batched `INSERT` statements, wrapping a table
/// that has an `IDENTITY` column in `SET IDENTITY_INSERT ... ON`/`OFF` so the original values
/// restore rather than being renumbered (D10's own stated constraint) — one table at a time, since
/// SQL Server allows only one table's `IDENTITY_INSERT` on per session.
///
/// `append`: `true` continues an `all`-mode dump onto the structure [`dump_structure`] already
/// wrote; `false` owns the file from scratch (a `data`-only dump).
pub async fn dump_data(
    pool: &Pool,
    database: &str,
    path: &str,
    append: bool,
    watch: &dump::Watch<'_>,
) -> Result<(), AppError> {
    use tokio::io::AsyncWriteExt;

    let tables = mssql_structure::table_stats(pool, database).await?;
    let weights: Vec<(String, u64)> = tables
        .iter()
        .map(|t| (t.name.clone(), t.rows.max(1)))
        .collect();
    let mut tracker = Tracker::new(&weights, path, true);

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(path)
        .await
        .map_err(|e| err!("error.cannotWriteFile", path = path, message = e))?;

    for table in &tables {
        if (watch.cancel)() {
            return Err(err!("error.transferCancelled", tool = "SQL Server dump"));
        }
        tracker.reached(&table.name);

        let (schema, name) = resolve(&table.name);
        // Two names, deliberately not one: `qualified` (no database) is what gets *written into
        // the file* — `mssql_script::run` (Task 5) puts the target database in scope with its own
        // `USE` before replaying it, and a hardcoded database there would fight that `USE` the way
        // the earlier database-qualification bug did (see this plan's Task 8). `live_qualified`
        // (three-part) is what *this function itself* queries against the pool right now, on a
        // connection that is not guaranteed to be sitting on `database` at all — the pool is one
        // per server (D2), shared by every command, so a connection `pool.get()` hands back may
        // still default to whatever database it was opened with. Using `qualified` for the live
        // `SELECT` below reads (or errors on) whatever database the connection happens to be on
        // instead of the one being dumped — confirmed against a real multi-database server, where
        // it failed with "Invalid object name" the single-database live test in Task 7 could not
        // have caught (its pool happened to open on the exact database being dumped).
        let qualified = qualified_ident(&schema, &name);
        let live_qualified = three_part(database, &schema, &name);
        // Computed and rowversion/timestamp columns are never inserted — the server derives or
        // assigns both — the same set `mssql::table_data`'s insert path already excludes.
        let columns: Vec<_> = table_columns(pool, database, &schema, &name)
            .await?
            .into_iter()
            .filter(|c| !c.is_computed && !c.is_rowversion)
            .collect();
        if columns.is_empty() {
            (watch.report)(tracker.progress());
            continue;
        }
        let has_identity = columns.iter().any(|c| c.is_identity);
        let column_names: Vec<String> = columns.iter().map(|c| c.name.clone()).collect();
        let column_list = column_names
            .iter()
            .map(|c| quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ");
        let select_list = columns
            .iter()
            .map(|c| select_expr(&quote_ident(&c.name), &c.data_type))
            .collect::<Vec<_>>()
            .join(", ");
        let keys = primary_key(pool, database, &schema, &name).await?;
        let order_by = dump_order_by(&keys, &column_names);

        if has_identity {
            let sql = format!("SET IDENTITY_INSERT {qualified} ON;\n");
            file.write_all(sql.as_bytes())
                .await
                .map_err(|e| err!("error.cannotWriteFile", path = path, message = e))?;
        }

        let mut offset: i64 = 0;
        loop {
            if (watch.cancel)() {
                return Err(err!("error.transferCancelled", tool = "SQL Server dump"));
            }
            let sql = read_uncommitted(&format!(
                "SELECT {select_list} FROM {live_qualified} ORDER BY {order_by} \
                 OFFSET {offset} ROWS FETCH NEXT {READ_PAGE_ROWS} ROWS ONLY"
            ));
            let mut client = pool
                .get()
                .await
                .map_err(|e| err!("error.mssql", message = e))?;
            let rows = client
                .query(sql, &[])
                .await
                .map_err(map_error)?
                .into_first_result()
                .await
                .map_err(map_error)?;
            let read = rows.len();
            drop(client);

            for chunk in rows.chunks(INSERT_BATCH_ROWS) {
                let mut values_list = Vec::with_capacity(chunk.len());
                for row in chunk {
                    let literals: Vec<String> = row
                        .cells()
                        .enumerate()
                        .map(|(i, (_, data))| {
                            sql_literal(&column_value(data), &columns[i].data_type)
                        })
                        .collect();
                    values_list.push(format!("({})", literals.join(", ")));
                }
                let statement = format!(
                    "INSERT INTO {qualified} ({column_list}) VALUES\n{};\n",
                    values_list.join(",\n")
                );
                file.write_all(statement.as_bytes())
                    .await
                    .map_err(|e| err!("error.cannotWriteFile", path = path, message = e))?;
            }
            (watch.report)(tracker.progress());

            if (read as i64) < READ_PAGE_ROWS {
                break;
            }
            offset += READ_PAGE_ROWS;
        }

        if has_identity {
            let sql = format!("SET IDENTITY_INSERT {qualified} OFF;\n\n");
            file.write_all(sql.as_bytes())
                .await
                .map_err(|e| err!("error.cannotWriteFile", path = path, message = e))?;
        } else {
            file.write_all(b"\n")
                .await
                .map_err(|e| err!("error.cannotWriteFile", path = path, message = e))?;
        }
    }
    // `write_all` on a tokio file returns once the bytes are handed to a blocking thread, so the
    // last write is still in flight here — and its error unseen — until it is flushed.
    file.flush()
        .await
        .map_err(|e| err!("error.cannotWriteFile", path = path, message = e))
}

/// Replays a dump file into `database`, one call into `mssql_script::run` (Plan 5) — the same
/// `GO`-aware batch splitter the Query tab drives, so a file this app wrote (`GO` never appears in
/// it, since `dump_structure`/`dump_data` separate statements with `;` alone) and a file written by
/// hand with `GO` batches both replay correctly.
///
/// No `Watch`, no mid-run cancel and no incremental progress — the same limit `sqlite_dump::restore`
/// already accepts, for the same reason: `run` takes the whole script as one call and returns only
/// once every statement in it has been tried, so there is no point inside it this file could check
/// a cancel flag from outside.
pub async fn restore(pool: &Pool, database: &str, path: &str) -> Result<(), AppError> {
    let sql = std::fs::read_to_string(path)
        .map_err(|e| err!("error.cannotReadFile", path = path, message = e))?;

    let results = mssql_script::run(pool, &sql, Some(database), |_| {}).await?;
    if let Some(failed) = results.iter().find(|result| result.error.is_some()) {
        return Err(err!(
            "error.mssqlRestoreFailed",
            statement = failed.statement.chars().take(200).collect::<String>(),
            message = failed.error.clone().unwrap_or_default()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, data_type: &str) -> StructureColumn {
        StructureColumn {
            name: name.to_string(),
            data_type: data_type.to_string(),
            nullable: true,
            default_value: None,
            default_is_expression: false,
            auto_increment: false,
            on_update_current_timestamp: false,
            generated: false,
            collation: None,
            comment: String::new(),
            key: String::new(),
            extra: String::new(),
        }
    }

    #[test]
    fn column_spec_from_carries_every_field_through() {
        let mut col = column("price", "decimal(10,2)");
        col.default_value = Some("0".to_string());
        col.collation = Some("Latin1_General_CI_AS".to_string());
        col.comment = "unit price".to_string();

        let spec = column_spec_from(&col);
        assert_eq!(spec.name, "price");
        assert_eq!(spec.data_type, "decimal(10,2)");
        assert_eq!(spec.default_value.as_deref(), Some("0"));
        assert_eq!(spec.collation.as_deref(), Some("Latin1_General_CI_AS"));
        assert_eq!(spec.comment, "unit price");
    }

    #[test]
    fn a_persisted_computed_column_carries_the_keyword() {
        let def = ComputedDefinition {
            definition: "[a] + [b]".to_string(),
            persisted: true,
        };
        assert_eq!(
            computed_column_definition("total", &def),
            "[total] AS ([a] + [b]) PERSISTED"
        );
    }

    #[test]
    fn a_non_persisted_computed_column_does_not() {
        let def = ComputedDefinition {
            definition: "[a] + [b]".to_string(),
            persisted: false,
        };
        assert_eq!(
            computed_column_definition("total", &def),
            "[total] AS ([a] + [b])"
        );
    }

    #[test]
    fn referential_action_translates_every_known_value() {
        assert_eq!(referential_action("CASCADE"), "CASCADE");
        assert_eq!(referential_action("SET_NULL"), "SET NULL");
        assert_eq!(referential_action("SET_DEFAULT"), "SET DEFAULT");
        assert_eq!(referential_action("NO_ACTION"), "NO ACTION");
    }

    #[test]
    fn an_unrecognised_action_falls_back_to_no_action() {
        assert_eq!(referential_action("SOMETHING_FUTURE"), "NO ACTION");
    }

    #[test]
    fn a_composite_foreign_key_lists_every_column_in_order() {
        let fk = ForeignKeyConstraint {
            name: "FK_order_item_order".to_string(),
            schema: "dbo".to_string(),
            table: "order_item".to_string(),
            columns: vec!["order_id".to_string(), "order_line".to_string()],
            ref_schema: "dbo".to_string(),
            ref_table: "order".to_string(),
            ref_columns: vec!["id".to_string(), "line".to_string()],
            on_delete: "CASCADE".to_string(),
            on_update: "NO_ACTION".to_string(),
        };
        assert_eq!(
            foreign_key_statement(&fk),
            "ALTER TABLE [dbo].[order_item] \
             ADD CONSTRAINT [FK_order_item_order] FOREIGN KEY ([order_id], [order_line]) \
             REFERENCES [dbo].[order] ([id], [line]) \
             ON DELETE CASCADE ON UPDATE NO ACTION"
        );
    }

    #[test]
    fn sql_literal_covers_null_bool_and_number() {
        assert_eq!(sql_literal(&serde_json::Value::Null, "int"), "NULL");
        assert_eq!(sql_literal(&serde_json::json!(true), "bit"), "1");
        assert_eq!(sql_literal(&serde_json::json!(false), "bit"), "0");
        assert_eq!(sql_literal(&serde_json::json!(42), "int"), "42");
    }

    #[test]
    fn sql_literal_prefixes_unicode_text_with_n() {
        assert_eq!(
            sql_literal(&serde_json::json!("mới"), "nvarchar(50)"),
            "N'mới'"
        );
    }

    #[test]
    fn sql_literal_escapes_a_lone_quote_in_plain_text() {
        assert_eq!(
            sql_literal(&serde_json::json!("it's"), "varchar(50)"),
            "'it''s'"
        );
    }

    #[test]
    fn sql_literal_writes_binary_as_a_hex_literal() {
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode([0x00, 0xff, 0x10]);
        assert_eq!(
            sql_literal(&serde_json::json!(encoded), "varbinary(50)"),
            "0x00ff10"
        );
    }

    #[test]
    fn dump_order_by_prefers_the_primary_key() {
        let pk = vec!["id".to_string()];
        let cols = vec!["id".to_string(), "name".to_string()];
        assert_eq!(dump_order_by(&pk, &cols), "[id]");
    }

    #[test]
    fn dump_order_by_falls_back_to_every_column_without_one() {
        let cols = vec!["a".to_string(), "b".to_string()];
        assert_eq!(dump_order_by(&[], &cols), "[a], [b]");
    }

    fn index(
        name: &str,
        primary: bool,
        unique: bool,
        index_type: &str,
        columns: &[&str],
    ) -> TableIndex {
        TableIndex {
            name: name.to_string(),
            primary,
            unique,
            index_type: index_type.to_string(),
            columns: columns
                .iter()
                .map(|c| super::super::mssql_structure::IndexColumn {
                    name: Some(c.to_string()),
                    prefix_length: None,
                })
                .collect(),
            comment: String::new(),
        }
    }

    #[test]
    fn dump_index_statement_never_names_a_database() {
        let idx = index("PK__customer__1", true, false, "clustered", &["id"]);
        assert_eq!(
            dump_index_statement("dbo", "customers", &idx),
            "ALTER TABLE [dbo].[customers] ADD CONSTRAINT [PK__customer__1] \
             PRIMARY KEY CLUSTERED ([id])"
        );
    }

    #[test]
    fn dump_index_statement_covers_a_plain_nonclustered_index() {
        let idx = index("ix_customers_code", false, false, "nonclustered", &["code"]);
        assert_eq!(
            dump_index_statement("dbo", "customers", &idx),
            "CREATE NONCLUSTERED INDEX [ix_customers_code] ON [dbo].[customers] ([code])"
        );
    }

    #[test]
    fn create_schema_statement_is_guarded_and_uses_dynamic_sql() {
        assert_eq!(
            create_schema_statement("sales"),
            "IF NOT EXISTS (SELECT 1 FROM sys.schemas WHERE name = N'sales') \
             EXEC('CREATE SCHEMA [sales]')"
        );
    }

    #[test]
    fn dump_index_statement_marks_a_unique_index() {
        let idx = index("ux_users_email", false, true, "nonclustered", &["email"]);
        assert_eq!(
            dump_index_statement("dbo", "users", &idx),
            "CREATE UNIQUE NONCLUSTERED INDEX [ux_users_email] ON [dbo].[users] ([email])"
        );
    }
}
