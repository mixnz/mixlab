# Phase 47 — Code that is safe to change

*Goal: the parts of the code a change most often breaks without a compiler error are written so
that a change to one branch cannot quietly move another. The daemon's start, supervise, stop,
repair and upgrade walks read as named steps. The window's effects depend on what they read. A
test harness, a recipe skeleton or a driver routine exists once. The version-resolving shim
compiles only what it runs.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Found by a review of the whole repository on 2026-10-20, measured on `master` at `e3ffda2c`, after
phase 45 brought every file under 1,000 lines. `cargo clippy --workspace -- -D warnings` was clean,
the code held 13 `#[allow(...)]` and 4 TODO/FIXME. The findings below are about how the code is
organized, not about anything broken. The complexity numbers come from a one-off run with
`-W clippy::too_many_lines -W clippy::cognitive_complexity -W clippy::match_same_arms
-W clippy::struct_excessive_bools`. Those lints are not turned on, and this phase does not turn
them on: a flat routing table scores as badly as a tangled walk, and only the walk is a problem.

---

## Found first

- [x] **T209** The daemon's walks as named steps. Seven functions decide what state comes next in
      one body, with a `match` per failure and a `tokio::select!` in the middle. Their clippy
      cognitive complexity (default limit 25): `serve/repairs.rs` `repair_home` 83,
      `services/runner/attempt.rs` `attempt` 73, `serve/machine.rs:74` 71,
      `api/upgrade/runtime.rs:347` 70 (171 lines), `runtimes/install.rs:320` 65,
      `services/runner/stop.rs:125` 64, `main.rs:125` 61 (162 lines). Each becomes an explicit
      state `enum` and one function per step that takes a state and returns the next. What a step
      decides moves into a pure function tested without spawning a process. The walk that is left
      reads top to bottom as the list of steps. No behaviour changes: the daemon's unit and
      integration test counts stay the same, and each walk gains tests for its decisions. One
      design for all seven, then one pull request with one stage per walk, `attempt` and `stop`
      first, because a wrong branch there stops a user's service. Design:
      [2026-10-20-t209-the-daemons-walks-as-named-steps-design.md](../specs/2026-10-20-t209-the-daemons-walks-as-named-steps-design.md).
      The rule has been in `docs/standards/rust.md`,
      *Structure & style*, since this phase was filed. This task brings the seven walks that
      predate it up to it.
- [x] **T209a** The window's effects depend on what they read. The frontend turns off
      `react-hooks/exhaustive-deps` 9 times and `rules-of-hooks` once. 5 of those 10 are in
      `shell/Workspace.tsx`, where `newTab` and `openTab` "are rebuilt every render". Those become
      stable callbacks (`useCallback`, or read through a ref), and the lint comes back on.
      `QueryEditor.tsx`, `RedisWorkspace.tsx` and `CollationSelect.tsx` each work around "`lang`
      beside `t`" the same way. That becomes one shared hook. `Workspace.tsx` (10 `useEffect`) and
      `modules/rest/RestTab.tsx` (11) are split the way T208a split `DbTab.tsx`, under the
      `splitting-a-react-component` skill, and verified in the running window and with
      `npm run screenshots -- --check`. Done when the only remaining disables are the ones whose
      comment names a reason a callback cannot give. Each of those stays listed here. The rule
      for new code is in `docs/standards/desktop/component-structure.md`, *State and effects*.
      Design:
      [2026-10-20-t209a-the-windows-effects-depend-on-what-they-read-design.md](../specs/2026-10-20-t209a-the-windows-effects-depend-on-what-they-read-design.md).
      None of the ten gave such a reason, so no `react-hooks` disable is left in `src/` and this
      list is empty. "`lang` beside `t`" is `useLanguageT`, the module number chords are one
      `useShortcuts`, and the rest are `useCallback`s and effect events. `Workspace.tsx` is 205
      lines over `shell/workspace/`, and `RestTab.tsx` 299 over `modules/rest/tab/`.
