---
status: approved
date: 2026-10-15
task: T208f
---

# One command set for the SQL engines (design)

MixLab's `db` module reaches five SQL engines: MySQL, PostgreSQL, SQL Server, ClickHouse and
SQLite. The frontend already sees them as one: the shared workspace works through one `SqlApi` and
one `SqlDialect` (`src/modules/db/sql/`), and `engines.ts` maps a kind to its pair. The Rust does
not. Each engine has its own set of Tauri commands, 135 in all. Measured on `master` at
`11c30bfa`:

| Engine | Commands |
| --- | ---: |
| MySQL (`mysql_*`) | 28 |
| PostgreSQL (`postgres_*`) | 28 |
| SQL Server (`mssql_*`) | 27 |
| ClickHouse (`clickhouse_*`) | 27 |
| SQLite (`sqlite_*`) | 25 |

The same 135 names are listed again in `modules/mod.rs` and in five `api.ts` files of about 165
lines each, every method one `invoke`. Adding one operation means five commands, five
registrations and five `api.ts` methods. Adding an engine means about 28 of each.

Roadmap: [phase 46](../roadmap/phase-46-a-database-module-of-one-shape.md), task T208f. Changing
the command surface is a cross-cutting decision, recorded in
[ADR 0062](../decisions/0062-the-sql-engines-share-one-command-set.md).

## What the survey found

**34 distinct operations.** Twenty are on all five engines: `list_databases`, `list_tables`,
`server_info`, `schema_outline`, `table_stats`, `table_data`, `table_structure`, `update_row`,
`insert_rows`, `delete_rows`, `create_table`, `rename_table`, `drop_table`, `add_column`,
`modify_column`, `drop_column`, `run_script`, `validate_sql`, `dump`, `restore`. The rest are on
some engines only:

| Operation | Missing on |
| --- | --- |
| `create_database`, `drop_database` | SQLite (a file is the database) |
| `collations` | ClickHouse |
| `add_index`, `modify_index`, `drop_index` | ClickHouse (it has skip indexes instead) |
| `cancel_query` | SQLite (no session to reach), ClickHouse (none in v1) |
| `query` | SQL Server, ClickHouse, SQLite |
| `add_skip_index`, `modify_skip_index`, `drop_skip_index`, `rebuild_order_by`, `row_count` | all but ClickHouse |
| `create_file` | all but SQLite; it takes a path, not a connection |

**The command signatures already agree.** Every engine's `list_tables` is
`(state, id, database)`, every `update_row` is `(state, id, database, table, updates, key)`, every
`run_script` is `(state, id, run_id, sql, database)`. The differences are in the types: each engine
has its own `PageQuery`, `TablePage`, `TableStructure` and `ColumnSpec` in Rust, while the
frontend sends and reads one TypeScript type for each (`SqlPageQuery`, `SqlTablePage`,
`SqlColumnSpec`, … in `types.ts`). Each engine's Rust `ColumnSpec` reads a subset of
`SqlColumnSpec` (ClickHouse reads five fields, MySQL ten). `create_table` is the one real
signature difference: ClickHouse takes an `engine` where the others take a `collation`. The
frontend's `SqlApi.createTable` already carries both.

**The frontend already handles a missing operation.** An engine's `api.ts` fills what it lacks
with `notSupported()`, or with `Promise.resolve()` for a `cancelQuery` whose button the dialect
never shows. The gate is the dialect (`cancellable`, the skip-index panels), not the api.

**The commands find their connection the same way.** `handle(state, id)` clones the `DbHandle`
out of the map, and `mysql_pool`, `postgres_pool`, … match one variant and return
`error.wrongConnectionKind` for any other. `retry_read!` wraps the reads of MySQL, PostgreSQL and
SQL Server (8, 8 and 9 uses) and one of SQLite's. ClickHouse uses none.

## D1 — one trait, `SqlEngine`, behind the `DbHandle`

A new module `drivers/sql.rs` holds the trait and its wire types (D2). It is written with
`async-trait`, already a dependency, so it can be used as `dyn SqlEngine`:

