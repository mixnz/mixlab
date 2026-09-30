---
status: implemented
date: 2026-09-09
task: T106
---

# T106 — One updater

Roadmap task [T106](../roadmap/phase-12-one-product.md), on
[the desktop client design](2026-09-08-the-desktop-client-in-this-repository-design.md)'s D9 and on
[T105](2026-09-09-t105-the-window-in-every-installer-design.md). 2026-09-09.

Two updaters ship in this repository today. One is MixEngine's: a signed `latest.json`, a payload
archive of the release's binaries, a write probe, a smoke test, a rename-then-place swap and a
rollback — `crates/mixengine-core/src/updates/`, and it has replaced `mix`, `mixengined` and
`mixengine-shim` since T88. The other is the standalone client's: `tauri-plugin-updater`, a second minisign key
compiled into `tauri.conf.json`, a feed hosted at `mixnz/<old>`, and a plugin that replaces the
*running bundle* on macOS and Linux. T104 stopped the second one being *called* — `SELF_UPDATE_FEED`
is `false`, and the comment there says why: a window on `0.0.6` offered the standalone client `0.0.33` would have
overwritten itself with standalone client. It stayed wired for this task.

This task removes it, and finishes the first one. What is left after it is one feed, one key, one
payload, one swap, and a window that comes back on the new version by itself.

## What is already true

Three quarters of D9 landed with T88 and T105, and this document is smaller for it. Written down so
nothing below is built twice:

- **`updates::apply::swap` already adds nothing.** Rule 2 of that function — *a name this install
  does not have is skipped and reported as kept* — is D9's "a headless install stays headless",
  written in 2026-09-04 and tested since. There is no change to make there.
- **The Windows and Linux payloads already carry the window.** `MIX_BINARIES` gained `mixlab` at
  T105, and `build-tarball.sh` and `windows/build.sh` build their payloads from that array.
- **`feed-check.sh` already asserts five names.** It reads `MIX_BINARIES` and demands set equality
  against every artifact's `provides`.
- **The Update pane already talks to the daemon.** `UpdatesSection.tsx` calls
  `update.status | check | decide | apply` and nothing else.

What is *not* true, and is this task's work: the macOS payload carries four binaries and no bundle —
T105 left a comment in `packaging/macos/build.sh` saying so and naming this task — the plugin is
still in the manifest, the capability list and the configuration, and nothing anywhere relaunches
the window after its executable has been replaced underneath it.

## D1 — The second updater leaves

Removed, in one commit, with nothing left behind:

| What | Where |
| --- | --- |
| `tauri-plugin-updater`, `tauri-plugin-process` | `apps/desktop/src-tauri/Cargo.toml`, `lib.rs` |
| `@tauri-apps/plugin-updater`, `@tauri-apps/plugin-process` | `apps/desktop/package.json` and its lock |
| `plugins.updater` — the standalone client's public key, `mixnz/<old>`'s endpoint, `installMode` | `tauri.conf.json` |
| `bundle.createUpdaterArtifacts` | `tauri.conf.json` — it configures a plugin that is gone |
| `updater:default`, `process:allow-restart` | `capabilities/default.json` |

`update-notes.yml`, the standalone client's `latest.json` and its `.sig` files need no removal here: D12 said in
as many words that those workflows are not moved, and none of them is in this repository. What they
were is a thing `mixnz/<old>` did, and D13 archives that repository when M12 ships — this is the
release that lets it.

**The key is the point.** `tauri.conf.json` carries a base64 minisign public key that is not
`mixengine_core::updates::PUBLIC_KEY` and is not `packaging/updates.pub`. Three keys are pinned by
this product on purpose and each is argued in `crates/mixengine-core/src/updates.rs`; a fourth,
belonging to a repository that is about to be archived, is not one of them. It goes with the plugin
that read it.

## D2 — Restarting is this crate's own, and it is one function

`tauri-plugin-process` was in the tree for `relaunch()`, which had two callers: the update install
and the ErrorBoundary's *Restart app* button. Both are kept; what changes is who implements them.

A new `apps/desktop/src-tauri/src/relaunch.rs`:

```rust
/// Where this window was started from, read once and cached: the executable to spawn, and the
/// thing an installer placed — the same path, except on macOS where it is the bundle around it.
pub struct Origin { pub executable: PathBuf, pub root: PathBuf }

pub fn origin() -> Option<&'static Origin>;    // read once, at startup
pub fn taking_over() -> bool;                  // read and remove MIXLAB_RELAUNCH
pub fn wait_for_predecessor(identifier: &str); // until the endpoint goes quiet
#[tauri::command] pub fn relaunch_app(app: AppHandle) -> Result<(), AppError>;
```

`root` comes from a second per-OS function beside D6's, for the same reason D6's exists:

