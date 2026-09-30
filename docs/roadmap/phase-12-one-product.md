# Phase 12 — One product

*Goal: one download installs everything, one updater keeps everything current, and a standalone-client user
loses nothing by switching.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Design: [2026-09-08-the-desktop-client-in-this-repository-design.md](../specs/2026-09-08-the-desktop-client-in-this-repository-design.md),
on [ADR 0027](../decisions/0027-the-desktop-client-lives-in-this-repository.md).

---

**This is the phase the users asked for.** The two complaints ADR 0027 opens with — *two downloads*
and *a database client I did not want* — are answered by this phase and the next respectively.
Phase 11 made the window buildable here; this one makes it *the* window, named MixLab,
inside MixEngine's installers, replaced by MixEngine's updater.

- [x] **T104** The application is MixLab. Product name, identifier `io.github.mixnz.mixlab`,
      executable names, window title and icon per the design's D6 — and only those: the daemon,
      the CLI, the home and the installers keep MixEngine's name. A mark of MixLab's own replaces
      the standalone client's two SVGs, derived from MixEngine's until one is drawn. **And the version becomes the workspace's**: the three fields the desktop
      build reads are held to `[workspace.package].version` by `packaging.rs` from here on — held
      back from T101 because a lower version under the standalone client's still-wired updater is an app that
      offers to replace itself with standalone client. **And a standalone-client user's data comes across, once** (D7): on the first launch
      that finds no settings, the store files are copied and the keyring entries they name are
      read under service `<Old>` and written under the new one — the old directory and the old
      entries never touched, a marker file so it never runs twice, `keyringRef`s into MixEngine's
      namespace copied as they are, and nothing from the webview's `localStorage`: theme and the
      last tab strip are not things a person made. An import found sets the profile to *Everything* once T108
      exists; until then it changes nothing visible. **(P)** — three application-data locations,
      three keyrings.
      Design: [2026-09-09-t104-the-application-is-mixlab-design.md](../specs/2026-09-09-t104-the-application-is-mixlab-design.md).
      **Three things this task settled.** The mark is drawn here rather than handed over: the
      design's D6 expects the owner's SVGs and they have not arrived, so `public/logo.svg` draws
      D6's description — a ring open at the lower right, an `ML` ligature whose `L` leaves through
      the gap — in one cut whose weights hold at 16 px, and the second, heavier cut waits for them.
      The import is Rust on both halves rather than the vitest-and-Rust split the design imagined:
      the file copy has to run inside `setup()`, before the event loop can deliver the webview's
      first `Store.load`, and that is the only moment in which it is race-free. And **the version
      drop was not safe on its own**: D6 argued that the rename moves the application out of
      the standalone client's install directory, which is true only on Windows — on macOS and Linux
      `tauri-plugin-updater` replaces the *running bundle*, so a window on `0.0.6` offered the standalone client
      `0.0.33` would have overwritten itself with it. The plugin stays wired for T106; the frontend
      stops calling it.

- [x] **T105** The window is in every installer, and the headless archive stays (D8).
      `MIX_BINARIES` and `MIX_CRATES` gain the fifth entry and `packaging.rs` holds the list. NSIS
      gets a Start Menu shortcut, an optional desktop shortcut and the `<old>://` registration;
      the `.pkg` places `MixLab.app` in `/Applications`; `.deb` and `.rpm` add a `.desktop` file
      and declare WebKitGTK; the AppImage's `AppRun` starts the window when given no arguments and
      runs `mix` with them otherwise. A new
      `mixengine-<version>-<os>-<arch>-headless` archive carries the four binaries and nothing
      else. Each script's own check — *the binaries are really in there* — counts five, or four
      for the headless one. **(P)**
      Design: [2026-09-09-t105-the-window-in-every-installer-design.md](../specs/2026-09-09-t105-the-window-in-every-installer-design.md).
      **Two things this task settled.** The executable is `mixlab` on every operating system and not
      D6's `MixLab.exe` on Windows: `updates::apply::swap` looks a payload's name up as
      `directory.join(binary_name(name))` and `binary_name` appends `EXE_SUFFIX` and nothing else,
      so a capitalised install file is one T106's updater would skip for ever without a word — no
      error anywhere, just a window that never updates. What a Windows user clicks is a Start Menu
      shortcut, and that is named MixLab. And **the AppImage does not carry WebKitGTK**, contrary to
      one line of D8: appimagetool bundles no libraries, and doing it means `linuxdeploy` and its
      GTK plugin — a dependency and a failure mode of a different size. `AppRun` fails in words
      naming the package to install instead, and bundling is T105a below. And **a `.app` in a
      `.pkg` is relocatable unless you say otherwise**: `pkgbuild` made `MixLab.app` a component,
      and `installer(8)` then asked Launch Services where that bundle identifier already lived and
      wrote it *there* — into the work tree's own build output on the runner. Green package, green
      `pkgutil --payload-files`, no `/Applications/MixLab.app`; caught by `macos/probe.sh`, which
      installs for real, on run 34274920375. `--component-plist` with `BundleIsRelocatable` and
      `BundleIsVersionChecked` false is the fix. Two smaller things fell out: the window is built by
      a script of its own (`packaging/desktop.sh`) because `stage.sh` builds with `cargo -p` from a
      root that excludes that crate, and `feed.sh` had to learn to skip `*-headless.*` — the new
      archives match its payload globs, and one left in would have stopped the whole `release` job
      at "is not a payload name this script recognises".

