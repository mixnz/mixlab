---
status: implemented
date: 2026-09-08
task:
  - T100
  - T101
  - T102
  - T103
---

# The desktop client in this repository — design

**Date**: 2026-09-08
**Decision**: [ADR 0027](../decisions/0027-the-desktop-client-lives-in-this-repository.md)
**Roadmap**: [phase 11](../roadmap/phase-11-the-desktop-app-comes-home.md) ·
[phase 12](../roadmap/phase-12-one-product.md) ·
[phase 13](../roadmap/phase-13-profiles.md)

## Goal

The standalone client — a Tauri 2 + React 19 desktop application whose fifth module is already a complete client
of MixEngine's API — moves into this repository and becomes MixEngine's desktop application,
**MixLab**. The daemon, the CLI and everything a terminal calls keep MixEngine's name; only the
window is MixLab. After
the three phases below:

- one installer per platform installs the daemon, the CLI, the helper, the shim and the window;
- one updater, MixEngine's own, keeps all of them current;
- a person who wants only MixEngine sees a window with the Dashboard and nothing of a database
  client; a person who wants the database client, the HTTP client, the terminal and the tools turns
  them on in Settings;
- the CLI-only distribution keeps existing, as a headless archive beside the installers;
- nothing the standalone client does today is lost, and nothing a standalone-client user has saved is lost.

What this design does **not** do: redesign any screen. The shell, the tab strip and every module
arrive as they are in the standalone client 0.0.33. Reshaping the window per audience is later work, and it will
have its own spec.

## What is being moved

| The standalone client | Size | Becomes |
| --- | --- | --- |
| `src/` — React frontend, five modules, shell, i18n | ~100k lines TS/CSS, 1547 vitest tests | `apps/desktop/src/` |
| `src-tauri/` — Rust: drivers, SSH, pty, REST, the MixEngine transport | ~36k lines | `apps/desktop/src-tauri/`, its own Cargo workspace |
| `AGENT.md`, `.agent/` | conventions and architecture | `apps/desktop/CLAUDE.md`, `docs/README.md` |
| `docs/specs/` | 26 design documents | `docs/specs/`, same names |
| `CHANGELOG.md` | 0.0.1 → 0.0.33 | `apps/desktop/CHANGELOG.md`, frozen as history |
| `.github/workflows/` | ci, release, audit, tool-downloads, update-notes | not moved; see D12 |
| `scripts/set-version.mjs`, `release-notes.mjs`, `fetch-bindings.mjs` | release tooling | retired; see D6, D4 |

The `mixengine` module on both halves — `src/modules/mixengine/` and
`src-tauri/src/modules/mixengine/` — is the part this repository's API already knows; the other
four modules are the toolbox ADR 0027's rule 2 describes.

## Decisions

### D1. Location and shape

```
apps/desktop/
  CLAUDE.md              the former AGENT.md, links fixed
  package.json           the frontend's own; name "mixlab"
  index.html  vite.config.ts  tsconfig*.json  eslint.config.js
  src/                   unchanged layout: shell/, core/, components/, icons/, i18n/, modules/
  src-tauri/
    Cargo.toml           [workspace] of its own; path deps on ../../../crates/{mixengine-proto,mixengine-platform}
    Cargo.lock           its own
    tauri.conf.json
    src/
```

The root `Cargo.toml` gains `exclude = ["apps/desktop/src-tauri"]`. `cargo` invoked at the root
never sees the desktop crate; `cargo` invoked in `apps/desktop/src-tauri` sees only it and the two
path dependencies. `docs/README.md`'s table gains a `desktop/` row, and `CLAUDE.md`'s workspace
layout gains `apps/desktop/`.

The git history comes along: `git subtree add --prefix=apps/desktop <<old>> master` in the
commit that lands T101, so `git log --follow` on any moved file reaches its the standalone client history. No
`filter-repo` rewrite of either side.

### D2. The dependency rule, and its test

The desktop crate may depend on `mixengine-proto` and `mixengine-platform`, and on no other crate
under `crates/`. `apps/desktop/src-tauri/tests/layering.rs` parses the crate's own `Cargo.toml` and
fails on any `path = "..."` dependency outside that pair — the root
`workspace_layering.rs` cannot see across the workspace boundary, so the guard lives on the far
side of it. One exception, and only under `[dev-dependencies]`: `mixengine-testkit`, so the
transport test can dial a real daemon. The same test refuses it anywhere else, which is the rule
the root workspace already keeps for that crate.