- [x] **T209b** Roadmap numbers stop being added to code comments. `.rs` files cite about 5,600
      task numbers (`T182f`, `T32`, …) and say "roadmap task" 2,281 times. 26% of Rust's
      non-blank lines are comments. The number tells a reader to look elsewhere, and goes wrong
      when a task is split or renumbered. The rule (in `docs/standards/rust.md` since this phase
      was filed) says what is kept and what goes. This task adds the check: a script beside
      `check-file-sizes.mjs` counts the citations per file against a baseline, fails when a file's
      count grows, and is run by `scripts/gate.sh` and CI's `lint` job. Existing citations are not
      rewritten in bulk. They go when the code around them is next changed, and `--shrink` records
      it.
      `scripts/check-task-citations.mjs` counts every `T` and number in the files
      `check-file-sizes.mjs` reads, comments and strings alike, except the scripts that read the
      roadmap. The baseline was written at 6,066 citations in 1,024 files.
- [x] **T209c** Measure duplication in the `db` module's SQL drivers before changing anything.
      `drivers/mssql/` is 4,740 lines, `sqlite/` 3,825, `clickhouse/` 3,641, `postgres/` 3,092,
      `mysql/` 2,383, and the shared `drivers/sql/` only 872. Run a duplicate-code scanner
      (`jscpd` or similar, installed as a dev tool, not a dependency) over
      `src-tauri/src/modules/db/drivers/`. Write down here how many lines are duplicated between
      engines, and in which parts (`dump`, `structure`, `script`, …). Then decide: a part with real
      duplication gets a follow-up task that moves it into `drivers/sql/`, and one without is
      recorded as checked. The answer is a number, as in phase 28.
      **Measured with `jscpd` 4.3.0 (`--format rust`, its defaults of 50 tokens and 5 lines) on
      `master` at `8733802d`.** `drivers/` is 26,397 lines in 58 files, and 2,051 of them (7.77%)
      are in 186 clones. 694 lines in 64 clones repeat code of the same engine and are not this
      task's. **1,543 lines in 122 clones repeat code of another engine.** That is summed per pair
      of files, so a block three engines share counts twice. By part, grouped by the function a
      clone sits in:

      | part | lines | engines | decision |
      | --- | --- | --- | --- |
      | `script`, splitting a script into statements (`split_statements` and its `push`) | ~185 | mssql, mysql, postgres, sqlite | **real**: one lexer frame with the dialect's quoting and comments filled in. T209i |
      | `script`, `run`'s loop over the statements, and `validate` | ~200 | postgres, sqlite, mysql, clickhouse | **real**: the same columns, `MAX_ROWS`, timing and failure per statement. T209i |
      | `page`, `build_where` and `table_data` | ~205 | all five SQL engines | **real**: the `in`, `between`, `isNull` … arms match word for word. T209j |
      | editing rows: `update_row`, `insert_rows`, `delete_rows` | ~174 | postgres, sqlite, mysql, mssql's `editing.rs` | **real for the three sqlx engines**: the transaction, the rollback and `error.rowFailed`. T209k. mssql runs on tiberius and stays its own |
      | `engine.rs`, each `impl SqlEngine` | 193 | every engine | checked: trait signatures that hand each call to the engine's own module, with nothing left to move |
      | `ddl` and `structure` | ~293 | about 20 functions, the longest clone 37 lines | checked: mostly `From<sql::types::*Spec>` into the part of a spec each engine reads, and short input checks in front of SQL that differs per dialect |
      | `dump` and `restore` | ~105 | split across six pairs | checked: small, and spread out |
      | tests, `column_value` and the rest | ~185 | | checked: tests, and decoding written against each driver's own types |

