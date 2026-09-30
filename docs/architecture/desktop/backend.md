# Backend

Rust, crate `mixlab` (lib name `tauri_app_lib`), edition 2021, async on Tokio.

## Layout

Shared by every module:

| File | Role |
| --- | --- |
| `main.rs` | Six lines: calls `tauri_app_lib::run()`. |
| `lib.rs` | The Tauri builder: plugins, one `register` call per module, `invoke_handler(modules::handler())`. Around 60 lines: before the builder exists it also reads the opening and forwards to a running copy. It names no module's types. |
| `launch.rs` | What the process was started with (`Opening`: the `mixlab://` URL and the credential taken out of the environment), the queue of tabs the backend asks the shell to open, and the one place a URL is matched to a module. |
| `instance.rs` | The pipe/socket between two copies of the app: a second start hands its line to the first and exits. |
| `error.rs` | `AppError` and the `err!` macro. Declared first and with `#[macro_use]`, so everything below it has the macro. |
| `secrets.rs` | The OS credential store, and the three `secrets_*` commands. Keyed by an arbitrary id, so any module can keep something in it. |
| `ssh/mod.rs` | russh-based local port forward (`open_tunnel`), `open_shell`, `test_connection`, and the `SshConfig`/`SshAuth` those take. Shared by db and terminal, and `known_hosts.json` is the app's, not either module's. |
| `tray.rs` | The tray icon and the hidden `tray` panel window: placing it beside the icon, hiding it on blur, close-to-tray for `main`, the icon's native menu, the checks that decide whether a session can show an icon at all, and a login start's hidden window (T168). Since T192 the icon is up on every preset, and `panel` — whether a visible module lends the panel a section — decides whether a primary click opens the panel or the main window ([ADR 0058](../../decisions/0058-the-tray-is-mixlabs-and-a-module-lends-it-a-section.md)). |
| `login_item.rs` | MixLab's own login entry through `tauri-plugin-autostart`, read back from the system on every ask (ADR 0042). |
| `modules/mod.rs` | `handler()` — every command of every module. |

The database module, under `modules/db/`:

| File | Role |
| --- | --- |
| `mod.rs` | `register(builder)`: puts `DbState` and `HandoffState` in the app. |
| `commands/mod.rs` | Connecting, disconnecting, and the private helpers the rest are built on: `handle`, `mysql_pool`, `postgres_pool`, `mongo_client`, `redis_connection`, `sql_endpoint`, `in_background`, `app_data_dir`. |
| `commands/{mysql,postgres,sqlite,mongo,redis,clickhouse,tools}.rs` | The `#[tauri::command]` functions. Thin: look the handle up, delegate to `drivers/`. |
| `commands/handoff.rs` | `handoff_take`: the tab opened for a handed-over connection takes it, once. |
| `models.rs` | `DbKind`, `ConnectionConfig` — the serde types crossing the boundary. |
| `state.rs` | `DbState { connections: Mutex<HashMap<String, ActiveConnection>> }`, `DbHandle`. |
| `handoff.rs` | A `mixlab://connect?…` URL as a `ConnectionConfig`, the rule for which environment variable may hold its password, and `HandoffState`, where it waits for its tab. |
| `drivers/mysql.rs` | Connect, query, row/value conversion, table data, CRUD. |
| `drivers/mysql_structure.rs` | Columns and indexes: read, ADD/CHANGE/DROP. |
| `drivers/mysql_script.rs` | Splits user SQL into statements and runs them one by one. |
| `drivers/postgres.rs` | Connect, list, read a page — with a pool **per database**, since a PostgreSQL connection cannot see into another. |
| `drivers/postgres_structure.rs` | The Structure and Statistics tabs, reported in the shapes `mysql_structure.rs` reports so one grid draws either. |
| `drivers/postgres_ddl.rs` | Tables, columns and indexes changed — one property at a time, unlike MySQL's `CHANGE COLUMN`. |
| `drivers/postgres_script.rs` | The counterpart of `mysql_script.rs`: split, run statement by statement, validate one without running it. |
| `drivers/sqlite.rs` | Connect to a **file**, read a page, write rows. No endpoint, no tunnel, no TLS — and metadata read through the `pragma_*` table-valued functions, since a `PRAGMA` takes no bind parameters. |
| `drivers/sqlite_structure.rs` | The Structure and Statistics tabs. Row counts are counted rather than estimated, and sizes come from `dbstat` — which exists because the app bundles its own SQLite. |
| `drivers/sqlite_ddl.rs` | What SQLite's `ALTER TABLE` can do, which is four things. A column edit that is not a rename is refused by name rather than rebuilt around. |
| `drivers/sqlite_script.rs` | Split and run. No cancel: the statement runs in this process, so there is no session to stop. |
| `drivers/sqlite_dump.rs` | Structure out of `sqlite_master` and back in. The one dump that is not a child process. |
| `drivers/mongo.rs` | Connect, list, find, paging, document CRUD. |
| `drivers/redis.rs` | Connect, SCAN paging, typed value reads, delete. |
| `drivers/clickhouse.rs` | HTTP interface, no `sqlx` driver exists for it. Read-only in v1: list, table data, structure, stats, schema outline — no write commands are registered at all. |
| `drivers/clickhouse_script.rs` | Split and run, one request per statement — the HTTP interface refuses a body holding more than one. `validate` runs `EXPLAIN AST`, which parses without executing. |
| `drivers/dump.rs` | Backup and restore by driving the vendors' own tools — `mysqldump`, `pg_dump`, `mongodump` and their restores. |
| `drivers/tools.rs` | Where those tools come from: the pinned downloads, and `PG_VERSION`. |
| `drivers/filters.rs` | Shared filter-value parsing (`split_list` etc.). |