`mixengine-platform` is taken with `default-features = false` plus whatever feature exposes the
endpoint address and the desktop-application lookup, exactly as `mixengine-cli` takes it. What the
desktop crate wants from it is D5's endpoint and D9's *where is `mixengined`*; if either is not
`pub` today, T102 makes it so.

### D3. One toolchain

The root `rust-toolchain.toml` moves from 1.97.1 to 1.98.0 and `rust-version` with it; the desktop
crate's own pin file is deleted. rustup resolves the nearest pin walking upward, so a second file
would mean two compilers on every developer machine and every runner. The bump is its own commit
before the subtree lands, so a lint 1.98 added is fixed in this repository's code by itself and not
inside a hundred-thousand-line import.

### D4. Bindings are imported, not vendored

`src/modules/mixengine/api/types/` — the standalone client's vendored copy of `bindings/` — is deleted. A Vite and
TypeScript path alias, `@mixengine/api`, resolves to `../../bindings` and the module imports from
that. `scripts/fetch-bindings.mjs` is retired.

This is the mitigation ADR 0011 said was missing. The `bindings` CI job already fails when
`mixengine-proto` and the committed contract disagree; the `desktop` job (D12) runs `tsc` against
that contract. A type reshaped in `mixengine-proto` therefore reaches a red typecheck of the
screens that read it in the same run.

### D5. The endpoint comes from `mixengine-platform`

`src-tauri/src/modules/mixengine/endpoint.rs` carries a copy of the pipe-name fingerprint from
`crates/mixengine-platform/src/windows/ipc.rs`, with a test pinning the value, and
`transport.rs` carries its own named-pipe dial, busy retry and owner check. All of it goes: the
desktop crate resolves the home the way `mixengine-cli/src/home.rs` does — `MIXENGINE_HOME`, else
`Host::home_dirs().default_home()`, made absolute and passed through `paths::in_full` — and then
takes `ipc::Endpoint::in_run_dir` and `ipc::Connection::connect` from `mixengine-platform`.
`Connection` already retries a busy pipe and already refuses an endpoint another account holds
(`Error::EndpointNotOurs`, carrying the account), so the owner check the standalone client's T1.3 wrote is the
platform's from now on, and `windows-sys` leaves the desktop crate's manifest with it. The
fingerprint pin the desktop kept becomes a test on the platform side, where the function lives.

**`[daemon] ipc_path` is not read any more.** the standalone client honoured that key because `config.toml`'s
template offers it. `mixengine-core` parses it and nothing — not `Paths`, not the daemon — ever
uses it, and `mix` never reads it either. A client that dialled a configured path would be dialling
somewhere no daemon listens. The key stays a config-file debt for this repository, noted in
[phase 11](../roadmap/phase-11-the-desktop-app-comes-home.md), and the desktop crate
computes its endpoint from the home alone, like the CLI.

### D6. Identity and version

| | The standalone client | Desktop app |
| --- | --- | --- |
| Product name | The standalone client | **MixLab** |
| Identifier | `io.github.haiquang9994.<old>` | `io.github.mixnz.mixlab` |
| Executable | `<old>` / `<Old>.exe` / `<Old>.app` | `mixlab` / `MixLab.exe` / `MixLab.app` |
| Version | its own, `set-version` across seven files | the workspace's, from the root `Cargo.toml` |
| Icon | The standalone client's two SVGs | MixLab's own mark — it exists, and the owner hands the SVGs to T104 (below) |
| Window title | The standalone client | MixLab |
| URL scheme | `<old>://` | `<old>://`, kept — see D10 |

**The logo is decided and is not in this repository yet.** A ring open at the lower right, an
`ML` ligature inside it whose `L` runs out through the gap, stroked in a gradient from deep blue at
the top of the ring to cyan at the opening; a heavier small-size cut of the same mark for anything
under 40 px, because the thin ring does not survive a 16 px favicon; a horizontal and a vertical
wordmark with *Mix* at weight 500 and *Lab* at 700, the letters outlined so no font travels with
the file; and a single-colour version of each. Brand blue `#2D86E0`; the light-surface gradient
runs `#1A57C4 → #2D86E0 → #35CDEB` and the dark-surface one `#2D86E0 → #4AA8F5 → #5FE1F7`. T104
receives the SVGs from the project owner, places the mark at `apps/desktop/public/logo.svg` and
the padded macOS variant at `public/logo-macos.svg` where the standalone client's `npm run icons` already reads
them, uses the small cut for the 16 px and 32 px sizes, and regenerates `src-tauri/icons/`. The
proposal itself lives outside the repository and is not linked from it.