- [x] **T209i** One statement splitter and one `run` loop for the SQL engines. `split_statements`
      in `mssql/`, `mysql/`, `postgres/` and `sqlite/` `script.rs` walks the same frame: line and
      block comments, a quoted run, the first word as the statement's verb, `;` to end it. Only
      the dialect differs: `#` comments and backslash escapes in MySQL, dollar quotes and `E''` in
      PostgreSQL, `[...]` and `GO` batches in SQL Server. That frame moves to `drivers/sql/`
      with the dialect as its argument, and `run`'s per-statement result collection with it, so far
      as the sqlx engines share it. ClickHouse's `Scanner` is compared and joins only if it fits.
      The splitter tests of every engine pass unchanged, and the frontend's copy in
      `src/modules/db/sql/` is left alone. Design:
      [2026-10-20-t209i-one-statement-splitter-design.md](../specs/2026-10-20-t209i-one-statement-splitter-design.md).
      `drivers/sql/syntax.rs` mirrors the window's `SqlSyntax`, and `drivers/sql/statements.rs` is
      the one splitter. `drivers/sql/run.rs` holds the loop PostgreSQL and SQLite share, generic
      over `sqlx::Database`, and the one `MAX_ROWS`. The four `script.rs` went from 2,245 lines to
      1,518, and `drivers/sql/` gained 644, 151 of them tests. SQL Server's `GO` is now read
      in the same pass as strings and comments, as the window reads it: a `GO` line inside either no
      longer cuts the script, which is in the CHANGELOG. MySQL keeps its own loop, because a
      procedure answers one statement with several result sets. **ClickHouse is checked and left
      out:** its `Scanner` resumes across the chunks its restore reads, and it escapes a quoted name
      with a backslash, which no `SqlSyntax` field says.
- [x] **T209j** One `build_where` for the SQL engines. `mysql.rs`, `sqlite.rs` and the `page.rs` of
      `mssql/`, `postgres/` and `clickhouse/` each turn the filter bar into a `WHERE` with the same
      arms. One function in `drivers/filters.rs` takes what differs: how a placeholder is spelled,
      how an identifier is quoted, the regex operator, and PostgreSQL's cast of a bound value. The
      SQL each engine sends is unchanged, which its existing tests and new ones per operator prove.
      Design:
      [2026-10-20-t209j-one-build-where-design.md](../specs/2026-10-20-t209j-one-build-where-design.md).
      `drivers/filters.rs` holds `build_where`, a `WhereSyntax` per engine and the `Placeholders`
      trait, with `QuestionMarks` for MySQL and SQLite and `Numbered` for PostgreSQL and SQL Server.
      ClickHouse keeps its `{pN:Type}` parameters in its own `page.rs`. The five engine files went
      from 2,640 lines to 2,266, and `filters.rs` from 185 to 518, tests included.
      `drivers/filters/where_clauses.rs` pins every engine's clause and binds for all 22 operators,
      written against the five copies before they went.
- [x] **T209k** One frame for editing rows in the sqlx engines. `update_row`, `insert_rows` and
      `delete_rows` in `mysql.rs`, `postgres.rs` and `sqlite.rs` repeat the transaction, the
      rollback on the first failure and `error.rowFailed`. That frame moves to `drivers/sql/`,
      and each engine keeps its placeholders, its key predicate and its bind. SQL Server's
      `editing.rs` runs on tiberius and is left as it is. Comes after T209j, which touches the same
      files. Design:
      [2026-10-21-t209k-one-frame-for-editing-rows-design.md](../specs/2026-10-21-t209k-one-frame-for-editing-rows-design.md).
      `drivers/sql/editing.rs` holds the `RowSql` trait and the frame, generic over
      `sqlx::Database`, and the one bind of an edited value. Each engine writes its SQL in
      `MySqlRows`, `PgRows` or `SqliteRows`, and keeps its early returns, what it reads before the
      transaction and its counter reset. The three engine files went from 2,409 lines to 2,307, and
      `editing.rs` is 174 lines, with 223 of tests in `sql/editing/statements.rs` pinning every
      statement of every engine. The one behaviour change: a failed `UPDATE` now rolls back
      explicitly on MySQL and PostgreSQL too, as SQLite did.

## Found after

- [x] **T209d** One daemon harness for every integration suite. The struct that starts
      `mixengined --home <home>` with `stdout`/`stderr` to null, and kills it in `Drop`, is copied
      almost line for line into about 20 test files (`fn start(` 28 times, `fn drop(` 22 times;
      `crates/mixengine-daemon/tests/idle.rs:44` and `limits.rs:381` are identical). `connect`,
      `call` and `ask` are copied 9 to 11 times. They are copied because
      `env!("CARGO_BIN_EXE_mixengined")` only reaches a test of the package that builds the binary
      (`docs/standards/testing.md`, *Fixtures*). So `mixengine_testkit::Daemon::start(binary:
      &Path, home: &Home)` takes the path as an argument, and each suite passes its own `env!`. A
      move and only a move: test counts unchanged, and no suite keeps a copy. Documented in
      `testing.md`, *Fixtures*, with this task.
      Design:
      [2026-10-21-t209d-one-daemon-harness-design.md](../specs/2026-10-21-t209d-one-daemon-harness-design.md).
      `Daemon` and `Client` live in `mixengine-testkit`; thirteen suites in `mixengine-daemon/tests/`
      and the CLI harness use them, 1,286 lines shorter between them for 271 in the testkit. The
      test lists are the same name for name. One behaviour change, in teardown: every dropped
      `Daemon` is asked to stop before it is killed, as `limits.rs` and the CLI already did; the two
      tests about what a start after a crash does call `Daemon::kill`. `lifecycle.rs` and the `api`
      suite keep their own on purpose.
