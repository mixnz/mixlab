# Phase 33 — A line shows its newest, and updates in place

*Goal: the available list is one row per line with every patch still reachable, and an installed
version moves to the newest patch of its line on a click, carrying its sites, pools, instances,
extension choices and pins, with the old version removed unless the person keeps it.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Design: [2026-09-29-t193-a-line-shows-its-newest-and-updates-in-place-design.md](../specs/2026-09-29-t193-a-line-shows-its-newest-and-updates-in-place-design.md).

---

- [x] **T193a** `lines::line_of` and its tests, `line`, `newest_in_line` and `updates` in the proto
      and bindings, both `list_available`, the grouped `mix runtime available` and
      `mix package available`, and MixLab's grouped *Available* list.
- [x] **T193b** `runtime.upgrade_plan` and `runtime.upgrade`: extension choices, pool settings and state,
      `web-app` pools that satisfy their `requires`, sites, default and pins moved in one
      transaction, the old version kept for what still needs it; `mix runtime upgrade`, and
      MixLab's *Update* dialog on runtime rows.
- [x] **T193c** `package.upgrade_plan` and `package.upgrade`: the Linux port grant asked for
      before a front end stops, instances moved one at a time and moved back on a failed start,
      `mix package upgrade`, and the same dialog on package rows. **(P)**
- [x] **T193d** `runtime-versions.md`, `services.md`, `client-surface.md`, the changelog lines, and
      the design flipped to `implemented`.

**Milestone M33**: on Windows, PHP 8.4.24 serving a site updates to 8.4.25 from MixLab, `phpinfo()`
through the site says 8.4.25, its extension choices survive and 8.4.24 is gone; a MariaDB 11.4
instance updates within its line and its databases are readable.