**Two names, on purpose.** MixLab is the window and nothing else. `mixengined`, `mix`,
`mixengine-elevate`, `mixengine-shim`, `MIXENGINE_HOME`, the home directory, the keyring namespace
`mixengine` the daemon's credentials live under, the installer names and the release feed all keep
MixEngine's name: a terminal user never meets the word MixLab, and a MixLab user sees MixEngine as
the thing the Dashboard manages. What takes the new name is what belonged to the standalone client — the
application identifier, the executable, the window, and the toolbox's own keyring service (D7).

The version is written in three places the desktop build reads — `package.json`,
`tauri.conf.json` and the desktop `Cargo.toml` — and none of them can inherit from a workspace they
are excluded from. `crates/mixengine-core/tests/packaging.rs`, which already holds the binary list
against `packaging/common.sh`, gains a check that all three carry `[workspace.package].version`.
Cutting a release stays a bump of the root `Cargo.toml`, and a forgotten desktop file is a red
test rather than a window reporting the previous version.

**That check lands with T104, not T101.** Through phase 11 the application keeps the standalone client's version
(`0.0.33`) and the standalone client's Tauri updater, which reads `mixnz/<old>`'s feed. Lowering the version to the
workspace's `0.0.6` while that updater is still wired would have every phase-11 build offer to
"update" itself to standalone client 0.0.33 — and on Windows that means downloading and running
The standalone client's installer over whatever directory the test build sits in. The version becomes MixEngine's in
the same task that gives the application MixEngine's name, two tasks before the updater leaves.

`apps/desktop/CHANGELOG.md` is frozen at 0.0.33 with one line at its top saying so. From the merge
on, the root `CHANGELOG.md` is the only one, under [changelog.md](../standards/changelog.md).

### D7. A standalone-client user's data comes across, once, and the old copy is never touched

A changed identifier is a changed application-data directory and a changed keyring namespace.
On the desktop application's first launch — the new directory holds no store files — it looks for
The standalone client's directory for this platform and, if present, imports:

- every `tauri-plugin-store` file at the top of that directory — today `connections.json`,
  `query-drafts.json`, `rest-environments.json`, `rest-history.json`, `terminal-hosts.json`,
  `terminal-settings.json` and `tool-usage.json`, copied by pattern rather than by list so a file
  a module adds later comes across too;
- for every saved connection, host and secret-marked environment variable named in those files,
  the credential under keyring service `<Old>` — read by account, written under service `MixLab`.
  Nothing enumerates the store; the store files say which accounts exist;
- nothing else. Logs, caches and downloaded dump tools are not user data, and neither is what the
  shell keeps in the webview's `localStorage` — theme, accent, the tab strip of the last session,
  a skipped update version. That storage belongs to WebView2's or WebKit's own profile, keyed by
  identifier, and none of it is something a person made. A standalone-client user opens the new window on the
  default theme with an empty strip and every saved connection in place.

Three rules. **The old directory and the old keyring entries are never deleted or written**: a
standalone client may still be installed and still be in use. **Import runs once**: a marker file in
the new directory records the date and the source, and a second launch does not look again.
**A `keyringRef` into MixEngine's own namespace is copied as it is**: those entries belong to the
daemon, are already under service `mixengine`, and change hands with nothing.

An import that finds the standalone client data sets the profile to *Everything* (D11); a fresh machine gets
*MixEngine*.

### D8. Packaging: a fifth binary, and the same six installers

`packaging/common.sh`'s `MIX_BINARIES` gains the desktop executable and `MIX_CRATES` gains its
crate; `packaging.rs` checks the list as before. Each `build` leg runs `npm ci && npm run build` in
`apps/desktop` and then `cargo tauri build --no-bundle` in the nested workspace — on macOS
`--bundles app`, because a webview application there is a `.app` directory rather than a file.
`stage.sh` copies the result beside the four binaries.

