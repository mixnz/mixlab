# Adding a backend command

A new backend call touches five places. Miss one and it fails at runtime, not at build time.

1. **Implement it** in the right driver module — `src-tauri/src/modules/db/drivers/<engine>.rs`,
   or one of its parts under `drivers/<engine>/` (`ddl.rs`, `structure.rs`, `script.rs`…) —
   taking the driver handle (`&MySqlPool`, `&mongodb::Client`, `&mut ConnectionManager`) as its
   first argument and returning `Result<T, String>`.

2. **Wrap it** in `src-tauri/src/modules/db/commands/<engine>.rs` as a `#[tauri::command]` that
   takes `state: State<'_, DbState>` plus `id: String`, gets the handle from `mysql_pool` /
   `mongo_client` / `redis_connection` (private helpers in `commands/mod.rs`, reached through
   `use super::…`), and delegates:

   ```rust
   let pool = mysql_pool(&state, &id).await?;
   mysql::list_tables(&pool, &database).await
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
