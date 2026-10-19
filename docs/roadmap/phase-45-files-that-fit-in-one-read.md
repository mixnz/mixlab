# Phase 45 — Files that fit in one read

*Goal: no source file grows past 1,500 lines, none already past it grows at all, `mix`'s
`main.rs` and `render.rs` (7,651 and 7,741 lines) become one module per command, the daemon's
`api/rpc.rs` (5,683) keeps the routing table and hands every handler to its capability, and its
`services/mod.rs` (4,613) keeps the `Registry` and its state and hands every walk to its own file.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Design: [2026-10-13-files-that-fit-in-one-read-design.md](../specs/2026-10-13-files-that-fit-in-one-read-design.md).

---

- [x] **T207** `scripts/check-file-sizes.mjs` and its test: a 1,500-line limit on tracked `.rs`,
      `.ts`, `.tsx`, `.js`, `.mjs` and `.css` files, and `scripts/file-size-baseline.json` naming
      the 34 files already past it at their current length; red on a new file over the limit, on a
      baselined file that grew, and on one that shrank without `--shrink`. Run by `scripts/gate.sh`
      and CI's `lint` job; documented in `docs/standards/rust.md` and `CLAUDE.md`.
- [x] **T207a** `crates/mixengine-cli/src/main.rs` moved into `commands/<name>.rs`, one module per
      command, with the helpers several commands share at the crate root (`output.rs`, `parse.rs`,
      `jobs.rs`, `prerequisites.rs`, `upgrades.rs`). A move and only a move: the command reference
      (`packaging/docs.sh --check`) and the unit-test count are unchanged, and `main.rs` leaves the
      baseline.
- [x] **T207b** `crates/mixengine-cli/src/render.rs` split the same way, `render/<name>.rs` beside
      `commands/<name>.rs`. Design:
      [2026-10-13-render-split-by-command-design.md](../specs/2026-10-13-render-split-by-command-design.md).

- [x] **T207c** `crates/mixengine-daemon/src/api/rpc.rs` split by capability: the routing table
      and the protocol stay, every `Api` handler and its helpers move to `api/<capability>.rs`
      (`service`, `service_settings`, `daemon`, `self_update`, `uninstall`, `disk`, `cert`,
      `metrics`), and each test goes with the handler it asserts on, over a shared
      `api/test_daemon.rs`. A move and only a move: 705 unit tests before and after, and `rpc.rs`
      leaves the baseline. Design:
      [2026-10-13-t207c-rpc-split-by-capability-design.md](../specs/2026-10-13-t207c-rpc-split-by-capability-design.md).
- [x] **T207d** `crates/mixengine-daemon/src/services/mod.rs` split by capability: the `Registry`,
      its fields and the types they are made of stay, and its operations move to
      `services/<capability>.rs` (`graph`, `start`, `stop`, `shutdown`, `recovery`), each test with
      the method it asserts on, its shared helpers in `services/fixture.rs`. A move and only a
      move: 705 unit tests before and after, and `mod.rs` leaves the baseline. Design:
      [2026-10-13-t207d-services-split-by-capability-design.md](../specs/2026-10-13-t207d-services-split-by-capability-design.md).

The 30 files left in the baseline after T207d, grouped by how they split rather than one design
per file (measured on `master` at `c2e02e8d`: code before `mod tests`, and the test module):

- [x] **T207e** Test modules out to `foo/tests.rs`, for the 15 files whose code is under 1,500 and
      only the tests push them past it: core `generate.rs`, `services.rs`, `sites.rs`,
      `manifest.rs`, `elevation.rs`, `blueprints/manifest.rs`,
      `generate/recipes/{caddy,postgres,mariadb,mysql}.rs`; proto `privileged.rs`; daemon
      `elevation.rs`, `sites.rs`, `uninstall/inventory.rs`, `api/apply.rs`. `foo.rs` keeps `#[cfg(test)] mod tests;`, the
      bodies do not change and **test paths stay `foo::tests::x`**, so nextest's filters and the
      unit-test counts are untouched. The convention (agreed 2026-10-14) goes into
      `docs/standards/rust.md` with it. One design, one pull request. Daemon `elevation.rs` (code
      1,474) and `sites.rs` (1,401) leave the baseline with little headroom. Design:
      [2026-10-14-t207e-test-modules-to-their-own-file-design.md](../specs/2026-10-14-t207e-test-modules-to-their-own-file-design.md).