| Platform | What the installer places |
| --- | --- |
| Windows NSIS, per-user | the five executables in one directory; Start Menu and optional desktop shortcut to `MixLab.exe`; `<old>://` registered to it |
| Windows portable zip | one `mixengine/` directory with the five |
| macOS `.pkg` | four binaries to `/usr/local/bin`, `MixLab.app` to `/Applications` |
| Linux `.deb` / `.rpm` | five to `/usr/bin`, a `.desktop` file and icon for the window |
| Linux AppImage | five in the AppDir, unpacked into the per-version cache as today; `AppRun` with no arguments starts the window, and with any argument runs `mix` with them, so `./mixengine-<version>-linux-x86_64.AppImage status` keeps working and a double-click opens MixLab |
| **Headless** `.zip` / `.tar.gz` | the four binaries, no window — the archive the CLI-only user downloads |

WebKitGTK is a runtime dependency on Linux that the four binaries never had. The `.deb` and `.rpm`
declare it; the AppImage carries what it needs; the headless archive declares nothing, which is
its point.

### D9. One updater: MixEngine's

`tauri-plugin-updater` and `tauri-plugin-process` leave the desktop crate, and with them the Tauri
`latest.json`, the `.sig` files, the `update-notes.yml` workflow and the standalone client's minisign key. The
Update pane already calls `daemon.update_status | update_check | update_decide | update_apply`; it
stays on those.

Two things change in `core::updates`:

- The feed's `provides` lists the desktop executable (on macOS, the `.app` directory) beside the
  four, and the payload archives carry it. `feed-check.sh` asserts the fifth entry.
- **`apply` replaces the binaries the install already has and adds none.** A headless install stays
  headless; an install from before the merge stays without a window until its owner runs the
  installer. The rule is one line, it is stated in the release notes, and it is the reason a server
  never grows a WebKit dependency it cannot satisfy.

After `UpdateApplied` the daemon relaunches as it does today. The window, whose own executable was
swapped underneath it by the same rename-then-place the CLI already survives, relaunches itself:
spawn the new executable, exit. On Windows a running executable can be renamed but not overwritten,
which is what the existing swap already relies on.

`mixengined --detach` is started from the desktop executable's own directory first, then from the
platform's install location for that OS, then from `PATH` — one function in `mixengine-platform`
that packaging and the desktop crate both read, replacing `health.rs`'s hand-kept `well_known()`.

### D10. `mix database open` still opens a window

`database.open`, the `desktop-app` extension kind and the standalone client's registry entry are unchanged. What
changes is what they find: `mixengine-platform`'s desktop-application lookup learns the merged
application's install location, and the merged application keeps `launch.rs`, `instance.rs` and
the `<old>://` scheme, so `mix database open mariadb` from a terminal opens a Database tab in the
running window, with the password in the environment variable and never on the command line, as
T83 specified. A standalone client still installed is found the way it is today.

Inside the window, *open* on a database service goes through `open_in_<old>.rs`'s in-process
path — `database.credentials`, a `Handoff`, a tab — and never through the OS. That is already how
The standalone client 0.0.33 behaves; the rename is cosmetic.

### D11. Profiles: a visibility setting with three presets

The shell's `MODULES` list is unchanged. The shell settings store gains `enabledModules: string[]`
and every place that enumerates modules reads through `visibleModules()` instead:

- the `[+]` menu and `Ctrl/Cmd+T`'s default module;
- `Ctrl/Cmd+1 … N`, which index into the visible list in order;
- the Settings dialog's per-module panes;
- session restore, which drops a tab whose module is hidden rather than mounting it blind.

| Preset | `enabledModules` | Default tab |
| --- | --- | --- |
| **MixEngine** | `mixengine` | Dashboard |
| **Everything** | all five, the standalone client's order with `mixengine` first | Dashboard |
| **Database tools** | `db`, `rest`, `terminal`, `tools` | Database |

A preset is a shortcut to a set; the Settings pane also offers the five checkboxes, so a person
who wants MixEngine and the terminal can have exactly that. Turning a module off closes its open
tabs after a confirmation; turning it on mounts nothing until a tab is opened.

**First run** — a directory with no settings — shows a single screen: *what will you use MixLab
for?*, the three presets, one click. D7's import skips the screen and picks *Everything*.

**The bridge when a module is hidden.** The Services screen's *open* affordance is drawn from
`database.client` as today. With `db` hidden, the same button offers two things instead of acting:
*use the built-in client* (which enables the module and opens the tab) or *use an external client*
(the `database.open` handoff, if one is installed). Nothing the daemon answers changes; the client
is choosing between two affordances it already has.

