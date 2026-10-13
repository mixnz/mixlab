# Phase 45 — Files that fit in one read

*Goal: no source file grows past 1,500 lines, none already past it grows at all, and `mix`'s
7,651-line `main.rs` becomes one module per command.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Design: [2026-10-13-files-that-fit-in-one-read-design.md](../specs/2026-10-13-files-that-fit-in-one-read-design.md).

---

- [ ] **T207** `scripts/check-file-sizes.mjs` and its test: a 1,500-line limit on tracked `.rs`,
      `.ts`, `.tsx`, `.js`, `.mjs` and `.css` files, and `scripts/file-size-baseline.json` naming
      the 34 files already past it at their current length; red on a new file over the limit, on a
      baselined file that grew, and on one that shrank without `--shrink`. Run by `scripts/gate.sh`
      and CI's `lint` job; documented in `docs/standards/rust.md` and `CLAUDE.md`.
- [ ] **T207a** `crates/mixengine-cli/src/main.rs` moved into `commands/<name>.rs`, one module per
      command, with the helpers several commands share at the crate root (`output.rs`, `parse.rs`,
      `jobs.rs`, `prerequisites.rs`, `upgrades.rs`). A move and only a move: the command reference
      (`packaging/docs.sh --check`) and the unit-test count are unchanged, and `main.rs` leaves the
      baseline.
- [ ] **T207b** `crates/mixengine-cli/src/render.rs` split the same way, `render/<name>.rs` beside
      `commands/<name>.rs`. Needs a design of its own for where its tests go.

**M45** `scripts/file-size-baseline.json` no longer names `crates/mixengine-cli/src/main.rs`, and
`bash scripts/gate.sh` is green.
