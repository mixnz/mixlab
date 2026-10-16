# 0062. The SQL engines share one command set

**Status**: Proposed
**Date**: 2026-10-15

## Context

MixLab's `db` module reaches five SQL engines: MySQL, PostgreSQL, SQL Server, ClickHouse and
SQLite. Each grew its own Tauri commands, named `<engine>_<operation>`, 135 in all. Each set is
registered in `modules/mod.rs` and wrapped again in an `api.ts` of its own.

The frontend stopped working that way some time ago. The shared SQL workspace calls one `SqlApi`
and asks one `SqlDialect` what to show, and the five `api.ts` files are five copies of the same
object with a different prefix on every `invoke`. The command signatures already agree across
engines. Only the Rust types behind them differ, and the frontend sends one TypeScript type for
each of those.

So one operation costs five commands, five registrations and five wrappers. A sixth engine would
cost about 28 of each, and nothing checks that the five copies of an operation stay alike.

## Decision

- **A SQL operation is one method of one trait, `SqlEngine`**, in `drivers/sql.rs`. Each engine
  implements it once. An operation an engine does not have is a default method that returns a
  typed `unsupportedOperation` error.
- **`DbHandle` is the one dispatch.** `DbHandle::sql()` turns a connection into a
  `Box<dyn SqlEngine>`. No command matches on the engine.
- **One Tauri command per operation, named `sql_<operation>`.** The arguments and payloads are
  the ones the frontend sends today. A command that needs no connection keeps its engine's prefix
  (`sqlite_create_file`).
- **The trait's signatures use one set of wire types**, matching the TypeScript types the frontend
  already has. An engine that reads less of a type converts it into its own.
- **The frontend has one `sqlApi`.** What tells engines apart in the window is the dialect, as it
  already is.
- **Adding a SQL engine means implementing `SqlEngine` and writing a dialect.** It adds no
  commands.
- **MongoDB and Redis keep their own commands.** They have workspaces of their own and share no
  operations with the SQL engines worth a trait.

## Consequences

- `modules/mod.rs` registers 32 SQL commands in place of 135, and the five engine `api.ts` files
  are gone. An operation added later is written once in the trait, once per engine that has it,
  and once in `sqlApi`.
- An engine that lacks an operation says so with a typed error that the window can show. Today the
  frontend fills the gap with stubs. The dialect still decides which buttons exist, so in normal
  use nobody sees the error.
- The command names change. Nothing outside the window calls them, so there is no compatibility to
  keep. The window and its backend ship as one binary.
- `docs/standards/desktop/adding-a-command.md` keeps its five-place rule for MongoDB, Redis and
  the other modules. For a SQL operation the places are the trait, each engine's implementation,
  the `sql_*` command, its registration and `sqlApi`.
- Rust and TypeScript now have one wire type each per SQL shape. Generating one side from the
  other becomes possible but is not decided here.

Design: [2026-10-15-t208f-one-command-set-for-sql-engines-design.md](../specs/2026-10-15-t208f-one-command-set-for-sql-engines-design.md).
