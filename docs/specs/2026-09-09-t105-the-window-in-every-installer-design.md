---
status: implemented
date: 2026-09-09
task: T105
---

# The window is in every installer — design

Roadmap task **T105**, [phase 12](../roadmap/phase-12-one-product.md), on the
merge design's [D8](2026-09-08-the-desktop-client-in-this-repository-design.md#d8-packaging-a-fifth-binary-and-the-same-six-installers)
and [ADR 0027](../decisions/0027-the-desktop-client-lives-in-this-repository.md).

## Goal

One download installs everything. After this task every MixEngine installer places **five**
executables instead of four — the daemon, the CLI, the shim, the privileged helper and **MixLab**,
the window — and a Windows user gets a Start Menu entry, an optional desktop shortcut and a
`<old>://` handler; a macOS user gets `MixLab.app` in `/Applications`; a Linux user gets a menu
entry and an icon, with WebKitGTK declared as the runtime dependency the four binaries never had;
and an AppImage opens the window when double-clicked while `./mixengine-*.AppImage status` keeps
working.

The CLI-only user — a server, a container image, a machine with no display — is not made to carry a
webview for it. A new **headless** archive per OS/arch carries the four binaries and nothing else,
and declares nothing.

Nothing about the updater changes here. `updates::apply`, `latest.json` and the Tauri updater plugin
are [T106](../roadmap/phase-12-one-product.md)'s.

## Decisions

### D1. The fifth name is `mixlab`, on every operating system

`MIX_BINARIES` gains `mixlab` and `MIX_CRATES` gains `mixlab`. The name is the cargo package's,
lower case, with no per-OS spelling:

| | Built | Installed |
| --- | --- | --- |
| Windows | `mixlab.exe` | `mixlab.exe`, beside the four |
| Linux | `mixlab` | `/usr/bin/mixlab` |
| macOS | `MixLab.app` | `/Applications/MixLab.app`, whose `Contents/MacOS/mixlab` is the same binary |

**This is a deliberate departure from the merge design's D6**, whose table spells the Windows
executable `MixLab.exe`. That spelling cannot survive T106. `updates::apply::swap` looks each name
from a payload's `provides` up as `directory.join(binary_name(name))`, and `binary_name` appends
`std::env::consts::EXE_SUFFIX` and nothing else — so a payload carrying `mixlab.exe` against an
install holding `MixLab.exe` matches rule 2 ("a name this install does not have is skipped"), and
the window would be quietly excluded from every update for ever, with no error anywhere. One name
that the updater, the packaging scripts and `feed.sh`'s `provides` all spell the same way is worth
more than a capital letter in Explorer, and Explorer is not where a user meets this file anyway:
they click a Start Menu shortcut named **MixLab**.

`mainBinaryName` is deliberately left unset in `tauri.conf.json`. Its documented default is "the
output binary from `cargo`", which is what produces `mixlab` today and what `.github/workflows/ci.yml`
already uploads as the `desktop-<os>` artifact. Setting it to `MixLab` would rename the Linux binary
to `/usr/bin/MixLab`, which the merge design's own D6 table rules out.

`MIX_WINDOW=mixlab` is declared beside the two arrays, so every script that has to treat the fifth
entry differently — and several must — names it from one place rather than spelling it inline.

### D2. The window is built once per leg, by `packaging/desktop.sh`

`stage.sh` cannot build this crate. It runs `cargo build -p <crate>` at the workspace root, and
`apps/desktop/src-tauri` is `exclude`d from that workspace (ADR 0027, rule 5) — `-p mixlab` there is
an error, not a build. D8 says as much: each leg runs `npm ci` and `cargo tauri build`, and
"`stage.sh` copies the result beside the four binaries".

So a new script:

```
packaging/desktop.sh [--target <triple>]
```

- requires `node` and `npm` by name, the way every other script requires its tools;
- exports `MIXENGINE_RELEASE=1`, for `stage.sh`'s own reason (T95): a window built without it
  defaults to the `MixEngine-dev` home and dials a pipe nobody installed;