```rust
/// The thing an installer placed, given the executable inside it.
///
/// `…/MixLab.app` for `…/MixLab.app/Contents/MacOS/mixlab` on macOS; the executable itself on
/// Windows and Linux, and on a macOS build that is not in a bundle at all — `cargo tauri dev`.
pub fn application_root(executable: &Path) -> PathBuf;
```

Three things in it are not obvious, and each is a bug that would otherwise ship.

**The executable's path is read once, at startup, and never again.** On Linux `std::env::current_exe`
reads `/proc/self/exe`, which follows the *inode*, not the name. The swap this task exists to finish
does `rename(mixlab → mixlab.old)` and then writes a new `mixlab`; a running window that asked
afterwards would be told its own path is `…/mixlab.old`, and a relaunch of that path is a relaunch of
the version the user just replaced — with no error anywhere, and a Settings pane that goes on
reporting the old version. So `origin()` is a `OnceLock` filled on the first line of `run()`, before
any update can have happened, and it is what both the identity check (D3) and the spawn use.

`root` is derived from `executable` at that same moment, so the pair can never describe two
different installs.

**A relaunched copy must not hand its start back to the copy it is replacing.** `run()`'s second act
is `launch::forward`, which finds the still-running predecessor, hands it a bare opening, and exits —
so a naive spawn-then-exit produces no window at all. The parent therefore sets `MIXLAB_RELAUNCH` on
the child, and the child, seeing it, skips `forward` and instead waits for the predecessor's endpoint
to go quiet before carrying on as the only copy. That wait is `instance::listening`, a probe that
opens the transport and closes it without writing a line: the listener's `answer` reads EOF, replies
`no`, and nothing is brought to front and no tab is opened.

The wait is bounded — 15 seconds, polled every 100 ms. A predecessor that has not gone by then is a
process that is stuck, and the honest answer is to open the window anyway: `instance::serve` will
find the endpoint taken, say so on stderr, and this copy runs without the single-instance listener.
That is exactly the state a race between two ordinary starts already produces, and it is a window on
the new version rather than no window at all.

**The variable is read and removed on the first line.** Left in the environment it would be inherited
by every terminal tab, every `mysqldump`, and — the one that matters — by the *next* relaunch's child,
which would then skip `forward` for ever. It is taken the way `Opening::from_process` takes the
handoff credential, and for the same reason.

`relaunch_app` spawns `origin().executable` — through `platform::hide_console`, the crate's one
answer to a child opening a console of its own — and only then calls `app.exit(0)`, so
`RunEvent::Exit` fires and `launch::stop` removes the socket file the child is waiting to see go. A
spawn that failed is an error returned to the pane with the window still open; the exit is never
reached before the child exists.

**macOS needs no `open(1)`.** Launching `MixLab.app/Contents/MacOS/mixlab` directly is what `open`
itself ends up doing; `NSBundle.mainBundle` is derived from the executable's path, so the bundle
context, the icon and the activation policy are the same either way. One code path on three systems.

## D3 — Who decides that the window was replaced

`update.apply` answers `UpdateApplied`, which already carries `directory`, `replaced` and `kept`.
A new command reads one and answers what to do about it:

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Relaunch { Relaunching, NotReplaced, Elsewhere }

