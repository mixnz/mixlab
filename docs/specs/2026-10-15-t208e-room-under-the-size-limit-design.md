---
status: approved
date: 2026-10-15
task: T208e
---

# Room under the size limit for seven backend files (design)

Seven Rust files under `apps/desktop/src-tauri/src/` sit within 125 lines of the 1,500-line limit
(T207). None of them is in `scripts/file-size-baseline.json`, so the next feature in any of them
fails the gate. M46 asks for no file under `apps/desktop/` over 1,350 lines.

Measured on `master` after T208d, with `wc -l`:

| File | Lines | Code | Tests | What it needs |
| --- | ---: | ---: | ---: | --- |
| `modules/db/drivers/sqlite/ddl.rs` | 1,495 | 906 | 589 | tests out |
| `sync/session.rs` | 1,470 | 1,320 | 150 | tests out |
| `modules/db/drivers/clickhouse.rs` | 1,439 | 1,099 | 340 | tests out |
| `modules/db/drivers/postgres.rs` | 1,430 | 1,224 | 206 | tests out |
| `modules/db/drivers/dump.rs` | 1,396 | 1,394 | in `dump/tests.rs` already | split by capability |
| `modules/db/drivers/sqlite.rs` | 1,390 | 846 | 544 | tests out |
| `modules/db/drivers/mysql/structure.rs` | 1,378 | 1,076 | 302 | tests out |

## D1 — six files: the test module goes to `foo/tests.rs`

The convention of `docs/standards/rust.md` (T207e): the file keeps `#[cfg(test)] mod tests;` and
the module's body moves to `foo/tests.rs`. The module path stays `foo::tests`, so no test is
renamed and none is added or lost.

- `sqlite/ddl.rs` → `sqlite/ddl/tests.rs`
- `sync/session.rs` → `sync/session/tests.rs`
- `clickhouse.rs` → `clickhouse/tests.rs`, beside its parts
- `postgres.rs` → `postgres/tests.rs`, beside its parts
- `sqlite.rs` → `sqlite/tests.rs`. Its module is `pub(super) mod tests` because the four parts
  reach its `Fixture` as `super::super::tests::Fixture`. The declaration keeps `pub(super)`, and
  those paths still resolve.
- `mysql/structure.rs` → `mysql/structure/tests.rs`

The body is moved as is, one indent level out, and its `use super::*;` keeps working since the
module's parent does not change. `sync/session.rs` ends at 1,320 lines, the largest of the six,
and under 1,350.

## D2 — `dump.rs`: each tool's dump goes to its engine

`dump.rs` holds two things. One is what every engine's backup shares: `DumpMode`, `Progress`,
`Watch`, and the machinery that drives a child process (`run`, `Invocation`, `Tick`, `Fed`,
`Tail`, `Tracker`, the file helpers, `share`). The other is three tools, one engine each:
`mysqldump`/`mysql`, `pg_dump`/`psql`, `mongodump`/`mongorestore`.

T208d gave each engine a folder, and three engines already keep their backup in
`<engine>/dump.rs` (ClickHouse, SQL Server, SQLite). The three tools move to the same place:

| From `dump.rs` | To | About |
| --- | --- | ---: |
| `OptionFile`, `TABLE_LINE`, `reached_table`, `is_mariadb_tool`, `mysqldump_args`, `mysql_dump`, `mysql_restore_args`, `mysql_restore` | `mysql/dump.rs` | 290 |
| `PgPassFile`, everything `pg_*`, `psql_args`, `postgres_dump`, `postgres_restore` | `postgres/dump.rs` | 360 |
| `split_uri`, `tool_uri`, `archive_*`, `ARCHIVE_*`, `mongo_dump`, `mongo_restore` | `mongo/dump.rs` | 215 |

`dump.rs` keeps the shared part, about 560 lines, and its module comment is reworded to say so
and to point at the three. `mongo.rs` gains `pub mod dump;` and so a folder of its own, the shape
T208d gave the SQL engines.

**Names.** In its engine's module the function no longer needs the engine in its name, so
`dump::mysql_dump` becomes `mysql::dump::dump` and `dump::mysql_restore` becomes
`mysql::dump::restore`, as `clickhouse::dump::restore` already is. The same for PostgreSQL and
MongoDB. The six callers in `commands/{mysql,postgres,mongo}.rs` change their path. The private
helpers keep their names (`pg_dump_args` stays `pg_dump_args`): a rename there would make the
move harder to review for nothing.

**Visibility.** What the three now reach across modules (`run`, `Invocation`, `Tick`, `Tracker`,
`create_file`, `open_file`, `share`, `Fed`, `Lead`, with the fields and methods the three use)
becomes `pub(super)`, which from `drivers::dump` is the whole of `drivers/`, as `Tracker` already
is. Never `pub`: nothing outside `drivers/` gains access to anything.

**Tests go with what they test.** `dump/tests.rs` (628 lines) splits by its own `use` lines:
the MySQL argument tests to `mysql/dump/tests.rs`, the PostgreSQL ones (arguments, preamble,
`pg_reached_table`) to `postgres/dump/tests.rs`, the MongoDB ones (URI, archive) to
`mongo/dump/tests.rs`. The `run` and `Tracker` tests, with `sleeper`, `slow_tool` and `Written`,
stay in `dump/tests.rs`. These tests change module path, as T208d's did. The count stays the same,
taken with `cargo test --locked -- --list` before and after.

`#[cfg(unix)]` sits inside `OptionFile` and `PgPassFile` and moves with them. No new OS `#[cfg]`
appears.

## D3 — checks

- `cargo test --locked -- --list | wc -l` in `src-tauri/`, before and after: equal.
- `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo test` in
  `src-tauri/`.
- `node scripts/check-file-sizes.mjs`: green, and none of the seven above 1,350.
- `bash scripts/gate.sh`.
- A real backup and restore of a MySQL and a PostgreSQL database from the running window, if a
  server is at hand here. The move changes no code, so this is a check on the paths, not on the
  dumps.

## D4 — documentation

- The phase file ticks T208e and links this spec. The entry's list of files stays as written.
- `docs/architecture/desktop/backend.md`, if its table names `drivers/dump.rs` as the home of the
  three tools, gets rows for `mysql/dump.rs`, `postgres/dump.rs` and `mongo/dump.rs`.
- The module comment of `sqlite/dump.rs` says `dump.rs` runs `mysqldump`, `pg_dump` or
  `mongodump`. It is reworded to name where those now live.
- Implemented specs keep their paths as history.

## D5 — commits

On a branch `t208e-room-under-the-size-limit`, cut from `master`:

1. The six test modules out (D1).
2. `dump.rs` split (D2). It does not compile in pieces, so it is one commit.
3. Documentation (D4), the roadmap tick and this spec's status.

## MixLab

This is MixLab's own `db` module backend and its sync session. It works the same with `mixengined`
absent or present. No Tauri command, its name, its payload or its result changes, so the frontend
and `check-client-surface` see nothing. Nothing goes in `CHANGELOG.md`. `mix` has no part in this.

## Not in scope

- Shrinking the code of `sync/session.rs` (1,320). It is under 1,350; splitting its lock and its
  lending apart is a design of its own.
- Any file between 1,000 and 1,375 lines not named above (`mssql.rs`, `mongo.rs`, …).
- The shared SQL trait (T208f), which will rewrite the engine files again.

## Is there an ADR here? No