- [x] **T105a** The AppImage does not carry the libraries the window needs, and that is now a
      decision rather than an interim (D8). The fork was `linuxdeploy` and its GTK plugin, or the
      measurement that says a distribution floor is cheaper than carrying WebKitGTK; the measurement
      won, and it is [ADR 0028](../decisions/0028-the-appimage-does-not-carry-webkitgtk.md). The
      AppImage's window uses the system's WebKitGTK 4.1, `AppRun` says which floor was missed rather
      than always naming the webview, and the command line inside the same image is unaffected either
      way. **(P)**
      Design: [2026-09-09-t105a-the-appimage-and-webkitgtk-design.md](../specs/2026-09-09-t105a-the-appimage-and-webkitgtk-design.md).
      **Three measurements settled it, on run 34298077029.** *What carrying would buy*: every
      distribution new enough to run the window already packages WebKitGTK 4.1, and the ones that do
      not are below the window's own glibc floor, which no bundle lowers — the window is built on
      `ubuntu-22.04` because it cannot be built in the container the other four come from (D12).
      Exactly one family is left, enterprise Linux 9, and the reading is closer than the table looks:
      the window requires `GLIBC_2.34`, which that release has, so it fails on the webview alone.
      *Whether carrying would be enough there*: no — its glib is older than the 2.70 WebKitGTK 4.1
      needs, which is why the package was never backported, so carrying the webview there means
      carrying glib, GIO and libsoup 3, the libraries that break when bundled. *Who would pay*: the
      closure is **205 MB across 133 files** on `x86_64` and 200 MB on `aarch64`, 117 MB of it
      WebKitGTK and JavaScriptCore, against an AppImage that is **33 MB** today — and `AppRun`
      extracts into a per-version cache before running anything, so all of it would land on the
      machine of the person running `… status` on a headless server.
      **And three things fell out of it.** The floor is **two floors in one artifact** — the window at
      glibc 2.35 with the distribution's WebKitGTK, the four command-line binaries at 2.28 in the same
      file — and both install pages now say so; `MIX_WINDOW_GLIBC` stays the *build machine's* 2.35
      rather than the measured 2.34, because no distribution sits between the two, and
      `packaging/linux/window-floor.sh` holds the binary under it (`<=`, so a toolchain that needs
      less is not a red build) and asserts the soname, from which
      `crates/mixengine-core/tests/packaging.rs` derives the three package names the pages promise.
      **`AppRun`'s old check named the wrong thing**: it grepped for a missing library and always
      blamed WebKitGTK, while the same `ldd` reports a too-old distribution as `` version `GLIBC_…'
      not found `` — so that person was told to install a package they already had. And it said it
      **where nobody was listening**, since a double click in a file manager has no terminal; the
      refusal is now shown in `zenity`, `kdialog` or `xmessage` when there is a display. Last:
      `packaging/linux/apprun-check.sh` had been in this repository since T85c and **nothing had ever
      run it** — it is a step of `lint` from here on.

- [x] **T106** One updater (D9). `tauri-plugin-updater`, `tauri-plugin-process`, the standalone client's key, its
      `latest.json` and `update-notes.yml` are gone. The feed's `provides` and the payload carry
      the desktop executable; `feed-check.sh` asserts it. **`updates::apply` replaces what the
      install has and adds nothing** — a headless install stays headless, and an install from
      before this release is told in the release notes that the window arrives by installer. After
      `UpdateApplied` the window relaunches itself the way the daemon does. **(P)** — on Windows a
      running executable is renamed, never overwritten, and the window is the running one.
      Design: [2026-09-09-t106-one-updater-design.md](../specs/2026-09-09-t106-one-updater-design.md).
      **Three things this task settled.** **The window's payload entry is a directory on macOS, and
      three layers had to learn it.** `feed.sh` skipped every directory entry it saw, so a payload
      carrying `MixLab.app` would have been described as four binaries and no window;
      `install::present` demanded `is_file()`, which would have refused the whole macOS payload —
      reporting the window and taking the four binaries with it; and `swap` resolved every name by
      appending an executable suffix, which no bundle name is. The name is now
      `mixengine_platform::install::application_file_name`, and the two constants it is asked with
      are held to `packaging/common.sh` beside `MIX_BINARIES`. The tree path is tested on all three
      systems rather than on macOS alone, because `replace` branches on `is_dir()` and never on the
      operating system. **A window cannot ask where it is after it has been replaced**: on Linux
      `/proc/self/exe` follows the inode, so a window calling `current_exe()` after the swap is told
      its own path is `…/mixlab.old` and would relaunch the version the user had just replaced —
      silently, reporting the old number in Settings for ever. The path is read on the first line of
      `run()` instead. And **a relaunch is not a start**: `launch::forward` would have handed the new
      copy's start to the copy it was replacing and exited, leaving no window at all, so the child
      waits for its predecessor's endpoint to go quiet — `instance::listening`, a probe that delivers
      no line and so brings no dying window to the front — before taking it. The one thing D9 asked
      for that this task could not do is the desktop application *refusing* a feed with no window in
      it: it is not the thing that reads the feed, and a release that built no window is still one a
      server should be able to install. `packaging/feed-check.sh` and `.github/scripts/test-feed.sh`
      are what keep that from being a release-day discovery, and the second of those runs in `lint`
      on every CI run.

- [x] **T107** Where the daemon is, and where the window is, are both the platform's to answer
      (D9, D10). `health.rs`'s hand-kept `well_known()` is replaced by one `mixengine-platform`
      function — the executable's own directory, then the OS's install location, then `PATH` —
      that packaging reads too. `mixengine-platform`'s desktop-application lookup learns the merged
      application's location beside standalone client's, so `mix database open` from a terminal
      opens a tab in the running window, password in the environment and nowhere else, exactly as
      T83 specified. `<old>://`, `launch.rs` and `instance.rs` stay. **(P)**
      Design: [2026-09-09-t107-where-the-daemon-and-the-window-are-design.md](../specs/2026-09-09-t107-where-the-daemon-and-the-window-are-design.md).
      **Four things this task settled.** **The window is looked for where this install is, and
      nowhere else** — not on `PATH`, not in App Paths, not through Spotlight, which is the opposite
      of how the same crate finds standalone client. Two reasons, and the second is the one that
      would have been found late: a `mixlab` first on somebody's `PATH` may belong to a *different*
      install of MixEngine, so which window a database opened in would depend on the order of a
      `PATH`; and a lookup that reads the machine rather than the install turns
      `crates/mixengine-cli/tests/database.rs` red on every developer's machine that has MixLab on
      it — those two tests assert *no client*, and the machine this was written on already has
      MixEngine installed and on `PATH`. What is left is beside the running program, plus
      `/Applications` on macOS alone, and only for a daemon that is itself in `/usr/local/bin`:
      that is the one system whose installer splits the binaries from the bundle. **The window
      answers before the extension, but only for its own scheme.** `desktop-app` is a general
      mechanism, and a future entry naming some other client for some other scheme must not be
      shadowed by a window that cannot read its URLs — so the condition is `scheme == <old>`, which
      is also what leaves T83's `nowhere` fixture answering exactly what it answered before. **A
      client that is not an extension is an optional field and not a third arm**:
      `DesktopClient::Installed.extension` became `Option<ExtensionId>`, skipped when absent, so the
      document an extension produces is byte-for-byte what it was and the one new case is a key that
      is not there. And `Databases::scheme()` went with it — it was a second read of the extension
      store, carrying an `Internal` error for *the desktop client vanished between two reads*; the
      client is resolved once now and that race is gone. Last: the daemon's presence check in the
      window **stopped running `mixengined --version`** to answer *is it installed*, which was a
      process creation and a hidden console for a question three `stat`s answer.

