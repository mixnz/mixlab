---
status: draft
date: 2026-10-14
task: T207g
---

# The desktop's three drivers past the limit, split the way their siblings already are (design)

Three of MixLab's database drivers under `apps/desktop/src-tauri/src/modules/db/drivers/` are in
`scripts/file-size-baseline.json`: `dump.rs` (2,029), `mssql.rs` (2,088) and `clickhouse.rs`
(1,873). The directory already splits a driver by capability, into flat siblings named
`<engine>_<capability>.rs` — `mssql_ddl`, `mssql_dump`, `mssql_script`, `mssql_structure`,
`mysql_structure`, `postgres_structure`, `sqlite_structure` — and this design applies that rule
where it was not applied, and T207e's test-module convention where code already fits.

`SqlTable.tsx`, the fourth desktop file the roadmap grouped here, is a single React component whose
callbacks share its state; splitting it is a refactor into hooks and components, not a move, and
is task **T207k** of its own (agreed on 2026-10-14).

Roadmap: [phase 45](../roadmap/phase-45-files-that-fit-in-one-read.md), task T207g. The
[parent design](2026-10-13-files-that-fit-in-one-read-design.md)'s D8 holds: a commit only moves.
`apps/desktop/CLAUDE.md`'s rules apply; nothing here spawns a process, adds a command, or changes a
string a user reads.

## D1 — `clickhouse.rs`: the one engine with no `_structure` sibling

Every other SQL engine keeps `table_structure`, `table_stats` and `schema_outline` in
`<engine>_structure.rs`; ClickHouse keeps them at the end of `clickhouse.rs` (lines 1095–1495).
They move to **`clickhouse_structure.rs`**, whole and in order: `StructureColumn`, `IndexColumn`,
`TableIndex`, `SkipIndex`, `parse_type_full`, `TableStructure`, `table_structure`,
`structure_skip_indexes`, `table_engine`, `structure_columns`, `truthy`, `TableStats`,
`table_stats`, `as_u64`, `OutlineColumn`, `OutlineTable`, `SchemaOutline`, `schema_outline`.

Measured by listing every item of the block and searching the crate for it:

- **The block reaches back for two items, both already `pub`**: `Connection` and
  `query_with_params`. No visibility changes.
- **Callers that name a moved item change their path**, `clickhouse::X` →
  `clickhouse_structure::X`: `commands/clickhouse.rs` (`TableStructure`, `table_structure`,
  `TableStats`, `table_stats`, `SchemaOutline`, `schema_outline`), `clickhouse_ddl.rs`
  (`StructureColumn`, `structure_columns`, `as_u64`), `clickhouse_dump.rs` (`as_u64`), and the
  four other engines' `_structure.rs`, which report their skip indexes with `clickhouse::SkipIndex`.
- **`parse_type_full`'s four tests** go with it; the rest of `clickhouse.rs`'s tests stay.

`clickhouse.rs` ends at about 1,450 lines with its tests inline and leaves the baseline.

## D2 — `mssql.rs`: writing a row is a capability of its own

`mssql.rs`'s code is 1,649 lines with `mssql_structure.rs` already apart. What is left that is one
subject is **writing**, and the frontend already names it: `src/modules/db/mssql/editing.ts`. It
moves to **`mssql_editing.rs`**: `update_row`, `insert_rows`, `delete_rows` and their `_body`
halves, `reset_identity`, and the helpers nothing but they use — `end_transaction`, `bind_write`,
`is_money_type`, `reject_money_thousands_separator`, `decode_binary`, `full_object_name`,
`key_predicate` (438 lines, measured).

- **What they reach that stays**: `quote_ident`, `resolve`, `Connection`, `Pool`, `map_error`,
  `three_part`, `table_columns`, `is_binary_type` — all already `pub` or `pub(super)` — and
  `MONEY_TYPES`, which `select_expr` also reads, so it stays and becomes `pub(super)`. That is the
  one visibility change.
- **Callers**: `commands/mssql.rs` (`update_row`, `insert_rows`, `delete_rows`) and `mssql_ddl.rs`
  (`end_transaction`) change their path to `mssql_editing::`.
- **Eight tests go with what they test**: the three money tests, the two base64 tests,
  `full_object_name_always_carries_its_schema`, and the two `key_predicate` tests.

That leaves `mssql.rs` at about 1,210 lines of code and about 345 of tests — past 1,500 together,
under it apart — so its test module then moves to `mssql/tests.rs` by T207e's convention.

## D3 — `dump.rs`: T207e as it stands

Code 1,395, tests 635: the test module moves to `dump/tests.rs`, nothing else changes.

## D4 — what a move could change, and why here it does not

- **The commands.** A Tauri command is registered by its function in `commands/`, which do not
  move; only the driver function a command calls is reached by a new path. No command name, no
  argument, no answer changes, so the frontend's `invoke` calls and the TypeScript types mirroring
  the Rust models are untouched — the moved `struct`s keep their names and their `serde`
  attributes, which is all the wire sees.
- **Order and attributes.** Items move with every `///`, `//` and `#[...]` line above them, in the
  order they had, as in T207d.
- **Strings.** The test module moves of `dump.rs` and `mssql.rs` are dedented except inside string
  literals, and every literal is compared before and after, as in T207e.
- **Not the root workspace.** `apps/desktop/src-tauri` is a workspace of its own: checks are
  `cargo fmt --check` and `cargo clippy --locked --all-targets -- -D warnings` there, and its unit
  tests by `cargo test --lib -- --list`, counted before and after. The privileged helper does not
  compile from it, so `helper.lock` is not involved.

## D5 — comments that will be wrong after the move

Three name a moved item at its old path, in prose: `clickhouse_ddl.rs`'s
"`clickhouse::StructureColumn`" and "`clickhouse::SkipIndex`", and `mssql_ddl.rs`'s
"`mssql::reset_identity`". They are fixed in a commit of their own after the moves, as T207c did,
with any others the build's rustdoc reports.

## D6 — commits and verification

1. `clickhouse_structure.rs`. 2. `mssql_editing.rs`. 3. `mssql.rs`'s tests to `mssql/tests.rs`.
4. `dump.rs`'s tests to `dump/tests.rs`. 5. The comments of D5. 6. The roadmap tick.

Each move commit: the unit-test count of `apps/desktop/src-tauri` unchanged; rustfmt, clippy with
`-D warnings` and the moved tests run there; `git diff --color-moved` leaving only `use` lines,
paths, the one visibility, headers and wrappers; `node scripts/check-file-sizes.mjs --shrink`.
Then the repository's gate (which builds and lints the desktop), `ask-ci` — whose `desktop` job and
`build` legs compile the window on all three systems — one pull request, squash-merged.

## MixLab

This is MixLab's own code, and nothing a user sees changes: the database module's commands, their
answers and every screen drawing them are as they were. No `## [Unreleased]` line, since nothing
is noticeable.

## Not in scope

- `SqlTable.tsx` — T207k.
- Splitting any driver below the limit, or giving MySQL, PostgreSQL or SQLite an `_editing.rs` to
  match. Each would be a reasonable symmetry and none is needed by the ratchet.

## Is there an ADR here? No

The rule is the directory's own; nothing cross-cutting is decided.

## Documentation, when it lands

- `docs/roadmap/phase-45-files-that-fit-in-one-read.md`: T207g ticked and narrowed to the three
  drivers, T207k added for `SqlTable.tsx`; this spec flips to `implemented`.
- `apps/desktop/CLAUDE.md`'s layout lists `drivers/` without its files and needs nothing.
