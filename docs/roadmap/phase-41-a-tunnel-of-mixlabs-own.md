# Phase 41 — A tunnel of MixLab's own

*Goal: an address on this machine can be shared on the internet from MixLab, with or without
MixEngine, and a module added to a preset reaches everyone who chose it.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Design: [2026-10-08-t203-a-tunnel-module-design.md](../specs/2026-10-08-t203-a-tunnel-module-design.md).

---

- [ ] **T203** A preset is stored as a preset: `mixlab-modules-preset` beside the module list, so a
      module added to a preset appears for everyone who chose it, and a set of somebody's own stays
      theirs. Synced, and read correctly beside a build that writes only the list.
- [ ] **T203a** The `tunnel` module: a pinned `cloudflared` found or downloaded, one quick tunnel per
      local address, its URL read from cloudflared and ended with MixLab, crash included. **(P)**
- [ ] **T203b** The tray lists running tunnels, copies a URL and stops one. **(P)**

**M41** On all three systems, a dev server on this machine opens from another network through a
URL made in MixLab's Tunnel tab, and quitting MixLab ends it.