The REST module, under `modules/rest/` — four files, because the thin part is all of it:

| File | Role |
| --- | --- |
| `mod.rs` | `register(builder)`: puts `RestState` in the app. |
| `commands.rs` | `rest_send` and `rest_cancel`. Builds a `reqwest` request from a `WireRequest`, sends it under a timeout and a `CancellationToken`, reads at most `MAX_BODY` (16 MB) and times the phases. |
| `models.rs` | `WireRequest`, `WireBody`, `RestResponse` — mirrored by hand in `src/modules/rest/types.ts`. |
| `state.rs` | `RestState`: a client per `(follow_redirects, accept_invalid_certs)`, and the token of every send in flight. |

The terminal module, under `modules/terminal/`. **Its comments are written in Vietnamese**, both
here and in `src/modules/terminal/`; every other layer writes them in English. Follow whichever the
file you are in already uses:

| File | Role |
| --- | --- |
| `mod.rs` | `register(builder)`: puts `TerminalState` in the app. |
| `commands.rs` | `terminal_local_shells`, `terminal_open`, `terminal_write`, `terminal_resize`, `terminal_close`. Output goes back on a `Channel`, not as a return value. |
| `local.rs` | The shells this machine has, and a pty session on one of them. |
| `remote.rs` | The same session over SSH, built on `ssh::open_shell`. Same `Session` shape, so nothing above knows which it got. |
| `stream.rs` | The batcher between a shell and the IPC: at most `MAX_CHUNK` (64 KB) a frame, flushed on a short timer so `yes` cannot post thousands of frames a second. |
| `models.rs` | `TerminalTarget` (local or ssh), `TerminalSize`, `LocalShell`, `TerminalEvent`. |
| `state.rs` | `TerminalState`: a `Session` per open tab — the input and resize channels, plus a token whose `Drop` kills the shell. |

## State belongs to the module

`lib.rs` calls `modules::db::register(builder)`, which is a `.manage(DbState::default())`, and one
such call per module — `rest::register`, `terminal::register`. Tauri keys managed state by type, so
each module reaches its own struct through the `State<'_, T>` in its command signature and never
meets another's. No struct at app level knows what a connection, a request or a session is.

## One command list, in blocks

`modules::handler()` holds every command. It is one list because Tauri takes exactly one
`invoke_handler` and `generate_handler!` needs its paths written out rather than collected — but it
is split into a block per module, and **each block is that module's own to edit**. A command
missing from it does not exist at runtime, and nothing at build time says so.

It is fixed to `tauri::Wry` rather than generic over the runtime: a command taking an `AppHandle`
takes `AppHandle<Wry>`, which satisfies `CommandArg` for that one runtime and not for an unknown
`R`. `lib.rs` builds on `Wry` regardless.

## Command shape

Every command follows the same skeleton:

```rust
#[tauri::command]
pub async fn mysql_list_tables(
    state: State<'_, DbState>,
    id: String,
    database: String,
) -> Result<Vec<String>, String> {
    let pool = mysql_pool(&state, &id).await?;
    mysql::list_tables(&pool, &database).await
}
```

Conventions baked into that shape:

- **Errors are `AppError`**, a translation key plus its parameters — see
  [error.rs](../../../apps/desktop/src-tauri/src/error.rs). Write `err!("error.tableNameRequired")`, or
  `err!("error.cannotWriteFile", path = path.display(), message = e)`, and add the key under
  `error.*` in **both** halves of whichever dictionary owns it: `src/modules/db/i18n/{en,vi}.ts`
  for a module's failures, `src/i18n/{en,vi}.ts` for the shared layers' — see
  [i18n](../../standards/desktop/i18n.md). A driver's own words are not translated: they ride
  along as `message` under a code like `error.mysql`. Wrapping one failure in another
  (`err!("error.rowFailed", index = i).caused_by(inner)`) gives the outer message a `{{cause}}`.
- **The connection map is never locked across a query.** `mysql_pool` / `mongo_client` /
  `redis_connection` look the connection up, clone the handle — a pool, a driver client, or an
  `Arc` — and drop the guard before returning it. One lock covers every connection in the app, so
  awaiting under it would make one slow query stop every tab.
- **Connect paths get `with_timeout`** (10s, `DB_CONNECT_TIMEOUT`) so a wrong host fails fast
  instead of hanging the UI.

## Values crossing the boundary

MySQL rows become `serde_json::Map<String, Value>` via `row_to_json`, which reads each column by
its `TypeInfo` — decimals, dates, blobs and JSON all have to be handled explicitly. Write-side
arguments arrive as `Option<String>`: text, or `null` for a real SQL NULL, with an absent key
meaning "leave the column out so its default applies". Keep that three-state distinction when
adding write commands.

The Query tab is different: `MysqlStatementResult.rows` is **positional** (`Vec<Vec<Value>>`), not
keyed, because an arbitrary `SELECT` can return the same column name twice.

## Identifier quoting

Database, table and column names the app interpolates go through `quote_ident` (backtick-quoting,
doubling embedded backticks). Values go through bind parameters. The only text that bypasses both is
SQL the user typed, wrapped in `sqlx::AssertSqlSafe` on purpose.

## Adding a dependency

`src-tauri/Cargo.toml`. Tauri plugins also need their permission added to
`src-tauri/capabilities/default.json` (currently `core`, `opener`, `dialog`, `store`,
`clipboard-manager:allow-read-text`, `updater`, `process:allow-restart`) — a plugin without its
capability entry fails at call time, not at build time.
