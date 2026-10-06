---
status: implemented
date: 2026-09-23
task: T88f
---

# A `.pkg` is updated by its installer

The design behind [ADR 0050](../decisions/0050-a-copy-the-pkg-installed-is-updated-by-the-pkg.md):
on macOS, a copy of MixEngine that the `.pkg` installed is updated by handing the next release's
`.pkg` to Installer.app. MixEngine downloads and verifies the package, the person installs it, and
MixEngine restarts once the new binaries are on disk.

## Goal

A person who installed MixLab from the `.pkg` presses **Update** in MixLab, or runs
`mix self-update`, and ends up with the new `mix`, `mixengined`, `mixengine-shim`, `MixLab.app` and
`mixengine-elevate`, with the services that were running running again. Nothing in MixEngine
elevates to get there.

## Not in scope

- Windows, the archives and Linux. Their paths do not change (ADR 0050, decision 5).
- A silent install behind a password prompt only. That is the alternative ADR 0050 rejected for now.
- Getting a copy of v0.0.7 onto this path. It cannot be done: the updating code is the installed
  code (ADR 0050, decision 6).
- Signing and notarising the `.pkg`. It is unsigned today, and a copy a browser downloaded is
  refused on first open with no Open button (M2). The person has to allow it under Privacy &
  Security. That is the first install, not the update, and it is a task of its own. So is the
  Installer.app window being titled with the file name rather than "MixLab".

## Measurements to take first

Nothing below is final until these are read on a Mac with macOS 15 or later, with v0.0.7 installed
from its `.pkg`. Each one can change a decision, and the decision it can change is named.

| # | Question | How | Changes |
| --- | --- | --- | --- |
| M1 | Does `pkgutil --file-info /usr/local/bin/mixengined` name `dev.mixengine.cli`? | Run it after installing v0.0.7's `.pkg` | D1. If it does not, the receipt check needs another form, for example `pkgutil --pkg-info dev.mixengine.cli` plus the path |
| M2 | Does a `.pkg` fetched by a process that is not a browser carry `com.apple.quarantine`? Does Installer.app then open it without a Gatekeeper refusal? | `curl -LO` the v0.0.7 `.pkg`, `xattr -l` it, `open` it | D5. If it is quarantined and refused, the design needs a documented answer before it can ship |
| M3 | Does Installer.app replace a running `/usr/local/bin/mixengined` and a running `/Applications/MixLab.app` cleanly, with both still running afterwards on their old images? | With the daemon and MixLab running, install the same `.pkg` again, then `mix status` and use the window | D6 |
| M4 | Does `open -a Installer <pkg>` from a daemon started by the login item bring Installer.app to the front? And what does it answer when the daemon was started over SSH? | Start the daemon both ways and call the open | D5 |

### Readings, 2026-09-24

Mac14,3 (Apple silicon), macOS 15.7.3 (24G419). v0.0.7 installed through Installer.app from
`mixlab-0.0.7-macos-universal.pkg`, fetched with `curl -LO` from the release (68,638,683 bytes,
SHA-256 `ce6d0802…eb85072`, equal to the published `.sha256`).

**Before the install.** Nothing of MixEngine was in `/usr/local/bin` or `/Applications`, but a
receipt was: `pkgutil --pkg-info dev.mixengine.cli` answered `version: 0.0.6`, installed
2026-09-08, listing `usr/local/bin/{mix,mixengine-shim,mixengined}` and the helper, whose files
were gone. On this Mac `/usr/local` is `root:wheel` and **`/usr/local/bin` is `light:staff`**,
writable by the account, with Homebrew at `/opt/homebrew`: the write probe would pass here, on
Apple silicon, not only on the Intel Homebrew Mac the ADR names.

**M1.** `pkgutil --file-info` on each of `/usr/local/bin/mixengined`, `mix`, `mixengine-shim`,
`/Applications/MixLab.app/Contents/MacOS/mixlab`, `…/Contents/Resources/mixengine-elevate` and
`/Library/PrivilegedHelperTools/dev.mixengine.elevate` answered, exit 0:

```
volume: /
path: /usr/local/bin/mixengined

pkgid: dev.mixengine.cli
pkg-version: 0.0.7
install-time: 1790183317
uid: 0
gid: 0
mode: 100755
```

