---
status: implemented
date: 2026-09-09
task: T104
---

# T104 — The application is MixLab — design

**Date**: 2026-09-09
**Roadmap**: [phase 12](../roadmap/phase-12-one-product.md), task T104
**Parent design**: [the desktop client in this repository](2026-09-08-the-desktop-client-in-this-repository-design.md),
D6 and D7 · **Decision**: [ADR 0027](../decisions/0027-the-desktop-client-lives-in-this-repository.md)

## Goal

The application under `apps/desktop/` stops being the standalone client and becomes **MixLab**: MixEngine's window,
carrying MixEngine's version, with a mark of its own and a keyring namespace of its own. A person
who was using the standalone client opens MixLab for the first time and finds every saved connection, host,
environment and draft where they left it, passwords included, with the standalone client's install they may still
have untouched.

Nothing else takes the new name. `mixengined`, `mix`, `mixengine-elevate`, `mixengine-shim`,
`MIXENGINE_HOME`, the home directory, the `mixengine` keyring namespace the daemon's own credentials
live under, the installers and the release feed all keep MixEngine's — the parent design's D6, *two
names, on purpose*.

## What changes, and what does not

| | Before | After |
| --- | --- | --- |
| Product name | The standalone client | **MixLab** |
| Identifier | `io.github.haiquang9994.<old>` | `io.github.mixnz.mixlab` |
| Cargo package / executable | `<old>` | `mixlab` |
| Window title, browser tab, brand button | The standalone client | MixLab |
| Version (three files) | `0.0.33`, its own | `0.0.6`, the workspace's |
| App keyring service | `<Old>` | `MixLab` |
| MixEngine keyring service | `mixengine` | `mixengine` — untouched |
| Mark | The standalone client's three platters | MixLab's ring and ligature |
| URL scheme | `<old>://` | `<old>://` — kept, parent design D10 |
| `<old>-preview:` webview scheme | unchanged | unchanged |
| `localStorage` keys (`<old>-theme`, …) | unchanged | unchanged |
| daemon, CLI, helper, shim, home, installers | MixEngine | MixEngine |

The identifier is what the operating system keys the application-data directory and the webview
profile on, so both are new and empty on the first launch after this task. D5 below is what makes
that survivable.

## Decisions

### D1. Five fields, one file each

- `apps/desktop/src-tauri/tauri.conf.json` — `productName`, `identifier`, `app.windows[0].title`,
  `version`.