- runs `npm ci` in `apps/desktop`, then
  `npm run tauri -- build --no-bundle --target <triple>`, or on macOS
  `npm run tauri -- build --bundles app --target universal-apple-darwin`, because a webview
  application there is a directory rather than a file;
- **opens what it just made**: asserts `mixlab`/`mixlab.exe` exists, or on macOS that the bundle
  glob matched exactly one directory and that it is named `MixLab.app`. A Tauri release that starts
  renaming its output stops this script by name rather than three artifacts later;
- copies the result into `$MIX_OUT/window/<key>/` and prints that directory.

The `<key>` is the target triple, except on macOS where it is always `universal-apple-darwin`:
`macos/build.sh` calls `stage.sh` once per slice, and both slices take the same universal bundle.
`mix_window_key` in `common.sh` is that one line.

`stage.sh` copies from `$MIX_OUT/window/<key>/` into the stage, and **runs `desktop.sh` itself when
that directory is empty**. On CI the explicit step has already filled it, so nothing is built twice;
on a developer machine `bash packaging/linux/build-deb.sh` still works end to end. The four Linux
packaging scripts each call `stage.sh`, so without this the window would be built four times per
leg — roughly forty minutes for one answer.

**The window is built on the host, never in the container.** `stage.sh --container` passes only the
`cargo build` line into `mix_in_container`; everything else in the script, this copy included, runs
on the runner. That is not incidental: the manylinux image is AlmaLinux 8, whose WebKitGTK is the
4.0 API on libsoup 2, and Tauri 2 links 4.1 on libsoup 3. `ci.yml` already says so for its own
build step; the arrangement here keeps it true.

### D3. What each installer places

| Platform | Artifact | What it places |
| --- | --- | --- |
| Windows | `mixengine-<v>-windows-<arch>-setup.exe` | the five in `$LOCALAPPDATA\Programs\MixEngine`; a Start Menu shortcut; an optional desktop shortcut; `<old>://` |
| Windows | `mixengine-<v>-windows-<arch>.zip` | one `mixengine/` directory with the five |
| Windows | `mixengine-<v>-windows-<arch>-headless.zip` | one `mixengine/` directory with the four |
| macOS | `mixengine-<v>-macos-universal.pkg` | four to `/usr/local/bin` and `/Library/PrivilegedHelperTools`, `MixLab.app` to `/Applications` |
| macOS | `mixengine-<v>-macos-universal-headless.tar.gz` | the four |
| Linux | `.deb` / `.rpm` | five to `/usr/bin` and the helper's path, a `.desktop` file, two icons, WebKitGTK declared |
| Linux | `mixengine-<v>-linux-<arch>.AppImage` | the five in the AppDir; `AppRun` opens the window with no arguments |
| Linux | `mixengine-<v>-linux-<arch>-headless.tar.gz` | the four |

**One thing the macOS package has to switch off.** `pkgbuild` turns any `.app` under `--root` into a
*component* and makes it **relocatable** by default, and a relocatable component is not installed at
the path the package names: at install time the installer asks Launch Services where a bundle with
this identifier already lives and writes it there instead. Measured on run 34274920375, where
`installer(8)` reported success, every other path was written, and `/Applications/MixLab.app` did not
exist — Launch Services had indexed the copy `packaging/desktop.sh` had built minutes earlier inside
the work tree. On a user's machine the same rule would quietly install MixLab wherever an older copy
had been dragged. So `build.sh` runs `pkgbuild --analyze`, sets `BundleIsRelocatable` and
`BundleIsVersionChecked` to false on the one component, reads both back, and passes the result as
`--component-plist`. Version checking goes with relocation for a plainer reason: left on, a machine
that already has this version keeps the copy it has and the package writes nothing, which is fine
while the bytes are identical and wrong the moment they are not. The other four paths are plain
files and are always written; the window is now the same.

### D4. NSIS: two shortcuts, one scheme, and an uninstaller that takes back only its own

`packaging/windows/mixengine.nsi` gains, in the install section, `File "${STAGE}\mixlab.exe"` and a
`Delete` line to pair with it — the pairing that file already documents as not decoration.