- [x] **T111** The dev loop stages the daemon beside the window. `npm run dev:app` builds the four
      headless crates at the root and copies them into `src-tauri/target/debug/` before `tauri dev`,
      so T107's first step finds a daemon built from the same tree — and never, on a machine with a
      release installed, the release daemon at its second step, started against the wrong home.
      `MIXENGINE_HOME` is set to the repository's `.mixengine-home` for the window unless it is
      already set. The *not installed* gate says which directories it looked in and offers a
      reinstall rather than a first download, which is what that state has meant since ADR 0027.
      **(P)** — the executable suffix, the copy refusal and the home path.
      Design: [2026-09-10-t111-the-dev-loop-stages-the-daemon-design.md](../specs/2026-09-10-t111-the-dev-loop-stages-the-daemon-design.md).
      **Three things this task settled.** The script **starts `tauri dev` itself** rather than
      standing in front of it behind `&&`: two commands joined that way are two processes, and the
      home the daemon must share would not survive the first one ending. The list is read out of
      `packaging/common.sh` by a Node parser rather than by sourcing it — `npm run` is typed into
      PowerShell, where a bare `bash` on a machine with WSL is WSL's, and the two scripts already in
      `apps/desktop/scripts/` are Node for that same reason. And **the gate's copy needed a test of
      its own**: `t("mixengine.gate.notInstalled", { searched })` interpolates `{{searched}}`, and a
      translation that dropped the placeholder would compile, pass every other test, and silently
      hide the one answer that screen exists to give — `i18n/gate.test.ts` is what says no.
      **And opening the window is what found the fourth.** The searched directories were first
      interpolated into the sentence, which reads fine with the three entries a design discusses and
      not at all with the forty-five a real `PATH` has: the paragraph filled the window and pushed
      the *Reinstall* button off the bottom edge, so the screen lost the one control it exists to
      offer. They are a bounded, scrollable list now, and every entry is kept — naming two while the
      lookup walked forty-seven would be a shorter lie. Nothing but a screenshot was going to say so.

