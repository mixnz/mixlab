# Phase 45 — Files that fit in one read

*Goal: no source file grows past 1,500 lines, none already past it grows at all, `mix`'s
`main.rs` and `render.rs` (7,651 and 7,741 lines) become one module per command, and the daemon's
`api/rpc.rs` (5,683) keeps the routing table and hands every handler to its capability.*

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

**M45** `scripts/file-size-baseline.json` no longer names `crates/mixengine-cli/src/main.rs`,
`render.rs` or `crates/mixengine-daemon/src/api/rpc.rs`, and `bash scripts/gate.sh` is green.