**The Start Menu shortcut is flat**: `$SMPROGRAMS\MixLab.lnk`, not a one-item folder. `SetShellVarContext`
stays at its default, which under `RequestExecutionLevel user` is this account's own Start Menu, and
nothing here asks for UAC.

**The desktop shortcut is an optional section**, which is what the roadmap's "optional" means in a
file with no components page today:

```
Page components      ; new, before the directory page
...
Section "MixEngine" SecCore
  SectionIn RO
Section /o "Desktop shortcut for MixLab" SecDesktop
```

`/o` means unselected by default, and a silent install (`/S`, which `packaging/windows/probe.sh`
uses) takes the defaults — so the probe's readings are unchanged and no unattended install grows a
desktop icon nobody asked for.

**`<old>://` is registered per user**, under `HKCU\Software\Classes\<old>`, with `URL Protocol`, a
`DefaultIcon` and a `shell\open\command` of `"$INSTDIR\mixlab.exe" "%1"`. Taking the scheme over
from a standalone client that is still installed is intended — D10 of the merge design says the
merged application is what `<old>://` opens.

**The uninstaller may not simply delete that key.** A standalone client may still be installed and
still be in use, and D7's rule is that the old copy is never touched. So `un.RemoveScheme` reads
`shell\open\command` back, looks for `$INSTDIR` inside it with the `StrFind` macro the file already
has, and deletes the key only when the command it finds is ours. A machine where the standalone client re-registered
itself after us keeps the standalone client's handler, which is the correct outcome and the quiet one.

### D5. Linux: a `.desktop` file, two icons, and WebKitGTK declared

A new `packaging/linux/mixlab.desktop`, installed by both native packages to
`/usr/share/applications/mixlab.desktop`:

```
Type=Application
Name=MixLab
Comment=MixEngine's desktop application
Exec=mixlab %u
Icon=mixlab
Terminal=false
Categories=Development;
MimeType=x-scheme-handler/<old>;
```

No `StartupWMClass`: what GTK reports for this window has not been measured, and a wrong value there
groups the running window under no icon at all — worse than the key being absent.

The icons are the window's own, already committed for the Tauri bundle:
`apps/desktop/src-tauri/icons/32x32.png` and `128x128.png`, installed to
`/usr/share/icons/hicolor/<size>/apps/mixlab.png`. `packaging/linux/mixengine.png` — a 16×16
placeholder — stays exactly where it is, as the AppImage's AppDir icon, and is not reused here.