`pkgutil --pkg-info dev.mixengine.cli` answered `version: 0.0.7`, `volume: /`, an empty
`location:`, the same `install-time`; the 0.0.6 receipt was replaced. A file the package did not
install (`/usr/local/bin/code`), a copy of `mixengined` under `/private/tmp`, and a path that does
not exist all answered only `volume: /` and `path: …`, with **no `pkgid:` line and exit 0**. Five
calls took 0.199 s together.

**M2.** The `curl` copy carries `com.apple.provenance` and **no `com.apple.quarantine`** (the
same on a second `curl` download). The package is unsigned: `pkgutil --check-signature` answers
`Status: no signature` (exit 1), and `spctl -a -vv -t install` answers `rejected`,
`source=no usable signature` (exit 3). `open -a Installer` on it exits 0 and Installer.app comes up
on its Introduction page, *"Welcome to the mixlab-0.0.7-macos-universal Installer"*, with no
Gatekeeper dialog. The window's title is the file name, not "MixLab".

The same file downloaded by Safari (`com.apple.quarantine: 0083;…;Safari;…`) and by Edge
(`0081;…;Edge;…`) is refused when opened: *"“mixlab-0.0.7-macos-universal.pkg” Not Opened. Apple
could not verify … is free of malware that may harm your Mac or compromise your privacy."*, with
**Done** and **Move to Trash** and no way to open it from the dialog.