#[tauri::command]
pub fn update_relaunch(app: AppHandle, directory: String, replaced: Vec<String>) -> Relaunch;
```

**Two fields and not `UpdateApplied` itself.** The typed answer is already in the front end, out of
`bindings/`; taking the whole struct here would mean a dependency on `mixengine-proto` for a shape
this function reads two fields of, and ADR 0027's rule 4 is a list to add to on purpose rather than
by convenience.

The rule, in order:

1. `replaced` does not contain this crate's own `CARGO_PKG_NAME` → **`NotReplaced`**.
2. It does, but `Path::new(&applied.directory).join(origin.root.file_name())`, canonicalized, is not
   `origin.root` canonicalized → **`Elsewhere`**.
3. Otherwise → spawn, exit, **`Relaunching`**.

**The name comes from `env!("CARGO_PKG_NAME")` and the file name comes off the disk.** Not a constant:
`cargo` names the executable after `[package].name` and `packaging/common.sh`'s `MIX_WINDOW` is held
to that same string by `crates/mixengine-core/tests/packaging.rs`, so the payload's `provides` key and
this window's own package name cannot disagree. `origin.root.file_name()` is `MixLab.app`,
`mixlab.exe` or `mixlab` because that is what an installer put on disk, which means D6's per-OS
naming is never spelled twice.

**Canonicalized on both sides**, because the daemon's `directory` is the parent of whatever path
`mixengined` resolved its own executable to and this window's is whatever it was started with: a
symlink on one side and not the other would answer `Elsewhere` for the install that was just
replaced. A path that will not canonicalize — it was renamed out from under us, which on this code
path it was not — falls back to comparing as written.

**`NotReplaced` is a sentence, not a silence.** It is the state a macOS `.pkg` install is always in:
the four binaries are in `/usr/local/bin` and `MixLab.app` is in `/Applications`, so the swap finds no
`mixlab` beside `mixengined` and reports it as `kept` — correctly, by rule 2, which is what stops an
update *adding* a window to a headless server. The daemon is then new and the window is old, and the
only thing worse than that happening is it happening without saying so. The pane says the window was
not replaced and that the installer is what replaces it, which is also D9's sentence for an install
from before this release.

**`Elsewhere` is the two-installs case** — a portable archive extracted beside an NSIS install, both
pointed at the same home. The window that was replaced is not this one; this one is untouched and
restarting it would prove nothing. Silence is right there, and it is a distinct answer rather than a
folded-in `NotReplaced` so that a log line can tell them apart.

## D4 — The macOS payload carries the bundle

`packaging/macos/build.sh` builds its payload and its headless archive **from one directory**,
`$MIX_OUT/tar`, which was correct while they held the same four files and becomes the bug the moment
one of them gains a fifth. It gains two roots, as `build-tarball.sh` already has:

- `$MIX_OUT/tar/mixengine/` — the four `lipo`'d binaries, and `MixLab.app` copied in with `ditto`.
- `$MIX_OUT/tar-headless/mixengine/` — the four, and nothing else.

`ditto` and not `cp -R`, for `macos/build.sh`'s own stated reason: it is what preserves an
application bundle on this system. The payload's read-back check gains `mixengine/$MIX_WINDOW_APP/`;
the headless archive's *negative* check — "an archive that quietly grew a webview is the one failure
this artifact exists to prevent" — is already written and is what would have caught the shared
directory.

The payload grows by the bundle, which T105a measured at 33 MB of AppImage for the same content.
That is what the Windows and Linux payloads have carried since T105, and it is the price of
`mix self-update` being able to replace the window at all.

## D5 — A `provides` entry may name a directory

`mixengine_core::install::present` refuses an artifact whose `provides` names something that is not
`is_file()`. A macOS bundle is a directory, so a payload carrying one would be refused by `stage`
before anything is downloaded — taking the other four binaries with it. macOS self-update would
break entirely, and the message would name the window.

The check becomes: **the path exists, and if it is a directory it is not empty.**

The teeth are kept. What `present` is there to catch is "an archive repacked without a binary the
index still lists", and a missing path still fails; an entry that resolves to an empty directory —
the shape a bundle copied by something that did not follow it would take — fails too. What is given
up is the ability to say "this name is a file", which was never the property being asserted: the one
entry whose *shape* matters is the smoke-test executable, and that one is proved by being run.

## D6 — The swap knows the fifth one is an application

`swap` resolves each `provides` key to a file with `binary_name`, which appends `EXE_SUFFIX` and
nothing else. On macOS the window's on-disk name is `MixLab.app`, which no suffix produces.

Two constants in `updates::apply`, held to `packaging/common.sh` by
`crates/mixengine-core/tests/packaging.rs` the way `MIX_BINARIES` already is:

```rust
pub const WINDOW: &str = "mixlab";          // packaging/common.sh's MIX_WINDOW
pub const WINDOW_BUNDLE: &str = "MixLab.app"; // packaging/common.sh's MIX_WINDOW_APP
```

and one function in `mixengine_platform::install`, because *what an application is called on disk*
is exactly the kind of per-OS fact `CLAUDE.md` says may not be a `cfg!` in `mixengine-core`:

```rust
/// `mixlab.exe` on Windows, `mixlab` on Linux, `MixLab.app` on macOS.
pub fn application_file_name(executable: &str, bundle: &str) -> String;
```

`swap` then resolves `WINDOW` through that and every other name through `binary_name`, and three of
its helpers learn that a target may be a tree:

- `replace` copies a directory recursively when the staged source is one, and `std::fs::copy` per
  file carries the mode across, so the bundle's executable arrives executable; `make_executable` is
  applied to files only.
- `unwind` and the failure path inside `replace` remove a tree rather than a file before renaming
  the `.old` back, so a copy that fails half way through a bundle leaves nothing half-written.
- `discard_old` removes a tree, and goes on reporting a count rather than an error — on Windows the
  `.old` of a running window cannot be unlinked, and the next daemon start collects it.

**This is tested on all three operating systems and not only on macOS.** `replace` branches on
`source.is_dir()` and never on the target platform, so a test whose staged payload holds a
*directory* under an ordinary binary name exercises the whole tree path everywhere. What is tested
per-OS is `application_file_name`, in the crate where the per-OS knowledge lives.

## D7 — `feed.sh` describes the bundle

`feed.sh` builds `provides` by walking the archive's entries and keeping the ones directly under
`mixengine/` that are not directories. A bundle is neither: `mixengine/MixLab.app/` is skipped by the
directory arm and `mixengine/MixLab.app/Contents/…` by the nested arm, so a macOS payload carrying one
would produce a four-entry `provides` and a feed that promises no window.

One arm is added, keyed on `$MIX_WINDOW_APP` — the script already sources `common.sh` — and it emits
`$MIX_WINDOW=mixengine/$MIX_WINDOW_APP` once, however many entries the bundle has inside it. Nothing
sniffs the operating system: the rule is *this archive contains that bundle*, and only the macOS
payload ever does.

**The feed's schema does not move.** `provides` is a map and always was; a value that names a
directory is a value. A client built before this release reads the row, looks for `mixlab` beside
`mixengined`, does not find it, and reports it as kept — which is rule 2 doing exactly what it is
for. Backwards compatible in the direction that matters, which is old clients reading new feeds.

## D8 — The shell stops advertising an update of its own

`src/shell/update.ts` is a 380-line hook driving a plugin that no longer exists, behind a constant
that has been `false` since T104. With the plugin gone it is not merely unused, it is unimplementable.

What is kept is what a person opening MixLab's Settings actually needs: **the running version, and
where updates come from.** The Updates pane becomes that — a version, a sentence naming MixEngine's
own updater, and a link to the release page — and everything that existed to drive a download goes
with the plugin: the toast in the corner, the dot on the brand button, skip/unskip, the progress
bar, the three `localStorage` keys and the i18n strings that named them.

**It becomes a signpost rather than nothing**, because the pane a MixLab user opens first is the
shell's, and the pane that updates MixEngine is inside a tab. Removing it entirely would be honest
about the code and dishonest about the product.

The MixEngine module's own Updates section — `update.status | check | decide | apply` — is unchanged
except for D3's call after `apply` returns.

## D9 — What is checked, and where

| Property | Where |
| --- | --- |
| The feed names the bundle on macOS | `.github/scripts/test-feed.sh`, in `lint` on every run |
| Every artifact's `provides` names all five | `packaging/feed-check.sh`, extended with a macOS leg |
| The macOS payload holds the bundle, the headless archive does not | `packaging/macos/build.sh`'s own read-back |
| `WINDOW`, `WINDOW_BUNDLE` match `common.sh` | `crates/mixengine-core/tests/packaging.rs` |
| A directory target is renamed, copied, unwound and discarded as a tree | `updates::apply`'s unit tests |
| `provides` may name a directory, and an empty one is still refused | `mixengine_core::install`'s tests |
| `application_file_name` and `application_root` per OS | `mixengine-platform`'s own tests |
| A relaunched copy waits for its predecessor instead of forwarding to it | `relaunch.rs`'s tests over `instance::listening` on a real endpoint |
| Nothing imports the two plugins any more | `npm run build`, `cargo clippy`, `tests/layering.rs` |

`test-feed.sh` is the one that runs unattended, and it is the one that matters: it exercises the
script a release runs and nothing else does, in the `lint` job, on every CI run.

## Error handling

- **A payload whose `provides` has no window** — refused by nothing, and that is deliberate. D9's
  error-handling section imagined the desktop application refusing such a feed; it cannot, because it
  is not the thing that reads the feed, and a release that built no window is a release whose four
  binaries a server should still be able to install. `feed-check.sh` and `test-feed.sh` are what keep
  it from being a release-day discovery, which is what that paragraph was really asking for.
- **A swap that cannot copy the bundle** — the existing rollback, extended to trees: every rename
  already made is undone, the `.old` comes back under its own name, and `update.apply` returns the
  failure with the services started again.
- **A relaunch that cannot spawn** — reported to the pane and the window stays open on the old
  version, which is a window a person can read a message in. Never `app.exit(0)` before the spawn has
  succeeded.
- **A predecessor that will not go away** — 15 seconds, then open anyway without the single-instance
  listener, with a line on stderr. See D2.

## What this task does not do

- **`mix self-update` from a terminal does not tell a running window.** It replaces `mixlab` beside
  the other four and the window goes on running the image it started from until somebody closes it.
  Telling it would mean the daemon pushing an event to a client that has no subscription to the
  update stream, which is T107's neighbourhood and not this one's.
- **Nothing learns where the window is.** `NotReplaced` is stated rather than diagnosed: the pane
  does not say *your window is in `/Applications` and your binaries are in `/usr/local/bin`*, because
  the function that could say so is T107's — "where the daemon is, and where the window is, are both
  the platform's to answer".
- **The release notes sentence is a person's.** D9 requires an install from before this release to be
  told that the window arrives by installer. The notes are generated from commit subjects before the
  draft exists (`feed.sh`'s own header says why), so this is a line in the release checklist —
  `docs/operations/build-and-release.md`, step 4 — and not a line of code.