`MimeType` is written and **no cache is updated**, because neither package has a maintainer script
and that is on purpose (`build-deb.sh`'s own header). A menu entry works regardless — menus read
`/usr/share/applications` directly — while `xdg-open <old>://…` reaches MixLab once anything on the
machine next runs `update-desktop-database`, which every desktop environment's own package triggers
routinely. Buying the rest of that with a `postinst` would cost the invariant that nothing runs at
install time, and it is not worth it.

**WebKitGTK is declared.** The `.deb` gains
`Depends: libwebkit2gtk-4.1-0, libgtk-3-0`; the spec file gains
`Requires: (webkit2gtk4.1 or libwebkit2gtk-4_1-0)`, an rpm boolean dependency (rpm 4.13+) because
the Fedora/RHEL name and the openSUSE name for one library are different and the install page offers
the `.rpm` to both. A distribution too old to have the 4.1 API cannot run this window at all, so a
package that refuses to install there says the true thing at the moment a person can act on it.

### D6. The AppImage: no arguments is the window, any argument is `mix`

`AppRun` keeps everything it does today — it unpacks itself into
`${XDG_CACHE_HOME}/mixengine/<version>` per file, for the reasons written in it — and changes only
its last line:

```
if [ "$#" -eq 0 ]; then
  exec "$cache/mixlab"
fi
exec "$cache/mix" "$@"
```

So `./mixengine-<v>-linux-x86_64.AppImage status` keeps working, `--version` — which
`build-appimage.sh` uses as its end-to-end proof — keeps working, and a double click opens MixLab.
The AppImage runtime consumes its own `--appimage-*` arguments before `AppRun` sees them, so none of
those reach either branch.

`packaging/linux/mixengine.desktop`, the AppDir's own, changes `Terminal=true` to `Terminal=false`:
a double click now opens a window, and a desktop environment that integrates the image should not
wrap it in a terminal. Its `Name` stays **MixEngine** — the AppImage is the whole product, not the
window — and its `Exec` stays `mix` with no `%u`, since with an argument `AppRun` runs the CLI.
`<old>://` on an AppImage is not offered, here or in D8.

**The AppImage does not carry WebKitGTK**, contrary to one line of D8. `appimagetool` bundles no
libraries; doing it would mean `linuxdeploy` and its GTK plugin, a dependency and a failure mode of
an entirely different size, and it is not in this task. What `AppRun` does instead is check, before
the no-argument branch, whether the window's dynamic dependencies resolve, and print one actionable
line naming WebKitGTK if they do not — rather than exiting 127 with a linker message. Bundling stays
a follow-up, recorded in the roadmap rather than assumed here.

### D7. The headless archive, and keeping it out of the feed

Three new artifacts, one per OS leg, built from the same stage as everything else by copying every
`MIX_BINARIES` entry except `$MIX_WINDOW`:

```
mixengine-<version>-windows-<arch>-headless.zip
mixengine-<version>-linux-<arch>-headless.tar.gz
mixengine-<version>-macos-universal-headless.tar.gz
```

Each holds one top-level `mixengine/` directory, like every other archive this product publishes, so
a person who extracts one gets a folder rather than four loose files. Each gets a `.sha256`, and an
unversioned alias for the handbook to link (`mixengine-<os>-<arch>-headless.<ext>`).

**`feed.sh` must be taught to skip them.** Its payload glob is
`mixengine-$version-linux-*.tar.gz` and the two like it, which a `-headless.tar.gz` matches; the
`case` that maps a name to an (os, arch) pair would then reach its `*)` arm and stop the whole
`release` job with "is not a payload name this script recognises". So the collection loop excludes
`*-headless.zip` and `*-headless.tar.gz` by name, with the reason written beside it. A headless
archive is a download, never an update payload: an install that has no window has nothing an update
would replace, which `updates::apply`'s rule 2 already guarantees without any help from the feed.

### D8. What the update payloads carry in this task

The Windows portable `.zip` and the Linux `.tar.gz` are both payloads *and* downloads, and both are
assembled by looping `MIX_BINARIES` — so they gain the window here, and `feed.sh` lists it in
`provides` because it is a plain file under `mixengine/`. That is harmless and it is the shape T106
wants: `swap` skips any name the install does not have, so a headless install stays headless today,
before T106 writes the rule down.

**The macOS payload keeps the four.** `feed.sh` builds `provides` only from entries directly under
`mixengine/` that are not directories — it skips `mixengine/*/*` and anything ending in `/` — so a
`MixLab.app` inside that tarball would add forty megabytes that no reader would ever look up. macOS
also cannot go through the `lipo -create` loop that assembles that archive, since the bundle is
already universal. Both are one skip, keyed on `$MIX_WINDOW`, and both are T106's to revisit when
the feed learns to describe a `.app`.

### D9. Each script's own check counts five, or four

The rule the T85 design set — *open what was just made and check the binaries are in it* — is
unchanged in shape and gains the fifth name automatically, because every one of those loops reads
`MIX_BINARIES`. What each script adds:

- `stage.sh`'s "missing from the stage" loop tests a directory for the window on macOS and a file
  everywhere else.
- `windows/build.sh` keeps its two loops (whole-entry `grep -qx` for the zip, substring for the
  installer listing) and adds a headless loop that counts four **and asserts the window is absent** —
  an archive that quietly grew a webview is exactly the failure the headless artifact exists to
  prevent.
- `macos/build.sh` asserts `./Applications/MixLab.app/Contents/MacOS/mixlab` is in the package, and
  that it carries both slices — read through the bundle's `CFBundleExecutable` rather than assumed,
  so the check is about what macOS will actually launch.
- `build-deb.sh` and `build-rpm.sh` add `/usr/bin/mixlab`, the `.desktop` file and the two icons to
  their expected-path lists, and assert the dependency field is really in the built package
  (`dpkg-deb -f … Depends`, `rpm -qp --requires`).
- `build-appimage.sh` keeps its run-it check and adds `usr/bin/mixlab` to the AppDir assertion beside
  the helper's.
- `linux/build-tarball.sh` gains the headless tarball and its four-and-not-five check.

### D10. The probes, and the one hand-kept list in them

`packaging/windows/probe.sh` walks `MIX_BINARIES` in four places and therefore covers `mixlab.exe`
with no edit at all — which is the point of that array. It will now fail if the installer or the
uninstaller forgets the window.

`packaging/macos/probe.sh` is the exception: it keeps a hand-written `paths` array, walked by the
occupied check, by `cleanup` and by M5, with a comment saying what a stale copy of it costs. It gains
`/Applications/MixLab.app`, and `cleanup`'s `sudo rm -f "${paths[@]}"` becomes `rm -rf` — a bundle is
a directory, and `rm -f` would leave the probe's own installation on the machine.

## Error handling

| Situation | What happens |
| --- | --- |
| `node`/`npm` missing when the window must be built | `mix_require node npm` names them and stops, before anything is compiled |
| `tauri build` produced nothing, or something under another name | `desktop.sh` stops naming the path it looked at |
| macOS bundle glob matched none, or more than one | `desktop.sh` stops and lists what it found |
| The stage is missing the window | `stage.sh`'s existing "missing from the stage" message, with the window's name |
| An installer was built without the window | the per-script content check stops the leg, as it does for the other four |
| A headless archive contains the window | the headless check stops the leg |
| WebKitGTK absent on a user's machine | `.deb`/`.rpm` refuse to install and say which package; the AppImage prints one line naming it |
| A standalone client owns `<old>://` at uninstall time | our uninstaller leaves the key alone |

## Testing

- `crates/mixengine-core/tests/packaging.rs`
  - the binary-list test gains the window. The expected name is **read out of
    `apps/desktop/src-tauri/Cargo.toml`'s `[package].name`** with `include_str!`, not spelled in the
    test: the packaging list and the crate that produces the file then cannot drift, which is the
    property the other four names get from their constants.
  - `every_crate_the_stage_builds_is_a_workspace_member` learns the one member that is not under
    `crates/`: the desktop crate is checked against the root manifest's `exclude` list and against
    its own `[package].name`, so a rename of either side is a red test rather than a `-p` that fails
    seven minutes into a packaging run.
  - a new test that `MIX_WINDOW` is declared and is an entry of `MIX_BINARIES` — the headless
    archives are `MIX_BINARIES` minus that one name, and a `MIX_WINDOW` that named nothing would
    make them silently identical to the payloads.
- `packaging/linux/apprun-check.sh` gains `mixlab` to its fixture and two assertions: no arguments
  hands over to the window, any argument hands over to `mix`. It still runs on any machine, needs no
  AppImage and no `appimagetool`.
- `packaging/feed-check.sh` writes a `-headless` archive into its fixture distribution and asserts
  the feed ignores it — one artifact per (os, arch) row and not two, and `provides` still equal to
  `MIX_BINARIES`.
- CI: the `build` job's "Build the window" step becomes `bash packaging/desktop.sh`, and its
  `desktop-<os>` upload points at `target/packaging/window/`. Five legs, six installers, three
  headless archives, and each script's own check is the gate.

## Out of scope, deliberately

- **Everything about the updater.** `tauri-plugin-updater`, the Tauri `latest.json`, `provides`
  gaining the `.app`, and `updates::apply`'s "replaces what the install has and adds nothing" are
  T106. This task changes no Rust in `crates/mixengine-core/src/updates/`.
- **Where the daemon and the window are found.** `health.rs`'s `well_known()` and
  `mixengine-platform`'s desktop-application lookup are T107.
- **Bundling WebKitGTK into the AppImage** — D6 above, recorded as a follow-up in the roadmap.
- **Signing and notarisation.** Unchanged and still not purchased (ADR 0005); the window is one more
  unsigned file in an unsigned release, and `probe.sh` measures exactly that.
- **`mix uninstall` learning about the window.** T87's smoke test is extended at the M12 milestone,
  not here.
