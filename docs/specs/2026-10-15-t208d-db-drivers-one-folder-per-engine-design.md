---
status: implemented
date: 2026-10-15
task: T208d
---

# The db backend's drivers: one folder per engine (design)

`apps/desktop/src-tauri/src/modules/db/drivers/` holds 29 Rust files and one SQL fixture side by
side. Eighteen of them are one part of one SQL engine, named `<engine>_<part>.rs`:
`mysql_script.rs`, `sqlite_ddl.rs`, `mssql_editing.rs`. `drivers/mod.rs` declares all 28 modules, and the code
reaches a part as `drivers::mysql_structure`.

T208f gives the SQL engines one trait. Each engine then needs one place where it is implemented,
and its parts should sit under that place. This task makes the folders, and changes only paths.

## D1 — the shape: `<engine>.rs` beside `<engine>/<part>.rs`

| Engine | Stays | Moves into `<engine>/` |
| --- | --- | --- |
| ClickHouse | `clickhouse.rs` | `ddl.rs`, `dump.rs`, `script.rs`, `structure.rs` |
| SQL Server | `mssql.rs` | `ddl.rs`, `dump.rs`, `editing.rs`, `script.rs`, `structure.rs` (`tests.rs` is there already) |
| MySQL | `mysql.rs` | `script.rs`, `structure.rs` |
| PostgreSQL | `postgres.rs` | `ddl.rs`, `script.rs`, `structure.rs` |
| SQLite | `sqlite.rs` | `ddl.rs`, `dump.rs`, `script.rs`, `structure.rs`, `fixture.sql` |

That is 18 Rust files and the fixture. `mongo.rs` and `redis.rs` have no parts and stay as they
are. `dump.rs` (with `dump/tests.rs`), `filters.rs` and `tools.rs` belong to no engine and stay too.

**The roadmap says `drivers/<engine>/mod.rs`. This spec keeps `<engine>.rs` instead**, beside its
folder, for three reasons:

- It is the shape this directory already has. `mssql.rs` sits beside `mssql/tests.rs` and
  `dump.rs` beside `dump/tests.rs` (T207e). A `mod.rs` for the engines would put two shapes in one
  directory.
- The rest of the repository does the same for a module with children:
  `generate/recipes/mysql.rs` beside `recipes/mysql/`, and every `foo.rs` beside `foo/tests.rs`.
- The five engine files do not move, so `git log` and `git blame` on them keep working with no
  `--follow`. Only the 18 part files are renamed.

The roadmap's line for T208d is rewritten to name this shape when it is ticked.

**Module paths.** `drivers::mysql_structure` becomes `drivers::mysql::structure`. `drivers/mod.rs`
keeps 10 `pub mod` lines (the five SQL engines, `mongo`, `redis`, `dump`, `filters`, `tools`). Each
engine file declares its parts as `pub mod ddl;`, `pub mod structure;` and so on. They are `pub`,
as `drivers/mod.rs` declares them today.

**Test names change with the path.** `drivers::mysql_structure::tests::x` becomes
`drivers::mysql::structure::tests::x`. Nothing in CI, `nextest` profiles or scripts names a
driver test, so no filter changes. The number of tests does not change: 807 in this workspace,
424 of them under `drivers::`, counted with `cargo test --locked -- --list` before the move.

## D2 — what is edited

Only paths:

1. **`use` lines and qualified paths inside `drivers/`.** In a moved part, the module one level up
   was `drivers` and is now the part's engine. So, at the part's top level:
   - `super::<engine>::x` becomes `super::x`.
   - `super::<engine>_<other>::x` becomes `super::<other>::x`, a sibling part.
   - `super::dump::…` and `super::filters::…` become `super::super::dump::…` and so on. In the
     three `dump` parts this matters: a plain `super::dump` there would name the part itself.
   - Another engine's part becomes `super::super::<engine>::<part>`. For example,
     `super::clickhouse_structure::SkipIndex` in four `structure` parts becomes
     `super::super::clickhouse::structure::SkipIndex`.
   - `clickhouse_dump.rs` imports its engine as a name, `use super::clickhouse::{self, Connection}`,
     and calls `clickhouse::x`. `super::{self}` cannot bind that name, so this line becomes
     `use crate::modules::db::drivers::clickhouse::{self, Connection}`. It is the one absolute path
     the move adds.

   One level deeper, in a part's `mod tests`, `super::super::` was
   `drivers` and is now the engine. So `super::super::sqlite::tests::Fixture` (four parts) becomes
   `super::super::tests::Fixture`, `super::super::sqlite_structure::table_structure` becomes
   `super::super::structure::table_structure`, and `super::super::mssql_structure::IndexColumn` in
   `mssql_dump.rs` becomes `super::super::structure::IndexColumn`.

   An engine file does not move, so its `super::filters` and `super::mysql` stay as they are. Its
   `super::<engine>_<part>::x` becomes `<part>::x`, a child (`mysql.rs` calls
   `structure::mariadb_expression_defaults`). No engine file has an item named `ddl`, `dump`,
   `script`, `structure` or `editing`, so the new `pub mod` lines take no name already taken.
