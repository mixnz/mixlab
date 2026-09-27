---
status: implemented
date: 2026-09-26
task: T187
---

# T187 — MixLab updates itself

Roadmap task [T187](../roadmap/phase-31-mixlab-updates-itself.md), on
[ADR 0056](../decisions/0056-mixlab-stands-without-mixengine.md). 2026-09-26.

ADR 0056 decided that MixLab stands without MixEngine, that an install carrying the window is
updated by MixLab's own updater, and that MixEngine never reads the feed or installs a release
unless someone asks. This document is how.

## What is already true

Listed so nothing below is built twice, and so that nothing already true is quietly broken:

- **The feed and its key.** `latest.json` at
  `https://github.com/mixnz/mixlab/releases/latest/download/latest.json`, with `latest.json.minisig`
  beside it, signed with the key committed as `packaging/updates.pub`. Schema `1`. `artifacts` holds
  one payload per machine, bound by SHA-256, and `installers` holds one `.pkg`, `.deb` or `.rpm` per
  machine and flavour (`window` or `headless`; a row with no flavour is the window's). A refetched
  document older than the cached one (`generated_at`) is refused. See
  `crates/mixengine-core/src/updates/feed.rs` and `docs/features/updates.md`.
- **What is published since v0.0.8.** Windows: a zip payload, swapped in place into the per-user
  install. macOS: a `.pkg`, handed to Installer.app (ADR 0050). Linux: a `.deb` and an `.rpm`,
  handed to the system's installer (ADR 0053). The archives, the tarball and the AppImage are no
  longer published. **So an in-place swap happens only on Windows**; everywhere else the updater
  downloads, verifies and hands over.
- **The window can already relaunch itself.** `apps/desktop/src-tauri/src/relaunch.rs` (T106 D2)
  reads its own path at startup, before any swap, and a relaunched copy waits for its predecessor
  instead of forwarding to it.
- **The window can already reach a daemon, when there is one.** The `mixengine` module's Rust half
  (`src/modules/mixengine/`) finds the endpoint, speaks JSON-RPC, and starts `mixengined`.
  `daemon.shutdown` answers `DaemonShutdown`, whose `services.reached` lists what it stopped.
- **A daemon restart restores only part of what ran.** A front end comes back by itself (T167e);
  anything else that was running comes back only if something starts it. That is why the daemon's
  own update writes a `restore` record, and why this updater has to keep its own (D6).
- **MixLab's reqwest, sha2 and flate2 are already in its lock file.** `minisign-verify` and `zip`
  are not.

## D1 — Where the code lives

```
apps/desktop/src-tauri/src/updater/
  mod.rs         the commands the Updates pane calls, and the background check
  feed.rs        fetch, verify, parse, the rollback check, the cache
  placement.rs   what kind of install this window is (D3)
  stage.rs       download, checksum, unpack, smoke test (Windows)
  swap.rs        rename, copy, roll back, clean up `.old` (Windows)
  handover.rs    the `.pkg` / `.deb` / `.rpm` path (macOS, Linux)
  lock.rs        the one lock both updaters take (D7)
  records.rs     what MixLab remembers between runs (D8)
apps/desktop/src/shell/update.ts                 the hook the pane and the indicator read
apps/desktop/src/shell/components/SettingsModal/UpdateSection.tsx
```

**Nothing under `src/updater/` names a `mixengine_*` crate**, as ADR 0056 rule 3 requires. The one
thing the updater needs from MixEngine is to stop and start a running daemon, and it asks the
`mixengine` module for that through three functions the module exports for the purpose:

```rust
// src/modules/mixengine/for_update.rs
/// Whether a daemon answers on this machine's endpoint. Never starts one.
pub async fn running() -> bool;
/// `daemon.shutdown`, and the services it reports stopping.
pub async fn stop() -> Result<Vec<String>, AppError>;
/// Start `mixengined` from `directory`, wait until it answers, then `service.start` each of `services`.
pub async fn start_again(directory: &Path, services: &[String]) -> Result<(), AppError>;
```

This is MixLab asking its optional module a question over the public API, in the same way the
module asks the daemon. It is not MixLab depending on MixEngine: with no daemon, `running()`
answers `false` and the other two are never called. The module's backend is registered in every
build whether or not the module is visible (T108 D8), so hiding the MixEngine tab does not remove
this path.

**Enforced by a test**, `tests/layering.rs`: no file under `src/updater/` may contain
`mixengine_`. It is the same kind of check the file already does for `Cargo.toml`.

## D2 — The feed, read a second time

`feed.rs` reads the same document as `mixengine_core::updates::feed`, with its own code:

- **The same key, compiled in** as `updater::PUBLIC_KEY`. A test reads `packaging/updates.pub` with
  `include_str!` and asserts equality, as `crates/mixengine-core/src/updates.rs` already does for
  its copy. Two copies of one key, each held to the same file, cannot drift apart.
- **The signature is verified before the bytes are parsed**, with `minisign-verify`. The minisign
  global signature covers the trusted comment, so it is verified too.
- **A struct of only the fields MixLab reads**: `schema`, `generated_at`, `version`,
  `published_at`, `notes`, `notes_url`, and in `artifacts` and `installers` the fields D4 and D5
  use. Not `deny_unknown_fields`, for the reason the core's feed gives: old readers must accept new
  documents. `schema != 1` means *this release cannot be read by this build*, and the pane says
  exactly that, with the link to the download page.
- **The rollback check and the cache.** The last verified document and its signature are stored in
  MixLab's app data directory, under `updates/`. They are re-verified on every read, and a fetched
  document with an earlier `generated_at` is refused. Offline, the cached one stands and nothing is
  shown.
- **A shared fixture keeps the two readers honest.** One signed feed under `packaging/testdata/`,
  signed with a test key, is parsed by the core's tests and by MixLab's. A field one reader starts
  relying on that the other does not handle shows up as a failing test, not as a release-day
  surprise.

## D3 — What kind of install this is

Read from `relaunch::origin()`, which was captured before anything could be swapped:

| Machine | Answer | How it is known |
| --- | --- | --- |
| any | **Development** | a debug build, or `origin.root` inside a Cargo `target/` directory. Never updated; the pane says it is a development build |
| Windows | **Swap** | the directory holding `mixlab.exe` is writable (the write probe core uses) and holds `mixengined.exe` |
| macOS | **Installer `pkg`** | `pkgutil --file-info` names receipt `dev.mixengine.cli` for the bundle's executable |
| Linux | **Installer `deb` / `rpm`** | `dpkg -S` or `rpm -qf` names the package that owns the executable |
| anything else | **Elsewhere** | the pane says something else installed this copy, and links the download page |

The receipts are those `mixengine_core::updates::placement` already recognises. Each probe is one
process, spawned through `crate::platform::hide_console`, and runs once per window start.

**A Windows window without `mixengined.exe` beside it** is still a Swap install: the payload
replaces what the directory has and adds nothing (ADR 0027 rule 3's rule, kept by ADR 0056 rule 4).

## D4 — Windows: download, swap, relaunch

In this order. Each step lists what happens if it fails.

1. **Take the lock** (D7). *Taken:* the pane names the holder and stops. Nothing has changed.
2. **Download** the artifact for this machine into MixLab's cache directory,
   `updates/<version>/`, resuming a partial file, with progress reported to the pane. **Check the
   SHA-256** against the signed feed. *Failure:* the partial file is kept for a retry, and nothing
   has changed.
3. **Unpack** only the entries under `mixengine/` that `provides` names. An entry whose path
   escapes the staging directory is refused. *Failure:* nothing has changed.
4. **Smoke test** the staged `mixengined.exe --version`: it must run and print the offered version.
   This is the check that catches a payload for the wrong machine, or one Smart App Control
   refuses (`docs/features/updates.md`). The window is not run; its start is proved by step 8.
   *Failure:* refused, and nothing has changed.
5. **Ask the `mixengine` module** whether a daemon is running. If one is, write the in-progress
   record (D8) **first**, then call `stop()` and add the services it reports to the record.
   *Stop fails:* the daemon is still running, and the update is refused with the daemon's own error.
6. **Swap** every name in `provides` that exists in the install directory: rename `x` to `x.old`,
   copy the staged `x` into place. This is the same rename-then-place that
   `mixengine_core::updates::apply::swap` does, so it works while the files are in use on Windows.
   The installed helper is never touched; the copy beside the program is in `provides` and is
   replaced like any other file. *Failure:* every rename already made is undone, and if a daemon
   was stopped, `start_again` runs from the **old** files. The pane shows the error. The record is
   cleared.
7. **If a daemon was stopped**, `start_again(directory, services)`. *Failure:* the files stay new.
   The pane says MixEngine did not come back, and the MixEngine tab's *Start* button is the way on.
   The record stays, so the next window start tries again (D8).
8. **Relaunch the window** through `relaunch.rs`. The record is cleared on the relaunched side,
   once it has started.

**`.old` files** are removed at the next MixLab start and at the next daemon start (the daemon
already does this for its own). Neither depends on the other.

## D5 — macOS and Linux: hand over to the installer

1. **Download** the `installers` row for this machine and flavour `window`, and check its SHA-256.
2. **Open it.** macOS: `open` hands it to Installer.app. Linux: `xdg-open`, when there is a desktop
   session. Either way, the pane shows the command that installs it by hand (`sudo installer -pkg …`,
   `sudo apt install ./…`, `sudo dnf install ./…`), because a software centre that will not install
   a local package is common. Nothing in MixLab elevates.
3. **Watch the version on disk**, every 3 seconds while the pane is visible and the window is
   focused (the MixEngine pane's T88f rule):
   macOS reads `CFBundleShortVersionString` from the bundle's `Info.plist`. Linux reads the package
   database (`dpkg-query -W -f '${Version}'` or `rpm -q --qf '%{VERSION}'`).
4. **When it reads the new version, the pane offers *Finish*.** Finish does D4 steps 5, 7 and 8:
   if a daemon is running it is still running the **old** image, so it is stopped, started again
   from the new files, and its services restored. Then the window relaunches.

Cancelling the installer loses nothing: nothing was stopped. The pane offers *Open again* and
*Back*, as the MixEngine pane does today.

## D6 — When MixLab looks, and what it never does

- **It checks at window start**, 30 seconds after the window is drawn, so a start is never slowed
  by the network. **Then every 24 hours** while the window is open.
- **Settings → Updates has a switch**, *Check for updates automatically*, on by default. Off means
  MixLab makes no request until someone presses *Check now*.
- **A failed check is silent.** An offline machine sees no error. *Check now* is the exception: a
  person who pressed it gets told why it failed.
- **It never downloads or installs without a click.** The check only fetches the feed.
- **Skip this version** and **Remind me later** are kept, in MixLab's store and not the daemon's.
  *Later* hides the offer until the next window start.

## D7 — One lock for both updaters

`mix self-update` holds `<home>/run/self-update.lock` today, and the daemon swaps while it holds
it. MixLab does not know about homes, and a lock belongs to the thing being changed: **both move to
`<install directory>/update.lock`.** That directory is the one being swapped, and it is writable
exactly when a swap is possible.

The file is a contract between two programs that share no code, so it is written down:

- **Unix:** `flock(LOCK_EX | LOCK_NB)` on the open file.
- **Windows:** the file opened for read and write with share mode `FILE_SHARE_READ` only. A second
  opener for writing gets `ERROR_SHARING_VIOLATION`.
- **Contents:** the holder's pid and a newline, nothing else, so the other one can say who holds it.
  `mixengine_platform::lock` parses exactly that, and a second field would make it read no pid.

That is what `mixengine_platform::lock` already does, so the CLI's change is its path and nothing
else. MixLab's `lock.rs` implements the same contract. **A test in the desktop crate takes the lock
with `mixengine_platform::lock` (a dependency the crate already has, and allowed in tests) and
asserts that `updater::lock` is refused, and the other way round.** This test is how the contract
is kept.

A `.pkg`, `.deb` or `.rpm` install is not swapped by either updater, so neither takes the lock
there.

## D8 — What MixLab remembers

In MixLab's app data directory, under `updates/`. Nothing goes in the MixEngine home.

| File | What | Cleared |
| --- | --- | --- |
| `latest.json`, `latest.json.minisig` | the last verified feed (D2) | replaced by a newer verified one |
| `decision.json` | `{ skipped: version? }` | on the next release |
| `in-progress.json` | `{ from, to, directory, daemon_was_running, services[] }` | when the relaunched window has started, or a rollback has finished |

**`in-progress.json` exists for a window that dies half way.** At start, MixLab reads it. If the
files on disk are the new version and a daemon was stopped, it runs `start_again` and clears the
record. If they are the old version, the swap was rolled back or never happened, and it only
restarts what was stopped. In both cases the pane says what happened.

## D9 — The pane and the indicator

*Changed by [T188](2026-09-27-t188-an-update-offered-in-the-corner-design.md): the strip across the
top is replaced by a panel in the corner, and the one-click Install is two clicks, Download and then
Install and restart (or Open installer).*

**Settings → Updates** (the shell's pane, drawn whatever modules are visible):

- The running version, and when the feed was last read.
- *Check now*, and the automatic-check switch.
- **An offer**: the version, the download size, the release notes (and a link when `notes_url` is
  set), and when a daemon is running, *MixEngine and N services will restart*. Buttons: *Install*
  (Windows) or *Download and open the installer* (macOS, Linux), *Remind me later*, *Skip this
  version*.
- **In progress**: download progress, then *Installing…*. Then the window relaunches.
- **The D5 states**: installer open, with the command; *Finish*; *Open again*; *Back*.
- **Development** and **Elsewhere** (D3): one sentence, and the download page button that is there
  today.
- The privacy policy and log folder rows stay where they are.

**The indicator**: a dot on the Settings button in the shell while an offer is pending and not
skipped, and a single toast when a new offer first appears (*MixLab 0.0.10 is available*, with
*View* opening the pane). The toast is shown once per version.

Every string goes through `t()`, in `en.ts` and `vi.ts`, and is written with the
`writing-user-facing-text` skill.

## D10 — What leaves MixEngine

- **The daemon's unprompted check** (ADR 0056 rule 8). `crate::updates::start` and its call in
  `main.rs` are removed. `update.check` stays for `mix self-update --check` and for any client.
- **`[updates] enabled` and `check_seconds`** stay in the config schema, are read and ignored, and
  `docs/guide/` says they no longer do anything. `config::Updates` is `deny_unknown_fields`, so
  removing them would make every existing `config.toml` that sets them fail to load.
- **`mix status`** needs no change: it shows what the last check found, and every check is now one
  somebody asked for.
- **The MixEngine module's Updates section** (`screens/Settings/UpdatesSection.tsx`) is removed, with
  `updatesState.ts`, `relaunchAfterUpdate`, the `update_relaunch` command, and their strings.
  `docs/features/client-surface.md` item 9 says that `update.*` is for `mix` and for other clients,
  and that MixLab draws none of it (ADR 0056).
- **`mix self-update`** moves its lock (D7) and otherwise does not change. On a window install it
  still works, and the window, when next started, finds itself replaced, which is T106's behaviour
  today.

## D11 — The first release that has this

The code that updates is the code already installed. A v0.0.8 window has no MixLab updater, so
**v0.0.8 → the release carrying T187 is the last update done the old way**: through the MixEngine
tab, `mix self-update`, or the installer by hand. The release notes say so in one line. The first
update done by MixLab is from that release to the next one.

The daemon side needs no bridge: a v0.0.8 window calling `update.apply` on a new daemon still
works, because `update.apply` is unchanged.

## Verification

Pure logic, in vitest and in `cargo test` for the desktop crate:

- `feed.rs`: the shared fixture verifies and parses; a flipped byte fails before parsing; an older
  `generated_at` is refused; an unknown field is accepted; `schema: 2` is refused with its own error.
- `PUBLIC_KEY` equals `packaging/updates.pub`.
- `placement.rs`: every row of D3's table, from recorded probe answers.
- `swap.rs`, on a temporary directory: a full swap; a swap that fails on the third file puts the
  first two back; a name absent from the directory is skipped; a directory entry is swapped as a
  tree.
- `lock.rs`: the cross-program test in D7.
- `layering.rs`: nothing under `src/updater/` contains `mixengine_`.
- The pane's state function, `updateView(status)`, over each state in D9.

By hand, with `npm run dev:app` and a local feed served with a test key (the way T88 was checked),
on the paths worth walking:

1. Windows, MixEngine never started: offered, installed, relaunched on the new version. No
   `mixengined` process at any point.
2. Windows, MixEngine running with MariaDB and PHP-FPM: offered with *MixEngine and 2 services will
   restart*; after the relaunch both are running on the new daemon.
3. Windows, a swap that fails (a file held open by another process): everything back, daemon and
   services running as before.
4. Windows, `mix self-update` running while *Install* is pressed: the pane names the holder.
5. macOS `.pkg`: Installer.app opens, *Finish* appears once installed, the window relaunches.
6. The switch off: no request to `github.com` in an hour with the window open.
7. `mixengined` running for a day with no client: no request to the feed.

## D12 — ADR 0054's completion is not repeated by MixLab

The daemon completes a Windows install at start from the payload it staged in
`<home>/cache/updates/<version>/`: today that adds `mixengine-trampoline` when it is missing
(ADR 0054). MixLab stages in its own directory, so a daemon started after a MixLab swap has nothing
to complete from. **MixLab does not complete either.** Measured on the tags, 2026-09-26:

- `v0.0.7`'s `MIX_BINARIES` has no trampoline. `v0.0.8`'s has it, its NSIS installer installs it
  (`packaging/windows/mixengine.nsi:263`) and removes it (`:567`), and T185a's completion is in
  `v0.0.8`.
- MixLab's updater first runs from the release carrying T187 (D11). A Windows install reaching that
  release without the trampoline must have started at `v0.0.7`, been updated by the daemon twice,
  and had no daemon start after either update. Each of those starts would have completed it.
- **A missing trampoline breaks nothing.** `<root>/bin` falls back to a copy of the shim, as before
  T185 (`crates/mixengine-core/src/shims.rs`), and the next full install brings the trampoline.

Completing from MixLab would mean MixLab holding `COMPLETABLE` and writing the record `mix uninstall`
reads into the daemon's store: MixLab depending on MixEngine's internals, which ADR 0056 forbids,
to close a gap that is narrow and already has a fallback.