- `apps/desktop/src-tauri/Cargo.toml` — `[package] name`, `description` (it is read back into the
  installer's metadata, so it is what a user reads in Add/Remove Programs), `version`.
- `apps/desktop/package.json` — `version`. The package is already named `mixlab`.
- `apps/desktop/index.html` — `<title>`.
- `src/shell/App.tsx` — the brand button's label.

`[lib] name = "tauri_app_lib"` stays: it is a linker name with a documented reason of its own, and
`main.rs` calls through it. `Cargo.lock` is regenerated for the package rename — CI builds
`--locked`.

`--no-bundle` names the executable after the *cargo package*, not after `productName`: run
34244691840's Linux artifact is `target/*/release/<old>`, lower case, beside a `productName` of
`<Old>`. So the `build` legs' upload paths in `.github/workflows/ci.yml` move to `mixlab` and
`mixlab.exe`; the macOS leg's `*.app` glob already follows `productName` and needs no edit. The
paths are uploaded with `if-no-files-found: error`, which is what would otherwise make this a
red run rather than an empty artifact.

Three strings say the product's name to something outside this process and move with it: the
keyring service (D4), the MongoDB driver's `app_name`, which is what a DBA reads in
`currentOp`, and the `-- the standalone client structure dump` header the three structure dumps write into a file
the user keeps. Every user-visible string in `src/i18n/` and the module dictionaries moves too.

**Comments and prose are not renamed.** Some ninety mentions of the standalone client in doc comments are the
history of the code that carries them, and a sweep over them would bury this task's real diff.
`apps/desktop/CLAUDE.md` and `apps/desktop/README.md` gain the new name where they say what the
application *is*; `apps/desktop/CHANGELOG.md` keeps the standalone client's, being the standalone client's history.

### D2. The version is the workspace's, and a test holds it there

The three files the desktop build reads cannot inherit from a workspace they are excluded from, so
the check `crates/mixengine-core/tests/packaging.rs` already performs for the binary list performs
it for the version: `include_str!` the root `Cargo.toml`, `apps/desktop/package.json`,
`apps/desktop/src-tauri/tauri.conf.json` and `apps/desktop/src-tauri/Cargo.toml`, and assert all
four carry one string. `include_str!` rather than a runtime read, for the reason the file's own
header gives: a file that moved is a build error rather than a test that reads nothing and passes.
The crate already has `toml` and `serde_json`, so the four are parsed rather than pattern-matched.

Cutting a release stays a bump of the root `Cargo.toml`; a forgotten desktop file is a red test.

### D3. The mark

D6 describes MixLab's mark — a ring open at the lower right, an `ML` ligature inside it whose `L`
runs out through the gap, brand blue `#2D86E0` with the light-surface gradient
`#1A57C4 → #2D86E0 → #35CDEB` — and says the project owner hands the SVGs to this task. They have
not been handed over, and the roadmap's fallback is a mark *derived* until one is drawn. So T104
draws that description as an app tile, in the shape the existing pipeline expects: the deep-blue
tile the standalone client's icon already establishes, with the ring and the ligature stroked over it in the light
gradient. `public/logo.svg` and `public/logo-macos.svg` are replaced, and `src-tauri/icons/` is
regenerated with `npm run icons`.

**One cut, not two.** D6 asks for a heavier small-size cut because a thin ring does not survive a
16 px favicon. `make-icons.mjs` renders every size from one file and `icon.ico` is a container this
repository has no writer for, so a second cut would mean a third input, a third render and a
partial `.ico` — machinery for a placeholder. Instead the ring is stroked at 6.5 of 64 units and
the ligature at 5.5, weights chosen so the one cut holds at 16 px. When the owner's set arrives,
`make-icons.mjs` gains its third input; that is a change to the pipeline, not to this task.

### D4. The keyring service moves, and the old one is only ever read

`secrets.rs`'s `SERVICE` becomes `MixLab`, and `<Old>` stays beside it as `LEGACY_SERVICE`, which
nothing writes to and nothing deletes from. `MIXENGINE_SERVICE` is untouched: those entries belong
to the daemon and change hands with nothing, which is why a `keyringRef` is copied across as the
plain string it is (D5).

`OsStore` gains the service it addresses instead of reading a constant, so the same three
operations serve the app's namespace, MixEngine's, and — read-only — the standalone client's. The
`read_mixengine_entry` special case folds into it.

### D5. First launch imports the standalone client's data, once

On the first launch that finds an empty application-data directory, MixLab copies the standalone client's store
files and the credentials they name. Three rules from the parent design hold throughout: **the old
directory and the old keyring entries are never written or deleted**, **the import runs once**, and
**nothing from the webview's `localStorage` comes across** — theme, accent, the last tab strip and
a skipped update version are not things a person made, and they live in a webview profile keyed by
the identifier that nothing in this process can reach anyway.

**Where the standalone client's directory is.** Tauri keys the application-data directory on the bundle identifier
and on nothing else, so the standalone client's is this application's own with the name swapped:
`app_data_dir().parent()/io.github.haiquang9994.<old>`. Checked on Windows against a real install:
`%APPDATA%\io.github.haiquang9994.<old>`, beside where MixLab's will be. A parent that does not
resolve, or an old directory that is not there, ends the import with nothing done.

**Two phases, and why.** Copying the files runs **synchronously inside `setup()`**: windows are
created before `setup` and the webview cannot deliver an IPC message until the event loop runs,
which is after `build()` returns, so this is the one window in which nothing else can touch the
directory. It is a handful of small JSON files. Copying the credentials runs **on a thread of its
own**, because on macOS the first read of the standalone client's items raises a Keychain authorization dialog for
an application the Keychain has never seen, and the parent design's error handling is explicit that
an import never blocks the window from opening.

**Which files.** Every top-level regular file whose name ends in `.json` — by pattern rather than
by list, so a store a module adds later comes across too. Two exclusions: a name beginning with a
dot, which is how `.window-state.json` (the maximized flag, shell chrome, not user data) stays
behind, and the marker file itself. A file over 64 MiB is skipped and named in the log rather than
copied on the startup path. A file already present in the new directory is never overwritten.

The pattern also catches `known_hosts.json`, which is the SSH module's rather than the store
plugin's. That is wanted: it is the set of fingerprints the user accepted, and leaving it behind
would greet every saved SSH host as a stranger.

**Which accounts.** Every string under an `"id"` key at any depth of a copied file, plus the
`rest-env:<id>` form the REST module files its environments under. The union rather than a rule per
file: the store files say which accounts exist, an account that is not there reads as `NoEntry` and
costs one lookup, and a module that invents a third shape is a silent loss the day it ships
otherwise.

**Reading them.** The source reader knows both shapes the standalone client writes: one entry per account on
Windows and Linux, and on macOS a single `vault` account holding every account's secrets in one
JSON object — with a per-account entry still possible there for a connection saved before the vault
existed. It reads the vault once, falls back to the per-account entry, and writes nothing anywhere.
Each account that yields secrets is then written through `secrets::save`, which is the same path the
application uses for its own writes and therefore lands in the right shape for this platform and
keeps the process's vault cache honest.

**The marker.** `<old>-import.json` in the new directory, written at the end of the synchronous
phase with the source path, the date, the files copied and the accounts found, and rewritten by the
credential thread with what it managed. Its presence alone is what a second launch reads: the
import does not run twice, whether or not the credentials made it. A credential phase that fails
wholesale leaves connections that ask for a password again — the state the standalone client already renders — and
a marker that says so.

**The profile.** An import found is what T108 will read to start a standalone-client user on *Everything*
rather than on *MixEngine*. Until T108 exists the marker changes nothing visible; it is written in
the shape T108 needs so that task adds a read and not a migration.

### D6. The self-update feed is not dialled between here and T106

The version drops from `0.0.33` to `0.0.6` while `tauri-plugin-updater` is still wired to
`mixnz/<old>`'s `latest.json`, which offers `0.0.33`. The parent design accepted that window
because the rename moves the application out of the standalone client's install directory — but that is only true
on Windows, where NSIS installs per `productName`. On macOS and Linux the plugin replaces **the
running bundle**, so accepting the offer would write standalone client over `MixLab.app` or over the
running AppImage. That is exactly the loss D6 delayed the version change to avoid, and the rename
does not prevent it.

So T104 stops dialling the feed. The plugin and its configuration stay exactly where they are —
removing them is T106's, and a `plugins.updater` block deleted while the plugin is still registered
is a startup risk for no gain. What changes is in the frontend, which is the only thing that ever
calls `check()`:

- `UpdateStatus` gains `unavailable`, and `useUpdateCheck` starts there: no check at launch, no
  check on demand, `canCheck` false.
- `REPO` becomes `mixnz/mixengine`, so *Open the releases page* — the way out that stays — leads to
  this application's own releases rather than to another product's.
- The Update pane says one line: MixLab is updated with MixEngine.

T106 deletes the file's Tauri half and puts the daemon's updater in its place, as D9 already says.

### D7. What keeps MixEngine's name

`packaging/common.sh`, `MIX_BINARIES`, `MIX_CRATES`, the installers and the feed are T105's and
T106's and are not touched here. Neither is `mixengine-platform`'s desktop-application lookup, which
is T107's: until then `mix database open` from a terminal finds a standalone client if one is
installed, and finds nothing if none is, exactly as it does today.

## Error handling

- **No old directory, no store files, a marker already there, or a new directory that already holds
  a store file** — the import ends having done nothing, and says which of the four it was at debug
  level. None of them is a failure.
- **A file that cannot be read or written** — skipped, named in the log at warning level, the rest
  of the import carries on.
- **A file that is not JSON** — copied anyway (it matched the pattern and it is the user's), but it
  contributes no accounts and the parse failure is named.
- **The credential store unreachable, or its dialog refused** — every account fails in turn, the
  marker records how many, and each imported connection is one MixLab renders as *ask again*.
- **A single account that cannot be read** — skipped, named and counted; the others still come
  across.
- **Anything at all in the import** — never propagates out of `setup()`. The window opens.

## Testing

- `crates/mixengine-core/tests/packaging.rs` — one test that the four version fields agree (D2).
- `apps/desktop/src-tauri/src/import.rs` unit tests, no Tauri and no keyring:
  - the file pattern takes `connections.json` and `known_hosts.json`, and leaves
    `.window-state.json`, the marker, a directory and a `.log` behind;
  - the accounts of a fabricated `connections.json`, `terminal-hosts.json` and
    `rest-environments.json`, including the `rest-env:` form and an id nested two levels down;
  - a whole import over two `tempfile` directories: the files land, the marker is written, a second
    call does nothing, and a new directory that already holds a store file is not imported into.
- `apps/desktop/src-tauri/src/secrets.rs` unit tests, over the `MemoryStore` the module already has:
  the legacy reader takes secrets out of a vault, out of per-account entries, out of a vault with a
  pre-vault entry beside it, and writes to neither.
- `src/shell/update.test.ts` — `unavailable` is not pending, does not announce, and cannot be
  checked.
- By hand on Windows, on a copy of a real the standalone client directory: the window opens, the Database tab lists
  every saved connection and connects without asking for a password, and
  `%APPDATA%\io.github.haiquang9994.<old>` is byte for byte what it was.

## Out of scope

- Packaging the window (T105), the updater's own removal (T106), and where the window is (T107).
- The profile screen and the *Everything* preset (T108). This task writes the marker T108 reads.
- The owner's two-cut mark and the third input `make-icons.mjs` would need for it (D3).
- Renaming the standalone client in doc comments (D1).
- A retry for an import whose credential phase failed: the parent design says once, and a second
  pass over a directory the user has since edited is a worse failure than a password re-entered.
