# Adding a backend command

A new backend call touches five places. Miss one and it fails at runtime, not at build time. An
operation of the SQL engines has five places of its own, below; everything else follows the five
after it.

## A SQL operation

MySQL, PostgreSQL, SQL Server, ClickHouse and SQLite share one command per operation
([ADR 0062](../../decisions/0062-the-sql-engines-share-one-command-set.md)):

1. **Declare it** on `SqlEngine` in `src-tauri/src/modules/db/drivers/sql/engine.rs`. If not every
   engine has it, give it a default body returning `unsupported("<operation>")`. A type that crosses
   to the window goes in `drivers/sql/types.rs`, matching its TypeScript twin.
2. **Answer it** in `drivers/<engine>/engine.rs` for each engine that has it, calling the functions
   beside that file.
3. **Wrap it** as `sql_<operation>` in `src-tauri/src/modules/db/commands/sql.rs`:
   `handle(&state, &id).await?.sql()?.<operation>(…).await`, and add the name to `COMMANDS` in
   `commands/sql/tests.rs`.
4. **Register it** as `db::commands::sql::sql_<operation>` in `modules/mod.rs`.
5. **Expose it** on `SqlApi` and in `sqlApi`, both in `src/modules/db/sql/api.ts`. If not every
   engine has it, the dialect is what hides the control that would call it.

`cargo test` checks steps 3 to 5 against each other: every `sql_*` command is registered, `sqlApi`
calls each one, and every call sends exactly the arguments its command reads.

## Any other command

1. **Implement it** in the right driver module — `src-tauri/src/modules/db/drivers/mongo.rs` or
   `redis.rs` for the `db` module —
   taking the driver handle (`&mongodb::Client`, `&mut ConnectionManager`) as its
   first argument and returning `Result<T, String>`.

2. **Wrap it** in `src-tauri/src/modules/db/commands/<engine>.rs` as a `#[tauri::command]` that
   takes `state: State<'_, DbState>` plus `id: String`, gets the handle from
   `mongo_client` / `redis_connection` (private helpers in `commands/mod.rs`, reached through
   `use super::…`), and delegates:

   ```rust
   let client = mongo_client(&state, &id).await?;
   mongo::list_collections(&client, &db).await
   ```

   Those helpers release the connection-map lock before returning. Do not lock
   `state.connections` in a command yourself — a query awaited while holding it blocks every other
   command in the app, in every tab.

3. **Register it** in `modules::handler()` in `src-tauri/src/modules/mod.rs`, in that module's
   block of the list, as `db::commands::<engine>::<name>`. This is the step that's easy to forget;
   without it `invoke` rejects the call with "command not found", and nothing at build time says
   so.

4. **Expose it** as a typed function in `src/modules/db/<db>/api.ts`. One exported function per command, its
   doc comment carrying the semantics a caller needs (what a null means, whether paging is stable,
   whether the call is sticky). Nothing outside `api.ts` calls `invoke`.

5. **Type the payload** in `src/modules/db/types.ts` if it returns or accepts a struct. Nothing verifies the
   Rust and TypeScript shapes agree — changing one means changing the other by hand.

## Argument naming

Tauri converts camelCase JS arguments to snake_case Rust parameters. Write `pageSize` in
`invoke(...)` and `page_size: u32` in Rust. Struct *fields* are not converted (serde doesn't rename
them here), so `ConnectionConfig.use_ssl` is snake_case on both sides.

## Verify

`cargo check --manifest-path src-tauri/Cargo.toml` compiles the Rust side; `npm run build`
typechecks the frontend. Neither knows whether step 3 was done — `npm run dev:app` and a click are
the only way to actually exercise the command.