```rust
#[async_trait]
pub trait SqlEngine: Send + Sync {
    async fn list_databases(&self) -> Result<Vec<String>, AppError>;
    async fn list_tables(&self, database: &str) -> Result<Vec<String>, AppError>;
    async fn table_data(&self, database: &str, table: &str, query: PageQuery)
        -> Result<TablePage, AppError>;
    // … one method per operation of the table above

    async fn add_skip_index(&self, _database: &str, _table: &str, _spec: SkipIndexSpec)
        -> Result<(), AppError> {
        Err(unsupported("add_skip_index"))
    }
}
```

- **Each engine implements it once**, in `drivers/<engine>/engine.rs`, on a small type built from
  its `DbHandle` variant (`Mysql { pool, mariadb }`, `Postgres(Arc<Pools>)`, …). The methods
  delegate to the functions that exist today; nothing in `drivers/<engine>/*.rs` is rewritten.
- **`DbHandle::sql(self) -> Result<Box<dyn SqlEngine>, AppError>`** is the one dispatch. Mongo and
  Redis answer `error.notSqlConnection`. The match on the variant lives there and nowhere else.
- **An operation an engine lacks** is a default method returning a typed error,
  `error.unsupportedOperation`, never a panic and never a silent `Ok`. Today's `Promise.resolve()`
  for `cancelQuery` stays in the frontend, where the dialect already hides the button.
- **Retrying a read stays where it is.** `retry_read!` moves into the engine implementations that
  use it today, unchanged. Making ClickHouse or SQLite retry is a behaviour change, out of scope.

## D2 — one set of wire types

The trait's signatures use one Rust type per TypeScript type the frontend already sends or reads:
`PageQuery`, `TablePage`, `TableStructure`, `TableStats`, `ColumnSpec`, `IndexSpec`,
`SkipIndexSpec`, `SchemaOutline`, `Collation`, `ServerInfo`, `StatementResult`, `SqlProblem`.

- Where the five engines' structs already have the same fields, they become this one type, moved
  to `drivers/sql.rs` (or `drivers/sql/types.rs` if that file would pass 800 lines).
- Where an engine's struct is a subset (each `ColumnSpec`), the shared type is the superset the
  frontend sends, and the engine converts with a `From` into its own type. Its own type stays, so
  its DDL code does not change.
- `create_table` takes `collation: Option<String>` and `engine: Option<String>`. The frontend
  always sends `engine` for ClickHouse, and a missing one is refused with a typed error rather
  than given a default. The other engines ignore it.

The wire format does not change: what the frontend sends and receives is byte for byte what it is
today. The plan measures, struct by struct, which of the two cases applies.

## D3 — one command per operation, named `sql_*`

`commands/sql.rs` holds 32 commands, one per operation of the survey but `create_file` and
`query`:

```rust
#[tauri::command]
pub async fn sql_list_tables(state: State<'_, DbState>, id: String, database: String)
    -> Result<Vec<String>, AppError> {
    handle(&state, &id).await?.sql()?.list_tables(&database).await
}
```

- **The names are the operation with `sql_` in front**: `sql_list_tables`, `sql_run_script`,
  `sql_add_skip_index`. Arguments keep their names, so every `invoke` payload stays as it is.
- **`sqlite_create_file` keeps its name.** It takes a path and no connection, so it has no engine
  to dispatch to.
- **`dump` and `restore` stay out of the trait.** They are orchestration, not a driver call: they
  need the `AppHandle` for progress, the transfer registry, the tools directory and the connection's
  endpoint, none of which belongs in `drivers/`. `sql_dump` and `sql_restore` match the
  `DbHandle` once and call the engine's own orchestration, which stays in
  `commands/<engine>.rs` as a private function: today's command body, unchanged.
- **`cancel_query`** keeps its `RunningQuery` registry in `commands/mod.rs`; the command looks the
  run up and the engine is asked to cancel it. PostgreSQL's command took a `database` that the
  frontend never sent, so it always used the default pool; `sql_cancel_query` takes no `database`,
  and PostgreSQL's engine cancels on the default pool as it does today.
- **`mysql_query` and `postgres_query` are deleted, not renamed.** Their only callers were
  `mysqlQuery` and `postgresQuery` in the two `api.ts` files, and nothing calls those.
- `modules/mod.rs` registers the 32 in place of 135. What stays under `commands/` for the five
  engines is their dump and restore orchestration, and `sqlite_create_file`.
