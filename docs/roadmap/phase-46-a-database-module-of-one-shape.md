# Phase 46 — A database module of one shape

*Goal: MixLab's `db` module is built the way the rest of the window already is. Its frontend
reaches the backend only through `api.ts`. No component of it carries a whole screen's state. Its
Rust answers the five SQL engines through one set of commands instead of one set per engine, and no
file in it sits within a few lines of the 1,500-line limit.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Found by a review of `apps/desktop` on 2026-10-15 (measured on `master`). `tsc`, `npm run lint` and
2,880 vitest tests were green. The findings below are about how the code is organized, not about
anything broken. The `db` module is about 29k lines of TypeScript and 30k of Rust, about 60% of the
window, and almost every finding sits in it.

---

## The frontend

- [x] **T208** Every `invoke` of the `db` module goes through `src/modules/db/api.ts`, as
      `apps/desktop/CLAUDE.md` already requires. Today `DbTab.tsx`, `tools.ts`, `transfer.ts`,
      `tunnel.ts`, `handoff.ts` and `savedConnections.ts` each call it themselves. `secrets_save`,
      `secrets_load` and `secrets_delete` are wrapped three times, in `db/savedConnections.ts`,
      `terminal/savedTargets.ts` and `rest/api.ts`. They become one helper in `src/core/`. An
      eslint rule then rejects an import of `invoke` from `@tauri-apps/api/core` in any
      `src/modules/**` file other than an `api.ts`, so the rule is checked by a tool and not
      only written down.
- [x] **T208a** `db/DbTab.tsx` (1,038 lines, 17 `useState`) split the way T207k split
      `SqlTable.tsx`. A hook owns a connection's lifecycle: connect, test the SSH tunnel,
      disconnect, and what a closed tab leaves behind. The tab keeps only the choice between the
      connection form and the workspace. Verified in the running window and with
      `npm run screenshots -- --check`. Design:
      [2026-10-15-t208a-db-tab-connection-lifecycle-design.md](../specs/2026-10-15-t208a-db-tab-connection-lifecycle-design.md).
- [x] **T208b** The same for the next two components of one piece:
      `db/components/TableStructure/TableStructure.tsx` (1,109) and
      `mixengine/screens/Dashboard/Dashboard.tsx` (1,107). Each gets hooks for what it does and
      components for what it draws. The Dashboard half is checked with MixEngine running and with
      it stopped. Design:
      [2026-10-15-t208b-table-structure-and-dashboard-hooks-design.md](../specs/2026-10-15-t208b-table-structure-and-dashboard-hooks-design.md).
- [ ] **T208c** The `db` module's folders follow its engines. The 40-odd files at the root of
      `src/modules/db/` go into folders by subject: `saved/` (connections, their store, their
      scroll), `queries/` (drafts, history, snippets), `sync/` and `tab/`. The components only
      Redis or Mongo draws (`RedisKeyList`, `RedisValue`, `RedisGroupKeys`, `RedisTypeBadge`,
      `Document`, `DocumentNode`, `NoSqlTable`, `InsertDocumentsDialog`) move to `redis/` and
      `mongo/`, so `components/` holds what every engine shares. Only files move: the vitest
      count stays the same.

## The backend

- [ ] **T208d** `src-tauri/src/modules/db/drivers/` gets one folder per engine:
      `drivers/<engine>/{mod,ddl,structure,script,dump,editing}.rs`, in place of 28 files named
      `<engine>_<part>.rs` beside two folders (`mssql/`, `dump/`) that already have that shape.
      Only files move: the Rust test count stays the same. It comes before T208f so that the
      shared trait has one place per engine to be implemented in.
- [ ] **T208e** Room under the size limit. Seven files are within 125 lines of 1,500:
      `drivers/sqlite_ddl.rs` (1,495), `sync/session.rs` (1,470), `drivers/clickhouse.rs`
      (1,434), `drivers/postgres.rs` (1,426), `drivers/dump.rs` (1,396), `drivers/sqlite.rs`
      (1,385) and `drivers/mysql_structure.rs` (1,378). The next feature in any of them would
      hit the gate. Tests go out to `foo/tests.rs` under T207e's convention, and the files whose
      code alone is near the limit split by capability, as T207h did.
- [ ] **T208f** One command set for the SQL engines. The frontend already sees MySQL, PostgreSQL,
      SQL Server, ClickHouse and SQLite through one `SqlApi`/`SqlDialect`. The Rust does not: it
      has about 25 commands per engine (`mysql_list_tables`, `postgres_list_tables`, …), about
      130 in all, and the same names again in `modules/mod.rs` and in five `api.ts` files. The
      engines get one trait, dispatched on `DbHandle`, and one command per operation
      (`sql_list_tables`, …). The five frontend `api.ts` files then shrink to one. Changing the
      command surface is a cross-cutting decision, so it starts with a design and an ADR. Mongo
      and Redis keep their own commands.
- [ ] **T208g** `unwrap()` and `expect()` in code that ships. `src-tauri/src` has about 1,040 of
      them, tests included. The ones outside `#[cfg(test)]` are counted, each one either becomes
      a typed error returned to the window or keeps an `expect` whose message says why it cannot
      fail, and the count is written down here.

**M46** No file under `src/modules/` imports `invoke` except an `api.ts`. `DbTab.tsx`,
`TableStructure.tsx` and `Dashboard.tsx` are each under 500 lines. `modules/mod.rs` registers one
set of SQL commands rather than five. No file under `apps/desktop/` is over 1,350 lines.
`npm run build`, `npm test`, `npm run lint` and `cargo clippy --all-targets -- -D warnings` in
`src-tauri/` are green.
