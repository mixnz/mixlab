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
- [ ] **T209b** Roadmap numbers stop being added to code comments. `.rs` files cite about 5,600
      task numbers (`T182f`, `T32`, …) and say "roadmap task" 2,281 times. 26% of Rust's
      non-blank lines are comments. The number tells a reader to look elsewhere, and goes wrong
      when a task is split or renumbered. The rule (in `docs/standards/rust.md` since this phase
      was filed) says what is kept and what goes. This task adds the check: a script beside
      `check-file-sizes.mjs` counts the citations per file against a baseline, fails when a file's
      count grows, and is run by `scripts/gate.sh` and CI's `lint` job. Existing citations are not
      rewritten in bulk. They go when the code around them is next changed, and `--shrink` records
      it.
- [ ] **T209c** Measure duplication in the `db` module's SQL drivers before changing anything.
      `drivers/mssql/` is 4,740 lines, `sqlite/` 3,825, `clickhouse/` 3,641, `postgres/` 3,092,
      `mysql/` 2,383, and the shared `drivers/sql/` only 872. Run a duplicate-code scanner
      (`jscpd` or similar, installed as a dev tool, not a dependency) over
      `src-tauri/src/modules/db/drivers/`. Write down here how many lines are duplicated between
      engines, and in which parts (`dump`, `structure`, `script`, …). Then decide: a part with real
      duplication gets a follow-up task that moves it into `drivers/sql/`, and one without is
      recorded as checked. The answer is a number, as in phase 28.

## Found after

- [ ] **T209d** One daemon harness for every integration suite. The struct that starts
      `mixengined --home <home>` with `stdout`/`stderr` to null, and kills it in `Drop`, is copied
      almost line for line into about 20 test files (`fn start(` 28 times, `fn drop(` 22 times;
      `crates/mixengine-daemon/tests/idle.rs:44` and `limits.rs:381` are identical). `connect`,
      `call` and `ask` are copied 9 to 11 times. They are copied because
      `env!("CARGO_BIN_EXE_mixengined")` only reaches a test of the package that builds the binary
      (`docs/standards/testing.md`, *Fixtures*). So `mixengine_testkit::Daemon::start(binary:
      &Path, home: &Home)` takes the path as an argument, and each suite passes its own `env!`. A
      move and only a move: test counts unchanged, and no suite keeps a copy. Documented in
      `testing.md`, *Fixtures*, with this task.
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
`sqlx` nor `reqwest`. The task-citation baseline from T209b is lower than when it was written.
`bash scripts/gate.sh`, `cargo test --workspace`, and `npm run build`, `npm test` and
`npm run lint` in `apps/desktop` are green.
