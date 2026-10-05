# Phase 37 — The window closes its known gaps

*Goal: every daemon method a person in MixLab needs has its button, and the ones the window has no
use for say why.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Design: [2026-10-06-t199-the-window-closes-its-known-gaps-design.md](../specs/2026-10-06-t199-the-window-closes-its-known-gaps-design.md).

---

- [ ] **T199** Start/Stop and Delete in the Sites row menu, Cancel on a running job and in a
      blueprint apply, Check served on the Domains screen, Write manifest on Projects; `job.list`,
      `cert.ca_rotate` and `cert.ca_uninstall` move to `cliOnly` with their reasons. Deleting a
      shared site is refused in the window until the daemon withdraws the share on delete, which
      is a task of its own.

**M37** `node scripts/check-client-surface.mjs` reports 0 known gaps.