- [x] **T207f** The same, where the tests alone are past 1,500 and split by topic into
      `foo/tests/<topic>.rs`: core `blueprints/plan.rs` (tests 2,437) and
      `generate/recipes/nginx.rs` (1,597); and the integration suite
      `crates/mixengine-daemon/tests/runtimes.rs` (2,224), into a directory of modules behind one
      test binary. Design:
      [2026-10-14-t207f-tests-split-by-topic-design.md](../specs/2026-10-14-t207f-tests-split-by-topic-design.md).
- [x] **T207g** The desktop's three database drivers, under `apps/desktop/CLAUDE.md`'s rules and
      its own workspace, split the way their siblings already are: `clickhouse.rs`'s structure to
      a `clickhouse_structure.rs` like every other engine's, `mssql.rs`'s writes to
      `mssql_editing.rs` and then its tests to `mssql/tests.rs`, and `dump.rs`'s tests to
      `dump/tests.rs`. Design:
      [2026-10-14-t207g-desktop-drivers-split-design.md](../specs/2026-10-14-t207g-desktop-drivers-split-design.md).
- [x] **T207h** Definitions whose code is past 1,500, with little state: proto `service.rs`
      (code 1,814), core `generate/recipe.rs` (1,521), platform `process.rs` (1,508) and
      `windows/process.rs` (1,590), each giving one capability a file: `service/policy.rs`,
      `generate/context.rs`, `process/once.rs`, `windows/process/child.rs`. Design:
      [2026-10-14-t207h-definitions-split-by-capability-design.md](../specs/2026-10-14-t207h-definitions-split-by-capability-design.md).
- [x] **T207i** The daemon's `services/runner.rs` (code 2,418) and `doctor.rs` (1,615), each one
      `impl` split by what its methods do, as T207d split `Registry`: `runner/attempt.rs`,
      `stop.rs`, `environment.rs`, `reload.rs`; `doctor/installs.rs`, `trust.rs`. Design:
      [2026-10-14-t207i-runner-and-doctor-split-design.md](../specs/2026-10-14-t207i-runner-and-doctor-split-design.md).
- [x] **T207j** Core `lib.rs` cannot be split by moving: lines 92–1935 are one `enum Error`, and
      an enum does not span files. Decide between error enums per subject (a change to code, and
      perhaps an ADR) and keeping it in the baseline with the reason written there. Decided on
      2026-10-14: nine per-subject enums wrapped by `Error`, as `GraphError` is. Design:
      [2026-10-14-t207j-core-error-groups-design.md](../specs/2026-10-14-t207j-core-error-groups-design.md).
- [x] **T207k** The desktop's `db/components/SqlTable/SqlTable.tsx` (1,843): one React component
      whose callbacks share its state, so not a move — custom hooks for what it does (editing a
      cell, selecting rows, the filters it remembers) and components for what it draws, verified in
      the running window and `npm run screenshots -- --check`. Split out of T207g on 2026-10-14.
      Design: [2026-10-14-t207k-sql-table-hooks-design.md](../specs/2026-10-14-t207k-sql-table-hooks-design.md).
- [x] **T207l** The daemon's `main.rs` (code 2,439): `serve` is one function of 1,180 lines, forty
      startup steps sharing dozens of locals, so not a move — the steps become functions with the
      state they share passed explicitly, in the order a start needs, and `Args` and `detach` move
      out. Split out of T207i on 2026-10-14. Design:
      [2026-10-14-t207l-daemon-serve-steps-design.md](../specs/2026-10-14-t207l-daemon-serve-steps-design.md).
- [x] **T207m** Test modules out to `foo/tests.rs` ahead of a 1,000-line limit. A limit of 1,000
      would put 76 files in the baseline. In 28 of them the code is under 1,000 and only the
      inline tests push the file past it: seven of the desktop's (`db/drivers/mongo.rs`,
      `secrets.rs`, `sync/engine.rs`, …), twelve of core's, five of the daemon's, three of proto's
      and the shim's `main.rs`. Each moves under T207e's convention. Five files hold a second test
      module, which moves to a file of its own name (`shims/claim_tests.rs`), so no test changes its
      path. Lowering the limit itself is a later design. Design:
      [2026-10-17-t207m-test-modules-out-before-a-1000-line-limit-design.md](../specs/2026-10-17-t207m-test-modules-out-before-a-1000-line-limit-design.md).