### D12. CI and the release

A new `desktop` job on ubuntu: `npm ci`, `npm run build`, `npm test`, `npm run lint`, then
`cargo clippy --locked --all-targets -- -D warnings` and `cargo test --locked` in the nested
workspace. It runs whenever `ci.yml` runs — on request and on a tag, like everything else — and
`release` waits for it as it waits for `bindings`.

The five `build` legs gain a Node setup and the steps in D8. **The two Linux legs build the window
on the runner, not in the manylinux container**: the container is AlmaLinux 8, whose WebKitGTK is
the 4.0 API on libsoup 2, and Tauri 2 links 4.1 on libsoup 3. So those legs' hosts are pinned to
`ubuntu-22.04` and `ubuntu-22.04-arm` — the container step is indifferent to its host, and the
window gets the glibc 2.35 floor the standalone client's own releases had rather than whatever `ubuntu-latest`
carries this month. `lint`'s `cargo deny` does not reach the nested workspace; the `desktop` job
runs `cargo audit` there against `apps/desktop/src-tauri/.cargo/audit.toml`, which is the standalone client's
`audit.yml` policy and its two documented ignores. The standalone client's `ci.yml`, `release.yml`,
`tool-downloads.yml` and `update-notes.yml` are not moved: the first two are replaced by the jobs
above, the third becomes a step of `desktop` that only reads, and the fourth has nothing to write
once D9 lands.

Versions in the release feed, the archive names and the handbook's install page do not change
shape; the artifacts merely carry one more file. The handbook gains a page for the window,
in both languages, under [ADR 0021](../decisions/0021-the-handbook-is-one-corpus-published-three-ways.md)'s rules.

### D13. What `mixnz/<old>` does after this

Nothing lands there after T101 except a README pointing here. When M12 ships — one installer, one
updater — its last release notes name the MixEngine release that replaces it, and the repository
is archived. Between T101 and M12 the standalone application keeps working against the daemon, as
it does today, because nothing on the API side changes for it.

## Error handling

- **D7 import** — a store file that fails to parse is skipped and named in the log; a keyring
  entry that cannot be read leaves the saved connection with no password, which is the state the standalone client
  already renders as *ask again*. An import never blocks the window from opening.
- **D9 apply** — a payload missing the desktop executable is a feed the desktop application refuses
  with the reason, since `provides` is what the updater reads and an entry absent from it is a
  release that did not build the window. `feed-check.sh` is what keeps that from being a
  release-day discovery.
- **D9 launch** — `mixengined` found nowhere is the third of `health.rs`'s three states, *no
  MixEngine here*, which is already drawn distinctly from *not running* and *not answering*.
- **D11 hidden module** — a `<old>://` handoff arriving while `db` is hidden enables the module
  for that tab and says so in the tab, rather than dropping a connection somebody just asked for.

## Testing

- **Layering**: `apps/desktop/src-tauri/tests/layering.rs` (D2); `packaging.rs`'s version and
  binary-list checks (D6, D8).
- **Endpoint**: the fingerprint pin moves to `mixengine-platform`'s tests; the desktop transport
  test binds a `mixengine_platform::ipc::Listener` on a temporary home's endpoint, answers one
  HTTP request on it, and dials it through the same code the window uses — the real transport on
  all three OSes with no daemon to build. A test that wants a whole daemon takes
  `mixengine-testkit` as a dev-dependency, the one edge into this workspace the layering test
  allows there.
- **Import**: vitest over the pure half (which files, which accounts, the marker) and one Rust test
  with a fabricated the standalone client directory and a throwaway keyring service.
- **Updater**: `feed-check.sh` for the fifth `provides`; a `core::updates` unit test that an
  archive with five binaries applied over a directory holding four writes four.
- **Profiles**: vitest over `visibleModules()`, the shortcut mapping and session restore; the
  first-run screen is a component with a snapshot.
- **End to end**: the `system` job's clean-VM smoke (T87) gains the window: install, first-run
  screen, Dashboard, create a site, open its database in a tab, uninstall leaves nothing.

## Out of scope, deliberately

- Redesigning any screen, the tab strip or the sidebar for the *MixEngine* profile.
- Folding the desktop crate into the root workspace.
- Code signing on any platform.
- New API methods. Every screen keeps drawing from what `client-surface.md` already lists, and a
  gap found while doing this is a Phase 10 task, not a desktop one.