**M3.** With `mixengined` (pid 932, started by MixLab's Start), MixLab (pid 915) and four services
running (caddy, mariadb, php-fpm, redis), the same `.pkg` was installed again through
Installer.app.

| | Before | After |
| --- | --- | --- |
| `mix status` | `0.0.7 — running (pid 932)` | `0.0.7 — running (pid 932, up 7m 55s)` |
| Services | caddy 1558, mariadb 1186, php-fpm 1560, redis 1566 | caddy 1558, mariadb 1186, redis 1566 unchanged; php-fpm 3232 (the tester pressed Stop and Start in MixLab after the install: `reason=Requested` in `daemon.log`) |
| Inode of `/usr/local/bin/mixengined` | 116267696 | 116287000 |
| Inode of `/usr/local/bin/mixengine-shim` | 116267698, 29 links | 116287002, 1 link |
| Inode of `MixLab.app/Contents/MacOS/mixlab` | 116267706 | 116287010 |
| Inode of `/Applications/MixLab.app` | 116267703 | 116287007 |
| `lsof -p 932`, `txt` | `/usr/local/bin/mixengined` node 116267696 | node 116267696, the old, unlinked file |
| `lsof -p 915`, `txt` | `…/MacOS/mixlab` node 116267706 | node 116267706 |
| Modification time of the binaries | Sep 23 22:37 | Sep 23 22:37 |

The daemon stayed up on its old image, and still stopped and started a service afterwards. The
window stayed usable. `daemon.log` has no warning or error from the install. The 28 shims in
`<home>/bin` (hard links made by `shims::place`) still point at the old inode 116267698.
`lsof` needs no `sudo` here: the daemon runs as the same account.

**M4.** No code in v0.0.7 makes the daemon run `open`, so (a) is simulated. The daemon started by
MixLab runs with `uid = 501`, `asid = 100018` (`launchctl print pid/932`), the same audit session
as a shell in the logged-in desktop. `mix autostart` on macOS is a LaunchAgent in `gui/501`.

- `launchctl asuser 501 /usr/bin/open -a Installer <pkg>` (no `sudo` needed for one's own uid):
  exit 0, Installer.app frontmost afterwards (Claude was before). **Simulated.**
- A transient LaunchAgent bootstrapped into `gui/501`, `RunAtLoad`, running the same `open`:
  exit 0, Installer.app frontmost afterwards (Finder was before). Booted out afterwards.
  **Simulated**, and the nearest thing to a daemon the login item started.
- (b) `ssh localhost` with Remote Login on, while the same user was logged in at the console:
  `open exit=0`, the SSH shell in `asid = 100103`. Installer.app was started (pid 3817) **in the
  desktop session, `asid = 100018`**, not in the SSH one. **The call did not fail.**
- Not measured: SSH to a Mac where the user has no desktop session at all. That needs logging out
  at the console.

## Design

### D1. Which copy takes this path

`mixengine_core::updates::placement` gains a third answer, `Placement::Installer`, which carries the
directory and the receipt identifier. It is asked **first**, before the AppImage check and the write
probe:

1. On macOS, ask `mixengine_platform::install::receipt_of(<daemon exe>)`. If it answers
   `dev.mixengine.cli`, the placement is `Installer`.
2. Otherwise continue with the existing order: the AppImage, then the write probe.

`receipt_of` is new in `mixengine-platform`. On macOS it runs `pkgutil --file-info <path>` and reads
the `pkgid:` line. **The line is the answer, and the exit status is not.** `pkgutil` exits 0 for a
file no package installed, and for a path that does not exist, and then prints only `volume:` and
`path:` (M1). So no `pkgid:` line means `None`, and only a failure to run `pkgutil` at all is an
error. On the other systems it answers `None`, with no error, because they have no `.pkg`
receipts. It is kept separate from `install::packaged_by`: that one answers "who removes this
file", and on macOS it deliberately answers nobody (T88e); this one answers "who installed this
copy".

It is asked once, where the placement is worked out today: when the daemon builds its update state
at start. A call costs about 40 ms (M1), and `update.status` reads the stored placement rather than
asking again.

**A receipt names a path, not a file.** It survives the files it lists: M1 found a 0.0.6 receipt
whose binaries were long gone. A copy unpacked from the `.tar.gz` over `/usr/local/bin` would
therefore be taken for the `.pkg`'s. That is accepted. Installing the `.pkg` is a correct way to
update that copy too, so a wrong answer here costs a few clicks, not a broken install.

Asking the receipt first is not an Intel corner case. `/usr/local/bin` is writable by the account
on any Mac where something handed it to that account: the Intel Homebrew Mac the ADR names, and
the Apple silicon Mac of the readings above, where Homebrew lives in `/opt/homebrew` and
`/usr/local/bin` is still `light:staff`. The write probe passes on both. Step 1 catches both, so
neither is half-updated.

### D2. The feed lists the installer

`packaging/feed.sh` adds an `installers` array to `latest.json`:

```json
"installers": [
  { "os": "macos", "arch": "aarch64", "kind": "pkg",
    "url": ".../mixlab-0.0.9-macos-universal.pkg", "size": 123, "sha256": "..." },
  { "os": "macos", "arch": "x86_64",  "kind": "pkg",
    "url": ".../mixlab-0.0.9-macos-universal.pkg", "size": 123, "sha256": "..." }
]
```

The one universal `.pkg` is listed under both architectures, as the payload already is. The
SHA-256 inside the signed document is what binds the file (T88's D3). `Feed` reads it with
`#[serde(default)]`, so a feed without it still reads, and a copy from before this reads a feed with
it and ignores it. `feed-check.sh` and `.github/scripts/test-feed.sh` learn the new array. The
comment in `feed.sh` that says *"Every payload archive, and never an installer"* changes to say
which installers are listed and why.

### D3. The protocol, added to and never changed

All of it is additive under [ADR 0019](../decisions/0019-an-added-response-member-is-optional.md).
A client from before this sees what it saw before.

- **`UpdateStatus.placement` stays `managed` on the wire** for this copy, with a sentence that is
  still true for an old client. A new tagged variant would make an old client fail to read the
  whole status.
- **`UpdateStatus.installer: Option<UpdateInstaller>`** is new, and present exactly when the
  placement is `Installer`. `UpdateInstaller { kind: "pkg", size }`. A client that sees it offers
  the installer path instead of the refusal.
- **`UpdateStatus.installed: Option<String>`** is new: the version the binary on disk reports while
  it differs from the running daemon's. It means "installed, restart to finish".
- **`update.hand_over { version }`** is a new method. It downloads, verifies and opens the package
  (D4, D5) and answers `UpdateHandedOver { version, package, command }`, where `package` is the
  verified file's path and `command` is `sudo installer -pkg <package> -target /`, written by the
  daemon so no client composes it. The daemon keeps running.
- **`update.finish {}`** is a new method. It is the second half of `update.apply`: stop, record,
  answer, exit (D6). It answers `UpdateApplied` with `replaced` empty, because the installer did the
  replacing, and `restarting` listing what the new daemon will start again.
- `update.apply` on an `Installer` placement is refused with a sentence naming `update.hand_over`,
  as it refuses `Managed` today.

`bindings/` is regenerated (`bash packaging/bindings.sh`). [client-surface.md](../features/client-surface.md)
lists both methods, so `mix` reaches everything MixLab does.

### D4. Download and verify

`update.hand_over` refuses unless the version asked for is the one offered, the placement is
`Installer`, and the feed has an installer for this machine. It then downloads the `.pkg` into
`<home>/cache/updates/<version>/`, where T88 already stages a payload, with the same partial-download
and resume behaviour the payload uses,
and checks its SHA-256 against the feed before anything else touches it. A mismatch deletes the file
and says so. Nothing is stopped at any point in this step.

### D5. Hand it to Installer.app

`mixengine_platform::install::open_installer(path)` is new. On macOS it runs
`/usr/bin/open -a Installer <path>` and returns once `open` has returned; it does not wait for the
installation. The other systems answer `Unsupported`, and nothing calls them.

The daemon records `update.handed_over = { version, at }` in `settings`, then answers.

**`open` answering 0 does not mean somebody saw Installer.app.** Over SSH, with the same user logged
in at the console, `open` exited 0 and Installer.app came up in the desktop session, not in the SSH
one (M4). The person at the SSH prompt may be nowhere near that screen. So the path to the verified
file, and the command that installs it without a window
(`sudo installer -pkg <path> -target /`), are part of every answer, not only of the error:
`UpdateHandedOver.package` carries the path, and `mix` prints both (D8). The file is kept until
`update.finish` or a newer offer clears the handover.

If `open` does fail, the error names the same file and the same command. M4 did not reach that
case. It needs a Mac where the user has no desktop session at all, and the design does not depend
on what `open` says there.

**Nothing clears the quarantine attribute, because none is set.** Gatekeeper judges a package only
when it carries `com.apple.quarantine`. A browser adds it, and macOS then refuses the unsigned
`.pkg` with no way to open it from the dialog. `curl` adds nothing, and Installer.app opened the same
unsigned file with no dialog at all (M2). The daemon downloads with `reqwest`, which, like `curl`,
has no reason to set the attribute. That is inferred, not measured, and the by-hand test checks it.
MixLab never downloads the package itself: only the daemon does, so the file never passes through
an application that could quarantine it. If a later macOS ever does quarantine it, this design is
not finished. Clearing the attribute would bypass Gatekeeper on purpose, and that needs its own
sentence in ADR 0050 first.

### D6. Detect the install, then finish

While `update.handed_over` is recorded, `update.status` runs the daemon's own path with `--version`.
The file at that path is the new binary once the installer has replaced it. The reading is cached
by the file's device and inode, so a status poll costs one `stat` after the first run. When the
answer equals the recorded version, `installed` carries it.

**Not by its modification time.** Installer.app writes each file under a new inode, but keeps the
modification time the file had in the package, which is when it was built (M3: `Sep 23 22:37`
before and after). A reinstall of the same build changes no time at all. Two releases built with
normalised times would not change it either. The inode changed for every file at every install,
so it is the key.

`update.finish` then does what `update.apply` does after its swap:

1. Stop everything in reverse dependency order (`stop_everything`).
2. `remember(to, stopped)`, so the new daemon's `restore_after_update` starts them again.
3. Clear `update.handed_over`, remove `<home>/cache/updates/<version>/`, answer, and exit.

M3 measured the assumption this relies on. With the daemon, MixLab and four services running,
Installer.app replaced every file, and the daemon kept its pid and ran on the old, unlinked
`mixengined`. It went on stopping and starting services, and the window stayed usable. The
services were not touched. So `update.finish` runs its stop order on the old image, which is the
code that recorded the handover, and that is what it should be.

The shims catch up at the next start, not at the install. The ones in `<home>/bin` are hard links
to `mixengine-shim`, so after the install they still name the old file (M3: 28 links on the old
inode). The new daemon's start runs the shim refresh, finds them not current, and links them to the
new file. Nothing here needs to add to that, but the by-hand test checks it.

A recorded handover is also cleared when the running daemon's own version equals it (somebody
installed and restarted another way), and when a newer release is offered.

### D7. MixLab

`UpdatesSection` gains the installer branch:

1. If `installer` is present, the button reads **Update** and states the size, as today. It calls
   `update.hand_over` and shows *"Installer is open. Finish the installation there."*
2. While that message is shown, it polls `update.status` every few seconds. When `installed`
   appears, it offers **Restart to finish**.
3. **Restart to finish** calls `update.finish`, then decides whether to relaunch the window.
   `relaunch::relaunch_after_update` gains a second rule for this path: relaunch when the bundle
   this window was started from now reports a different `CFBundleShortVersionString` in its
   `Info.plist` than the running window. The path was read at start (T106), so it is still
   `/Applications/MixLab.app`. Installer.app replaces the bundle's directory itself, not only the
   files in it (M3: a new inode for `MixLab.app`), but at the same path, so reading `Info.plist`
   there reads the new one.
4. After the relaunch the window finds no daemon and shows **Start**, exactly as after T88's
   update. It does not start the daemon itself.

Strings go through `i18n` in English and Vietnamese, per `writing-user-facing-text`.

### D8. `mix`

- `mix self-update`, on an `Installer` placement, shows the same offer and consent prompt, then
  calls `update.hand_over` and prints: the installer is open on this Mac's screen, and
  `mix self-update --finish` completes the update once it is done. It **always** prints the
  verified package's path and `sudo installer -pkg <path> -target /` as the way to install it
  without the window, because a person running `mix` over SSH may not be at that screen, and
  `open` succeeding does not tell the daemon who is (D5, M4).
- `mix self-update --finish` calls `update.finish` and then starts the new daemon, as
  `mix self-update` does today after `update.apply`. It refuses with a sentence when `installed` is
  absent.
- `mix self-update --check` prints `installed` when it is there.

### D9. What does not change

- The `.pkg` has no scripts and gains none.
- `mixengine-elevate` and every `PrivilegedOp` are untouched. The helper moves forward because the
  `.pkg` writes it, as it does on a first install.
- `elevation.upgrade` still refuses a managed placement. `Installer` counts as managed for it.

## Testing

- **`mixengine-core`:** placement ordering with a mock receipt (receipt first, then the AppImage,
  then the probe); a feed with and without `installers`; the handover record through hand over,
  install detected, finish, and each way of clearing it.
- **`mixengine-daemon`:** `update.hand_over` against a local feed and a mock `open_installer` that
  records its call; a SHA-256 mismatch deletes the file and opens nothing; `update.finish` restores
  services on the next start. The mock platform stands in for macOS, so these run on every system.
- **`mixengine-platform`:** `receipt_of` parses `pkgutil --file-info` output held in fixtures: one
  with a `pkgid:` line, and the two-line answer with none, which is what a foreign file and a
  missing path both produce with exit 0 (M1). `open_installer` answers `Unsupported` off macOS.
- **`mixengine-daemon`, the install check:** a status poll after the file at the daemon's path is
  replaced under a new inode with the **same** modification time still re-reads `--version`.
- **`packaging`:** `feed-check.sh` rejects a feed whose macOS rows lack an installer.
- **By hand on a Mac, before release:** install v0.0.8 from its `.pkg`, point the daemon at a test
  feed (`--update-url`, `--update-key`) that offers a v0.0.8-plus build, and press **Update** in
  MixLab. Before going through Installer.app, check `xattr -l` on the file in
  `<home>/cache/updates/<version>/`: no `com.apple.quarantine` (D5). Go through Installer.app, press
  **Restart to finish**, and check that every binary, the window and the helper report the new
  version, that the services came back, and that `<home>/bin/php` now has the inode of
  `/usr/local/bin/mixengine-shim` (D6). Then run it once more, pressing Cancel in Installer.app, and
  check that nothing changed. Run `mix self-update` once over SSH, and check that it prints the
  package path and the `installer` command.

## Checked by hand, 2026-10-07

Mac14,3 (Apple silicon), macOS 15.7.3, starting from a `.pkg` install of v0.0.14: receipt 0.0.14,
and a daemon still running the 0.0.13 image the 0.0.14 install had replaced, which is the state M3
describes. Two rounds, because since [T187](2026-09-26-t187-mixlab-updates-itself-design.md) the
window and `mix` are two paths: MixLab's own updater downloads into its `updates/<version>/` and
reads neither `--update-url` nor `--update-key`, so Settings → Updates was walked against the real
feed, 0.0.14 → 0.0.15; `mix self-update` was walked against a local feed signed with a throwaway
key, 0.0.15 → a 0.0.16 built from this checkout with `MIX_MACOS_SLICES=aarch64`.

**Round 1, Settings → Updates, 0.0.14 → 0.0.15, the real feed.**

- *Download*: `mixlab-0.0.15-macos-universal.pkg`, 54,031,244 bytes, under
  `~/Library/Application Support/io.github.mixnz.mixlab/updates/0.0.15/`, SHA-256 equal to the
  feed's `macos`/`aarch64`/`pkg`/`window` row, and **no extended attribute at all**: no
  `com.apple.quarantine` (D5).
- *Open installer*, then **Cancel** in Installer.app: receipt 0.0.14; every inode as before
  (`mixengined` 117489086, `mix` …087, the shim …088, the helper …092, `MixLab.app` …094); the daemon
  still pid 81670; caddy 81719 and redis@main 85401 running; the package and its `ready` marker
  kept. Nothing changed.
- *Open again*, installed: receipt 0.0.15; new inodes (…101, …103, …104, …108, …110) with
  **modification time `Oct 6 14:43`, when the package was built**, not when it was installed (D6);
  every binary, `Info.plist` and the helper's `pkg-version` 0.0.15; the daemon still pid 81670 on
  the unlinked inode …086 (`lsof`); services untouched; `<home>/bin/php` still on the old shim inode
  …088, the new shim with one link. The pane read `Info.plist` and offered *Finish*.
- *Finish*: daemon 0.0.15 (pid 92313) on inode …101; caddy and redis@main back; MixLab relaunched
  (pid 92326) on inode …115; `<home>/bin/php` on the new shim inode …104, 20 links; helper 0.1.4,
  the version both releases ship. The Keychain asked once for the `MixLab` item, which an ad-hoc
  signed build costs at every update (`apps/desktop/CLAUDE.md`).

**Round 2, `mix self-update` over SSH, 0.0.15 → 0.0.16, the test feed.**

- The daemon started with `--update-url http://127.0.0.1:8765/latest.json --update-key <test key>`;
  `mix self-update --check` offered 0.0.16 "through Installer.app". A feed `feed.sh` writes from a
  one-slice build names the helper `macos-arm64`, which `Arch` refuses (`unknown variant arm64`):
  corrected by hand, and not a release case, since a tag is always universal (T171).
- `ssh localhost mix self-update --yes` printed *the installer is open*, the package path under
  `<home>/cache/updates/0.0.16/` and the `sudo installer -pkg … -target /` line, exit 0, and opened
  nothing in the SSH session; Installer.app came up on the desktop, in the same audit session
  (100020) as MixLab and the daemon (D5, M4). The package carried `com.apple.provenance` and no
  quarantine attribute. A second `update.hand_over` reused the verified file instead of downloading
  it again.
- Installed through Installer.app: `--check` printed *0.0.16 is installed. finish it: mix
  self-update --finish*. `--finish` stopped the daemon and `mix` started the new one: daemon 0.0.16,
  every binary and the helper's `pkg-version` 0.0.16, shims relinked, `cache/updates/0.0.16`
  removed, and `--check` answering *not newer than the 0.0.16 this machine is running*.

**Two things diverged, both outside this design, both fixed with the check.**

1. `mix self-update` answered *Broken pipe* to an `i` typed after 35 seconds, over SSH and at the
   console alike. `mix` holds one HTTP/1.1 connection per run, and the daemon's header-read timeout
   of 30 seconds applies to every request on a kept-alive connection, so the connection was closed
   while the person read the offer. `mix elevation grant` and the server-switch prompt sat on the
   same connection. `Client::call` now dials again after an idle of half that timeout. `mix`
   keeps its own copy of the value, pinned by a test that reads the daemon's source, rather than a
   constant in `mixengine-proto`: the helper is built from every file of that crate, and a constant
   it never uses would still move `HELPER_VERSION` and cost every machine an elevation prompt.
   Checked again with the fixed `mix` and a feed offering 0.0.17: 35 seconds at the prompt, then
   the handover printed.
2. `update.finish` and `update.apply` remembered every service the stop walk reached, so the new
   daemon started six services where two had been running, and `--check` had said two. The walk
   takes the whole graph and reports an already-stopped service as stopped. Both now record only the
   services that were running before the stop (`updates::restorable`), which is what T88 promised.

The Mac stays on the 0.0.16 build until the fixes have been checked against it; the released
`.pkg` is then installed again by hand.

## Documentation, when it lands

- [features/updates.md](../features/updates.md): the `.pkg` path, and the rule ADR 0050 narrows.
- [features/client-surface.md](../features/client-surface.md): the two methods.
- The handbook's update page, in English and Vietnamese.
- `CHANGELOG.md`: from this release, a Mac that installed the `.pkg` updates from MixLab or
  `mix self-update`, after one install by hand.