- [x] **T165** The window is the only desktop client. `desktop-app` leaves the manifest format, the
      API, the daemon, the platform layer, `mix` and MixLab, and migration 24 removes installed rows
      of the kind — its one entry, the standalone client, was withdrawn from the registry on 2026-09-17 and *is*
      MixLab. `database.client` answers `installed` for this install's window or `no_client`; the
      per-OS lookups and T107's scheme rule go with it. **(P)** — three lookups deleted.
      [ADR 0038](../decisions/0038-the-window-is-the-only-desktop-database-client.md).
      Design: [2026-09-17-t165-the-window-is-the-only-desktop-client-design.md](../specs/2026-09-17-t165-the-window-is-the-only-desktop-client-design.md).
      **No protocol bump**, for the reason the ADR gives: everything removed was written only by a
      daemon. **And the one real handoff test kept a real client**: `mariadb.rs` runs a copy of the
      daemon out of its own directory with a script named `mixlab` beside it, since beside the
      running program is the only place the window is looked for. **And the upgrade census learned a
      partial loss**: `upgrade.rs` gains `REMOVED` beside `EMPTIED`, and the first fixture it met was
      `schema-0001`, whose `extensions` table migration 16 had already emptied whole.

**Milestone M12** — on a clean machine of each OS, one installer installs the daemon, the CLI, the
helper, the shim and the window; the window's Update button and `mix self-update` each replace all
five; a standalone-client user's saved connections open in the new window with their passwords; `mix database
open` from a terminal lands in a tab; the uninstaller leaves nothing (T87's smoke, extended).
`mixnz/<old>` is archived on the day this ships.
