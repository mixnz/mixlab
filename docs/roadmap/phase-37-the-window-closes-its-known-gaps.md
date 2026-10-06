# Phase 37 — The window closes its known gaps

*Goal: every daemon method a person in MixLab needs has its button, and the ones the window has no
use for say why.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Design: [2026-10-06-t199-the-window-closes-its-known-gaps-design.md](../specs/2026-10-06-t199-the-window-closes-its-known-gaps-design.md).

---

- [x] **T199** Start/Stop and Delete in the Sites row menu, Cancel on a running job and in a
      blueprint apply, Check served on the Domains screen, Write manifest on Projects; `job.list`,
      `cert.ca_rotate` and `cert.ca_uninstall` move to `cliOnly` with their reasons. Deleting a
      shared site is refused in the window until the daemon withdraws the share on delete, which
      is a task of its own.
- [x] **T199a** `site.delete` withdraws a shared site's share on `site.unshare`'s road: the
      firewall plan, the listener and the mDNS name, under the sharing lock. The window's guard
      against deleting a shared site goes with it.

**M37** `node scripts/check-client-surface.mjs` reports 0 known gaps.