2. **Paths in `commands/`.** About 147 call sites name a part as `mysql_structure::…` after a
   `use crate::modules::db::drivers::{mysql, mysql_structure, …}`. Both become the engine path:
   the `use` names `mysql`, and the call says `mysql::structure::…`. No alias keeps the old name
   alive. T208f rewrites these files anyway, and an alias would put the old shape back under the
   new one.

   **Five Tauri commands share a name with a part:** `mysql_dump`, `postgres_dump`, `sqlite_dump`,
   `clickhouse_dump` and `mssql_dump`. Each is a `fn` in `commands/<engine>.rs`, registered under
   that name in `modules/mod.rs`. The frontend invokes it by that string. They are not paths and do
   not change. Only `<engine>_<part>::` followed by a name, and an `<engine>_<part>` inside a
   `use … drivers::{…}` list, are rewritten. `commands/clickhouse.rs` and `commands/mssql.rs`
   today import a module `clickhouse_dump` and define a command `fn clickhouse_dump`. The two live
   in different namespaces, and after the move only the `fn` is left.
3. **`include_str!("sqlite_fixture.sql")`** in `sqlite.rs`'s tests becomes
   `include_str!("sqlite/fixture.sql")`, and the file is renamed with it.
4. **Doc comments** that link or name a part by its old path take the new one. That covers three
   intra-doc links and prose mentions such as `mysql_script::run` in `state.rs`. Rustdoc's
   broken-link warning is the check for the links, and a `grep` is the check for the prose.

**Visibility does not change.** A `pub(super)` item in a part used to be visible to all of
`drivers`, and is now visible to its engine. The survey found that every such item
(`read_default`, `structure_columns`, `as_u64`, `Scanner`, `Fed`, `quote_string`,
`column_definition`, `comment_statement`, `end_transaction`, `data_columns`, `page_sizes`) is used only within its own
engine. The items other engines reach, such as `SkipIndex`, are already `pub`. If the compiler finds
one the survey missed, it becomes `pub(in crate::modules::db::drivers)`. That is exactly what its
`pub(super)` meant before, and the commit body names it.

No function body, type, SQL string or test changes.

## D3 — what proves nothing changed

- `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings` and
  `cargo test --locked` in `apps/desktop/src-tauri`. The number of tests run is the same as before,
  and the list of test names matches once the old paths are mapped to the new ones (807 before).
- `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items` for the intra-doc
  links in D2.4. `scripts/gate.sh` does not run rustdoc on this workspace, so it runs on `master`
  first. If `master` already warns, that count is the baseline, and it must not grow.
- `git diff -M` shows 19 renames. A check script lists every changed line that is not a `use`
  line, a `mod` line, a path inside an expression, or a doc comment, and that list is read.
- `bash scripts/gate.sh` before any push.
- This compiles on Windows only here. Of the files that move or are edited, none has an OS
  `#[cfg]`. Only `dump.rs` and `tools.rs` have one, and neither moves or gains a changed path
  under it. So the Linux and macOS legs build the same edits.

## D4 — documentation

`check-docs` fails on dead links. Fifteen markdown links reach a part by its old path, in six files:
`docs/roadmap/desktop/query-editor.md` and in five implemented specs (query-result-grid,
clickhouse-ddl, clickhouse-dump-restore, sqlite-completion, mssql-support). Each is retargeted.

Backticked paths change in the living documents only:

- `docs/standards/desktop/adding-a-command.md`, step 1, says to implement a command "in the right
  `src-tauri/src/modules/db/drivers/*.rs` module". It becomes `drivers/<engine>.rs` or one of its
  parts under `drivers/<engine>/`.
- `docs/architecture/desktop/backend.md`, whose table lists `drivers/mysql_structure.rs` and the
  others one row each. The rows become `drivers/mysql/structure.rs` and so on.
- `docs/roadmap/phase-46-…md`. T208e's open entry names `drivers/sqlite_ddl.rs` and
  `drivers/mysql_structure.rs`, and those names become the new paths. T208d's own line is
  rewritten to the shape built (D1).

Implemented specs and reviews keep their backticked paths as history, as T208c did.

## D5 — commits

On a branch `t208d-drivers-per-engine`, cut from a pushed `master`:

1. The move and the path edits (D1, D2). It does not compile in pieces, so it is one commit.
2. Documentation (D4), the roadmap tick and this spec's status.

## MixLab

This is MixLab's own `db` module backend. It works the same with `mixengined` absent or present.
No Tauri command, its name, its payload or its result changes, so the frontend and
`check-client-surface` see nothing. Nothing goes in `CHANGELOG.md`. `mix` has no part in this.

## Not in scope

- `mongo.rs` and `redis.rs` growing folders. They have no parts to put in one.
- Shrinking any file (T208e) or the shared trait (T208f).
- Renaming a part to say what it holds. `editing.rs` stays `editing.rs`.

## Is there an ADR here? No

## Documentation, when it lands

The phase file ticks T208d and links this spec, and this spec flips to `implemented`.