- [ ] **T209e** The shim compiles only what it runs. `mixengine-shim` runs on every `php`, `node`,
      `python` a user types. It depends on `mixengine-core`, which brings `sqlx` and `reqwest`, so
      its normal dependency tree is 288 crates against `mix`'s 110, and the debug binary is
      12.7 MB. It uses about 15 functions from core: `paths`, `bin_commands::kind`,
      `services::client::*`, `packages::records`, `packages::remember`,
      `runtimes::extensions::conf_d`, `runtimes::java::home`, `certs::ca::certificate_path`,
      `generate::ca::path`. Those move to a small crate with neither dependency that core
      re-exports, or core puts `sqlx` and `reqwest` behind a feature the shim does not turn on. The
      design measures the shim's start time and size, debug and release, before and after.
      `workspace_layering.rs` then fails the build if `sqlx` or `reqwest` reaches the shim again.
      Changing a crate boundary is a cross-cutting decision, so this starts with a design and an
      ADR. **(P)**
- [ ] **T209f** Forms that hold their state as one value. `mixengine/screens/Packages/Languages.tsx`
      has 25 `useState`, `terminal/components/TargetForm/TargetForm.tsx` 21, `db/redis/RedisWorkspace.tsx`
      19, `db/sql/SqlWorkspace.tsx` 18, `mixengine/screens/Extensions/Extensions.tsx` and
      `mixengine/screens/Blueprints/ApplyDialog.tsx` 16. States that change together (loading,
      error, result; a draft and whether it was edited) become one `useReducer`, or one
      discriminated union, so a state that cannot happen cannot be written. Comes after T209a,
      which touches some of the same files. Verified in the running window.
- [ ] **T209g** One skeleton for the service recipes. `mixengine-core/src/generate/recipes/` is
      14,902 lines, almost a fifth of core, and grows with every service MixEngine supports.
      `mysql_family.rs` already shares MySQL and MariaDB. The design compares caddy, nginx,
      mariadb, mysql, postgres, redis, mongodb, php_fpm and memcached, lists what each recipe
      writes the same way (the config file's frame, ports, log paths, datadir, readiness), and
      moves that into `generate/recipe/`. The generated files are byte-identical before and after,
      and the recipe tests prove it.
- [ ] **T209h** Detached tasks that outlive shutdown. `docs/standards/rust.md`, *Async*, already
      says a task that outlives its call is held by something shutdown waits for, with "no
      detached tasks that outlive shutdown". The daemon still has `tokio::spawn` calls whose
      handle nobody keeps, for example `api/service.rs:392`, `api/uninstall.rs:47`,
      `certs/renewal.rs:146` and `certs/handshake.rs:238`. Each one is listed here and either
      joins a `TaskTracker` that shutdown waits for, stopping on the root `CancellationToken`, or
      is written down as safe
      to drop, with the reason. A task that writes state is never safe to drop. The same is
      checked in `apps/desktop/src-tauri`.

**M47** None of the seven walks in T209 is above clippy's cognitive-complexity limit of 25.
`Workspace.tsx` and `RestTab.tsx` turn off no React hooks lint. No integration suite starts
`mixengined` with its own copy of the harness. `cargo tree -p mixengine-shim -e normal` has neither
`sqlx` nor `reqwest`. The task-citation baseline from T209b is lower than the 6,066 it was written at.
`bash scripts/gate.sh`, `cargo test --workspace`, and `npm run build`, `npm test` and
`npm run lint` in `apps/desktop` are green.
