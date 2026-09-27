# Phase 31 — MixLab updates itself

*Goal: a person who uses MixLab and never starts MixEngine is told about a new release and installs
it from MixLab's own Settings; and MixEngine never reads the feed or installs a release unless
someone asks.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Design: [2026-09-26-t187-mixlab-updates-itself-design.md](../specs/2026-09-26-t187-mixlab-updates-itself-design.md).
Decision: [ADR 0056](../decisions/0056-mixlab-stands-without-mixengine.md).

---

- [x] **T187a** The feed, read by MixLab: `src/updater/feed.rs`, the compiled-in key held to
      `packaging/updates.pub`, the rollback check and the cache, and the shared signed fixture under
      `packaging/testdata/` read by both the core's tests and MixLab's. Spec D2.
- [x] **T187b** (P) What kind of install this is: `src/updater/placement.rs`, the five answers of
      spec D3. Spec D3.
- [x] **T187c** One lock for both updaters: `<install directory>/update.lock`, taken by
      `mix self-update` and by `src/updater/lock.rs`, with the cross-program test. Spec D7.
- [x] **T187d** Windows: download, unpack, smoke test, stop a running daemon through the
      `mixengine` module, swap, roll back, start it again, relaunch; and `in-progress.json` read at
      the next start. Spec D4, D8.
- [x] **T187e** (P) macOS and Linux: download the window's installer, open it, watch the version on
      disk, *Finish*. Spec D5.
- [x] **T187f** The pane and the indicator: Settings → Updates, the automatic-check switch, the
      check at start and every 24 hours, *Later* and *Skip*, the dot and the toast. Spec D6, D9.
- [x] **T187g** MixEngine stops looking on its own: the daemon's start check and clock removed,
      `[updates]` keys read and ignored, the MixEngine module's Updates section and
      `update_relaunch` removed, `client-surface.md` and `features/updates.md` corrected. Spec D10.
- [x] **T188** An update offered in the corner: a bottom-right panel that downloads when asked and
      installs when asked again, with `update_install` split into download and install.
      Design: [2026-09-27-t188-an-update-offered-in-the-corner-design.md](../specs/2026-09-27-t188-an-update-offered-in-the-corner-design.md).

**Built 2026-09-26, M31 not yet measured.** Every task above landed with its tests: the feed
reader, placement, lock, stage, swap, recovery, handover and the pane's state are unit-tested, the
lock is held to `mixengine_platform::lock` on Windows and (through a scratch crate in WSL) on Linux,
and `a_daemon_that_nobody_asked_does_not_read_the_feed` pins the daemon half. What no test here can
take is the window half: a debug build is a *development* install and never updates, so M31's two
Windows readings need a release build against a local feed, and the `.pkg` path needs a Mac.

**Milestone M31**: on Windows, a MixLab whose MixEngine was never started is offered the next
release and comes back on it from Settings with no `mixengined` process at any point; one with
MixEngine running comes back with the same services running; and a `mixengined` left running for a
day makes no request to the feed.