- Mongo's 16 and Redis's 7 commands do not change.

## D4 — the frontend: one `sqlApi`

- `src/modules/db/sql/api.ts` gains `sqlApi: SqlApi`, each method one `invoke("sql_…")`, carrying
  the doc comments the interface already has.
- The five `mysql/api.ts`, `postgres/api.ts`, `mssql/api.ts`, `clickhouse/api.ts` and
  `sqlite/api.ts` are deleted. `sqlite_create_file`'s wrapper, the one thing in them that is not
  an `SqlApi` method, moves to `sql/api.ts`. The eslint rule that only an `api.ts` imports
  `invoke` holds.
- `demo/fixtures/db.ts` answers seven `postgres_*` commands for the screenshots. They become the
  matching `sql_*` names.
- `SQL_ENGINES` in `engines.ts` keeps one entry per kind, each `{ api: sqlApi, dialect }`. The
  workspace and everything below it do not change.
- The `notSupported()` stubs go: the backend answers `error.unsupportedOperation` for them, and
  the dialect still keeps their buttons hidden.

## D5 — errors and strings

Two keys are added to the backend's messages and to `en.ts` and `vi.ts`, worded under
`writing-user-facing-text`: `error.notSqlConnection` and `error.unsupportedOperation` (naming the
operation). `error.wrongConnectionKind` stays for Mongo's and Redis's own helpers.

## D6 — documentation

- `docs/standards/desktop/adding-a-command.md` is rewritten: a SQL operation is one trait method,
  one implementation per engine, one `sql_*` command, one registration and one `sqlApi` method.
  The five-place rule stays for Mongo, Redis and the rest.
- `docs/architecture/desktop/backend.md`: its `mysql_list_tables` example becomes
  `sql_list_tables`, and the drivers table gains `drivers/sql.rs` and `drivers/<engine>/engine.rs`.
- `docs/standards/desktop/adding-a-module.md`, where it says the `db` module has one `api.ts` per
  engine.
- `apps/desktop/CLAUDE.md`'s layout: `mysql/ postgres/` no longer hold an `api.ts`.
- ADR 0062.

## D7 — order of work and checks

On a branch `t208f-one-command-set-for-sql-engines`. Each commit compiles and passes its tests,
because the old and new commands can live side by side until the last step:

1. The trait, the wire types, `DbHandle::sql`, and the five implementations (D1, D2). Rust only;
   nothing calls them yet. Unit tests for `DbHandle::sql` and for the `From` conversions.
2. The 32 `sql_*` commands and their registration, beside the old ones (D3).
3. `sqlApi`, `SQL_ENGINES` pointed at it, the engine `api.ts` files deleted (D4, D5).
4. The 135 old commands and their registrations deleted.
5. Documentation (D6), the roadmap tick and this spec's status.

Checks: `cargo clippy --all-targets -D warnings`, `cargo test`, `npm run build`, `npm test`,
`npm run lint`, `npm run screenshots -- --check`, `bash scripts/gate.sh`. A grep finds no
`invoke("mysql_`, `postgres_`, `mssql_`, `clickhouse_` or `sqlite_` but `sqlite_create_file`.
The running window is driven on a SQLite file through every operation the workspace offers
(browse, edit a row, add and drop a column and an index, run a script, dump and restore). The
server engines are checked on whatever server is reachable, and the report says which were not.

## MixLab

This is MixLab's own `db` module and reaches no daemon; it works the same with `mixengined` absent.
No screen changes: the workspace calls the same `SqlApi`, and the dialects still decide what is
shown. A Tauri command name is not an API anything outside the window calls, so nothing else
breaks; `check-client-surface` covers the daemon's methods and is not affected. Nothing goes in
`CHANGELOG.md`: a user sees no difference. `mix` has no part in this.

## Not in scope

- Mongo and Redis, which keep their own commands and their own workspaces.
- Changing which engines retry a read, or what any operation does.
- Generating the TypeScript types from Rust. The wire types become one set on each side, which
  makes that easier later, but it is its own decision.
- Merging the five `dialect.ts` files. They differ in substance.

## Is there an ADR here? Yes

[ADR 0062](../decisions/0062-the-sql-engines-share-one-command-set.md): the SQL engines share one
command set behind one trait, and an engine is added by implementing the trait.