**M45** `scripts/file-size-baseline.json` no longer names `crates/mixengine-cli/src/main.rs`,
`render.rs`, `crates/mixengine-daemon/src/api/rpc.rs` or
`crates/mixengine-daemon/src/services/mod.rs`, and `bash scripts/gate.sh` is green.


Since T207j the size baseline is empty: no source file in the repository is over 1,500 lines, and
`node scripts/check-file-sizes.mjs` now holds every file to the limit itself.

- [x] **T207n** The limit goes down to 1,000 lines, for every counted extension alike. After T207m,
      48 files are past it: 43 `.rs`, the desktop's `db/i18n/en.ts` and `vi.ts`, `db.css` and
      `App.css`, and the worker's `account.ts`. `scripts/file-size-baseline.json` names them at
      their length, written once, since `--shrink` never adds an entry. From here they only shrink.
      `check-file-sizes.test.mjs` pins the checked-in limit and the baseline's sorted form.

The 48 files T207n named, grouped by how they split and how a split is checked (measured on
`d612d5d2`). Each task ends with `--shrink`, and after T207q the baseline is empty again.

- [x] **T207o** Test files past 1,000 split by topic, under T207f's convention: a unit-test hub
      becomes `foo/tests/<topic>.rs`, and an integration suite becomes `tests/<name>/main.rs`
      beside its topics, still one binary. Sixteen files: core `blueprints/plan/tests/resolving.rs`,
      `generate/recipes/caddy/tests.rs` (1,425), `nginx/tests/sites.rs`, `postgres/tests.rs`,
      `generate/tests.rs`, `manifest/tests.rs`, `services/tests.rs`, `sites/tests.rs` and
      `tests/index.rs`; daemon `elevation/tests.rs`, `tests/api.rs`, `tests/packages.rs` and
      `tests/runtimes/install.rs`; cli `tests/harness/frontend.rs`, `tests/mariadb.rs` and
      `tests/uninstall.rs`. Only tests move: the counts per binary stay the same, and the filters
      in `.config/nextest.toml` are checked against the new paths. Design:
      [2026-10-17-t207o-test-files-split-by-topic-design.md](../specs/2026-10-17-t207o-test-files-split-by-topic-design.md).
- [x] **T207p** Code past 1,000 in the root workspace, split by capability. Most are one large
      `impl`, split by what its methods do as T207d and T207i did: core `config.rs`,
      `generate.rs` (`impl Generator`, 869), `blueprints/plan.rs`, `index/packages.rs`
      (`impl PackageIndex`, 953), `services.rs` and `sites.rs`; daemon `api/apply.rs`,
      `api/rpc.rs`, `elevation.rs` (1,478; `impl Elevation`, 1,268), `extensions.rs`,
      `runtimes.rs` (`impl Runtimes`, 1,000), `sites.rs`, `uninstall.rs`,
      `uninstall/inventory.rs` and `updates.rs`; platform `process.rs`, `windows/process.rs` and
      `windows/restricted.rs`; proto `privileged.rs`, `rpc.rs` (`pub mod method`, 942) and
      `service.rs`; cli `render/service.rs`. That is 22 files, with one commit per crate. The
      three proto files are in the privileged helper, so `helper-lock.sh --check` decides
      whether a bump is due. Platform's `process.rs` is checked on Linux through WSL and with
      macOS clippy, as `before-pushing` says. Design:
      [2026-10-18-t207p-code-split-by-capability-design.md](../specs/2026-10-18-t207p-code-split-by-capability-design.md).
- [x] **T207q** Outside the root workspace, ten files, each checked by its own project. The
      desktop's Rust: `db/drivers/{clickhouse,mssql,postgres}.rs` move `table_data` and
      `build_where` into the engine's folder, and `sync/session.rs` splits `impl SyncState`
      (1,082). The desktop's frontend: `db/db.css` and `shell/App.css` split by the component
      they style, verified with `npm run screenshots -- --check`, and `db/i18n/en.ts` and
      `vi.ts` split by screen with the same keys in both. The server: `native/src/accounts.rs`
      and `worker/src/account.ts` (1,083), verified by `server/conformance`. Design:
      [2026-10-18-t207q-files-outside-the-root-workspace-design.md](../specs/2026-10-18-t207q-files-outside-the-root-workspace-design.md).

Since T207q the size baseline is empty again: no source file in the repository is over 1,000
lines, and `node scripts/check-file-sizes.mjs` holds every file to the limit itself.
