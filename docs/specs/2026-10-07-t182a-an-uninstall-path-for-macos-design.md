---
status: implemented
date: 2026-10-07
task: T182a
---

# An uninstall path for macOS, and the shape for Linux

[T182](2026-09-24-t182-removing-mixlab-is-one-act-design.md) made removing MixLab one act on
Windows, where an uninstaller exists to carry it. macOS has none: a `.pkg` installs and never
uninstalls, and what it placed is root-owned. So the handbook still tells a Mac user to run
`mix uninstall` and then `sudo rm -rf` four paths by hand, and
[ADR 0051](../decisions/0051-an-uninstall-ends-what-it-undoes.md) left that order standing *"until
those formats have an uninstall path of their own (T182a)"*. This is that path.

## Goal

A person removes MixLab from a Mac the `.pkg` installed in one act, from the application itself:
they see what will be undone and what is in the way before anything changes, choose whether their
data goes, allow one administrator prompt, and the application quits with nothing of MixLab or
MixEngine left on the machine. A headless `.pkg` install gets the same removal from `mix`. The two
promises of T182 hold: **P1**, an uninstall can always be finished, and **P2**, what blocks it is
found before anything is removed.

## Not in scope

- **Authorities from other homes in the trust store.** An uninstall removes its own home's
  `MixEngine Local CA` and no other; this Mac holds three. Which of them an uninstall may claim is
  roadmap task **T182c**, listed beside this one in
  [phase 9](../roadmap/phase-9-ship.md).
- **A window entry point on Linux.** The `.deb` and the `.rpm` have a package manager to remove
  them, and ADR 0048 makes that the package manager's job. D6 gives Linux its shape in `mix` and
  leaves the window's item to a follow-up row.
- **Windows.** T182's uninstaller is unchanged. The new query field is ignored there (D3).
- **Signing the `.pkg` or the application**, and the Keychain prompt an ad-hoc signed build
  costs. Unchanged by this.

## Measured on this Mac, 2026-10-07

Mac14,3, macOS 15.7.3, MixLab v0.0.15 installed from `mixlab-0.0.15-macos-universal.pkg`, with a
home that has run sites, a shared site and three runtimes.

**What the `.pkg` places, as root** (`pkgutil --files dev.mixengine.cli`, 21 entries):

| Path | Owner | Who removes it today |
| --- | --- | --- |
| `/usr/local/bin/mix`, `mixengined`, `mixengine-shim`, `mixengine-trampoline` | `root:wheel` | nobody: the handbook's `sudo rm` line |
| `/Applications/MixLab.app`, with `Contents/Resources/mixengine-elevate` inside it (T88d) | `root:wheel` | nobody: the same line |
| `/Library/PrivilegedHelperTools/dev.mixengine.elevate` | `root:wheel` | `mix uninstall`, row *privileged helper* (ADR 0048, rule 3) |
| the receipt `dev.mixengine.cli` in the package database | — | nobody: `pkgutil --forget` is not in the handbook |

On this Mac `/usr/local/bin` itself is `light:staff`, so its four files could be unlinked without
root; on a Mac where it is `root:wheel` they cannot. `/Applications` is `root:admin` and
group-writable, but a bundle's inner directories are `root:wheel` 755, so removing the bundle needs
root either way. Finder asks for an administrator's password to move such an application to the
Trash.

**What `mix uninstall --dry-run` plans**, on the same machine: the hosts block (nothing there), the
resolver files in `/etc/resolver` (`internal`, `localhost`, `test`), the packet-filter anchor, its
block in `/etc/pf.conf` and the boot-time job `/Library/LaunchDaemons/dev.mixengine.pf.plist`
(ADR 0012), the firewall rules (nothing), this home's certificate authority in the System keychain,
the privileged helper, the audit log `/Library/Logs/MixEngine/elevate.log`, the login entry
(nothing), the `PATH` entry in two files, MixLab's data directory, three cache and log directories,
four service passwords and MixLab's own credentials, the home, and four `BLOCKED` rows for `node`
processes running out of `runtimes/`. Exit 3. **Nothing in the plan names the four binaries, the
application bundle or the receipt.** Those three are what this design adds, and the helper is
already a row.

## The two promises, here

- **P1.** Every step is idempotent and the application, the receipt and the menu item are the last
  things to go: a removal that stops part-way leaves a MixLab that can be opened and asked again,
  and the next run reads *nothing there* for what the first one did.
- **P2.** The plan, with its `BLOCKED` rows, is shown before anything changes, and the package's
  files are removed only after every other privileged row has been removed, so a failure in the
  batch leaves the program in place to retry from.

## Decisions

### D1. The act lives in the application menu, and that is the uninstaller's entry point

**MixLab ▸ Remove MixLab from this Mac…** in the application menu, macOS only, shown only when the
window was placed by the `.pkg` (`updater::placement` already answers `Installer { installer: "pkg" }`
from the receipt, D3 of [T187](2026-09-26-t187-mixlab-updates-itself-design.md)). It opens one
dialog, the *Remove MixLab* dialog (D4), which runs the whole removal and quits.

**Why the menu and not Settings.** The application menu is where macOS puts the acts that are about
the application itself: About, Settings, Quit. Settings is where ADR 0051 just removed a section,
and a row there would read as that section coming back.

**How this squares with ADR 0051, decision 2.** What the ADR removed was a Settings section that
undid MixEngine and left the window running on a machine that could no longer elevate. It then
assigned the act to *the uninstaller*: on Windows, to the program the installer leaves for it. A
`.pkg` leaves no such program, so on macOS the only place an uninstaller can live is inside the
application bundle, and the only way to reach it is from the application. The dialog is the
uninstaller, drawn by the window: it removes the window with everything else and ends with the
application gone, which is the opposite of the state the ADR refused. The ADR anticipated this in
the same sentence (*"until those formats have an uninstall path of their own (T182a)"*), so no
amendment is needed and none is written. [client-surface.md](../features/client-surface.md) item 7a
gains the sentence that says so.

**The menu is built in Rust.** Tauri gives macOS a default application menu; MixLab has never
replaced it. `Menu::default` is taken as it is, the item is inserted into the first submenu before
*Quit*, and `on_menu_event` emits one event the shell listens for. The Edit submenu, which is what
makes ⌘C and ⌘V work in a webview on macOS, is kept because the default is kept.

### D2. Removing the package's files is one more operation of `mixengine-elevate`

Two ways were weighed for the root-owned files:

- **`do shell script "rm -rf …" with administrator privileges`, run by the window once.** A second
  prompt beside the daemon's, and a shell command composed by a client and run as root: exactly
  the `Exec { cmd }` shape [ADR 0018](../decisions/0018-a-signed-candidate-is-what-lets-a-path-cross-the-boundary.md)
  and the security model refuse, and the window elevating, which nothing in the product does.
  Rejected.
- **`PrivilegedOp::PackageRemove {}`**, a new operation of the helper, carrying no fields, on the
  pattern of `HelperRemove {}` and `AuditLogRemove {}`. Chosen.

The operation removes, in this order, and reads *absent* for anything already gone:

1. `/usr/local/bin/mix`, `/usr/local/bin/mixengined`, `/usr/local/bin/mixengine-shim`,
   `/usr/local/bin/mixengine-trampoline` — the names `packaging/common.sh` lists, compiled in;
2. `/Applications/MixLab.app`, **only after reading its `Info.plist` and finding MixLab's own
   `CFBundleIdentifier`** (`io.github.mixnz.mixlab`); a bundle of any other identity at that path is
   left alone and reported as kept, with the identifier it had;
3. the receipt, `pkgutil --forget dev.mixengine.cli`.

**Every path is a constant, and the request carries none**: this is what lets the helper validate
the operation entirely on its own, as ADR 0018 requires, and what keeps a daemon that has been
talked into anything from naming a path. The constants live in `mixengine-platform`'s macOS install
module beside `helper_path()` and `helper_sources()`, which already spell `/usr/local/bin` and
`MixLab.app`; the helper's operation takes them from its dispatch as parameters, so its tests pass a
temporary directory the way the audit-log and helper tests do. They are the paths
`packaging/macos/build.sh` places, and a test in `mixengine-core/tests/packaging.rs` holds the two
lists equal, as it already does for the binary names. On Windows and Linux the operation answers
`Unsupported`, and nothing enqueues it there.

**The helper changes, so `HELPER_VERSION` moves** (`bash packaging/helper-lock.sh --bump`, to
0.1.5). [T182b](2026-09-25-t182b-a-helper-that-keeps-up-and-an-uninstall-that-finishes-design.md)'s
flow delivers it: the daemon queues `HelperReplace` first in the uninstall's batch, so the helper that
runs the batch is the one that knows `package-remove`; an installed helper too old to read the batch
at all is answered by the copy inside the application (D3 of T182b). The `.pkg` places the new
helper from the first release carrying this, so a machine installed at that release or later needs
no replacement.

**The helper removing the program that holds its own source is fine.** `HelperRemove` already
removes the file the elevated process is running from; a Unix unlink does not touch a running
image. Order inside the batch: `PackageRemove` after every other privileged row, then `HelperRemove`,
then `AuditLogRemove`, as today.

### D3. The package is a row of the plan, asked for by the query

`UninstallQuery` gains `package: bool` (`serde(default)`, `false`), beside `keep_home` and
`keep_relocated`. With it, the plan gains one row, `ResidueId::Package`, whose `location` is the
receipt and whose text names the four files, the bundle and the receipt, and `daemon.uninstall`
enqueues `PackageRemove` with the other privileged rows. Without it, nothing changes anywhere, so
every caller from before this reads what it read before
([ADR 0019](../decisions/0019-an-added-response-member-is-optional.md)).

- **Every plan on a copy an installer placed carries the row; only `package: true` on a `.pkg`
  copy plans it.** The daemon's placement, read from the receipt at start (T88f), decides: a `.pkg`
  copy gets `Planned` with `package: true`, `Absent` when its files are already gone, and `Kept`
  naming `mix uninstall --package` without the flag; a `.deb` or `.rpm` copy gets `Kept` naming
  `sudo apt remove <package>` or `sudo dnf remove <package>`; a Windows copy gets `Kept` saying the
  Windows uninstaller removes the program. A copy no installer placed has no row. That makes D5's
  hint and D6's printed command one row, which `mix` and the window render, with nothing enqueued
  outside the `.pkg` case.
- **It goes last among the privileged rows, and it is settled like them** (T182b, D7): still there
  afterwards is `Failed`, the uninstall is not finished, the daemon stays up, and the same command
  finishes it next time (P1).
- **Blocked rows are unchanged.** T182's D4 names a process running from a directory the home
  removal would take. The four binaries are not added to that search: a `mix` in another terminal
  or the window running this very removal are processes running from the package, a Unix unlink
  leaves them running, and nothing breaks in a person's hands the way a `php` from `runtimes/` would.
  A second MixLab is not possible: the window is single-instance.
- **Both flavours, one row.** The headless `.pkg` shares the receipt and places no bundle; the
  operation reads the bundle as absent and removes the rest.

`bash packaging/bindings.sh` regenerates `bindings/`; `mix uninstall` gains `--package` (D5).

### D4. The window runs it in this order, and never outlives it

The *Remove MixLab* dialog is the shell's, not the `mixengine` module's: a person who never started
MixEngine removes MixLab too. The shell reaches the daemon through one Rust seam,
`modules::mixengine::for_uninstall`, on the pattern of `for_update`: the frontend knows no
MixEngine concept, and the seam calls `daemon.uninstall_plan`, `daemon.uninstall`, follows the job
and waits for the process.

1. **Open.** The dialog asks the seam for the plan with the two choices as they stand. **The seam
   starts the daemon when none is running.** This is the one job the window starts MixEngine for
   on its own, because the person has asked for MixEngine's traces to be undone and only the
   daemon knows them; `mix uninstall` does the same, against an empty home when there was none,
   which is idempotent and slow rather than wrong (T182, D7). The dialog says *Checking what is on
   this Mac…* while it waits.
2. **Show, changing nothing (P2).** The rows, in the plan's order, as the Windows choices page
   draws them: what will be undone, with *nothing there* rows dimmed, and every `BLOCKED` row at the
   top, naming the program, its pid and the folder, with *Check again* re-reading the plan. Two
   checkboxes, **both unticked**: *Also delete MixLab's data* (the home's path, and that it holds
   the databases, certificates and projects' records) and *Also delete the folders you moved out
   of it* (listing each, shown only when there is one). **Remove MixLab** is disabled while any row
   is blocked.
3. **Stop writing, and take back what is the window's own.** Before the act, the window sets itself
   *leaving*: nothing is saved on exit from here on, not the connections, not the window state, not
   the vault. The handbook today warns that an open window *"saves its passwords again after they
   are removed"*; a removal run from the window is where that would otherwise happen. It also turns
   its own login item off (`login_item`, a LaunchAgent `tauri-plugin-autostart` wrote): that entry
   is MixLab's, not a row of MixEngine's plan, and left behind it would start a bundle that no
   longer exists at every login.
4. **Remove.** The seam calls `daemon.uninstall { keep_home, keep_relocated, grant: true,
   package: true }`. The daemon raises its one prompt; a declined prompt changes nothing
   (ADR 0051, decision 3; T182b, D6), the dialog says so, and the window stays with the item still
   in its menu (P1). The dialog follows the job and draws each row as it settles.
5. **Wait for the daemon to have gone**, by pid and start time (T182b, D8), up to the same
   120 seconds `mix` allows. The batch unlinks the daemon's own `mixengined`, and the daemon exits
   when the home is gone; the window, like `mix`, reads the ending off the process rather than off
   the connection.
6. **Read back, then quit.** The window checks the two things it can see for itself: its own
   bundle path (`relaunch::origin().root`, read at start, so still `/Applications/MixLab.app`) no
   longer exists, and `pkgutil --pkg-info dev.mixengine.cli` no longer answers. It then removes once
   more, with its own hands, the directories of its own the report's `WindowData` and `WindowCache`
   rows named: a webview writes to its storage without being asked, so one of them may have come
   back while the dialog was drawn. Both gone: the application exits, with nothing of it left to
   open. Anything still there, or a row `Failed`: the dialog names it and the window stays open,
   with the menu item, so running it again finishes (P1). A kept home or kept folders are shown as
   kept, not as failures.

**When no daemon can be started** (the binaries broken, the socket refused), the dialog says so and
shows the two terminal lines the handbook gives for that Mac, `mix uninstall --package` and the
`sudo` line; nothing is removed.

The window is removed by step 4 while it is running; macOS keeps the mapped image until the
process exits, which is what T88f's M3 measured for Installer.app replacing the bundle. The
dialog's last screen is drawn from memory, and the exit in step 6 writes nothing.

### D5. A headless `.pkg`, and any Mac from a terminal: `mix uninstall --package`

`mix uninstall --package` sets the query's `package`, and `--dry-run --package` shows the row with
the rest. The removal is the same batch, raised by the daemon; `mix`, running from
`/usr/local/bin/mix`, is unlinked by it and finishes its read-back from memory, as it finishes today
after the daemon has gone. After a finished run it reads back the four paths, the bundle and the
receipt, and reports any that remain.

- **Without `--package`**, on a copy the receipt names, `mix uninstall` finishes as it does today
  and adds one line: *to remove the program as well: mix uninstall --package*. The documented
  meaning of `mix uninstall`, *undo what MixLab did*, does not change under anyone's feet.
- **A Mac with no desktop session** cannot raise the prompt, so the privileged rows fail there
  today and fail the same way with this; nothing in MixEngine elevates another way. The handbook
  keeps the `sudo` line for that machine, and gains `sudo pkgutil --forget dev.mixengine.cli` at
  its end, which it never had.

This keeps *no client-only capability* without a new method: the window and `mix` ask the same
`daemon.uninstall` with the same field.

### D6. Linux: `mix` prints the package manager's command; the window's item is a follow-up

A `.deb` or an `.rpm` is the package manager's to remove (ADR 0048), and a `prerm` that runs as root
cannot reach a per-user daemon (T182, *Out of scope*). So the shape is the handbook's order, made
to print itself: after a finished `mix uninstall` on a copy `install::packaged_by` names,
`mix` prints `sudo apt remove mixlab` or `sudo dnf remove mixlab`, with the package's name as the
database spelled it, the way `mix self-update` prints the install command on those systems
(T182b, D5). `--package` on Linux plans the row as `Kept` with that command as its reason (D3).

A *Remove MixLab…* item in the Linux window would have to run `pkexec apt remove mixlab`, or hand
the package to the software centre, and either is a design of its own. It is a follow-up row,
**T182j**, added beside this one in phase 9, and the handbook's Linux order stands until then.

### D7. What stays behind, on purpose

- **Other homes' authorities in the trust store** — T182c, above.
- **Nothing else.** The Keychain items (`WindowCredentials`, `Credentials`), the window's data and
  caches, the login entry and the `PATH` entry are rows already; the receipt, the bundle and the
  four binaries are this design's. A person who keeps the home keeps it knowingly, through the
  first checkbox, and a later install picks it up (T182f–h).

## MixLab

The window does it, from **MixLab ▸ Remove MixLab from this Mac…**, through the *Remove MixLab*
dialog of D4: the plan with its `BLOCKED` rows and the two choices before anything changes, one
administrator prompt, the rows settling, and the application quitting once its bundle and the
receipt are gone. The item exists only on a macOS copy the `.pkg` placed; a development build, a
Windows or Linux window has no such item. `daemon.uninstall_plan` and `daemon.uninstall` leave
`cliOnly` in `apps/desktop/client-surface-exceptions.json`, because the window calls them now, and
`node scripts/check-client-surface.mjs` fails on an entry that no longer applies, so the two lines
go with the change.

The window's `mixengine` module stays a client: the seam calls the two methods and the job stream
through `rpc::call`, typed against `bindings/`, and its Rust depends on `mixengine-proto` and
`mixengine-platform` only (`tests/layering.rs`). Reading `pkgutil --pkg-info` and its own bundle
path in step 6 is the window's own, as the updater's placement probe already is.

## Testing

- **`mixengine-elevate`:** `PackageRemove` against paths under a temporary directory, as the
  audit-log and helper tests are written: removes the four files and the bundle, reads *absent* on
  a second run, leaves a bundle whose `Info.plist` names another identifier and says which, and
  forgets the receipt through a `pkgutil` the test can observe; `Unsupported` off macOS; the
  operation is in the allow-list under `package-remove`.
- **`mixengine-core/tests/packaging.rs`:** the helper's path constants equal what
  `packaging/common.sh` and `packaging/macos/build.sh` place.
- **`mixengine-daemon`:** the plan has the row only with `package: true` and only on a receipt the
  mock platform names; `Kept` with each reason on Windows, Linux and an unpackaged macOS copy; the
  row is enqueued last among the privileged ones; a `Failed` package row leaves the uninstall
  unfinished and the daemon up.
- **`mixengine-cli`:** `mix uninstall --dry-run --package` renders the row; `mix uninstall` on a
  receipt-named copy prints the `--package` hint; a finished uninstall on a `dpkg`/`rpm` copy
  prints the remove command (mock platform).
- **The desktop crate:** the seam's wait by pid and start time; the leaving flag stops every write
  on exit (a test like `sync_and_the_connections_are_one_visit_to_the_store`); `layering.rs` stays
  green; the dialog's state function over each state (plan, blocked, prompting, removing, failed,
  gone), in vitest.
- **`packaging/helper-lock.sh --check`** says the helper moved, and the bump is committed with it.
- **By hand on this Mac, from a real `.pkg` install**, recorded in
  `packaging/macos/uninstall-check.md` the way `packaging/windows/uninstall-check.md` records the
  Windows walk: install, create a site with a running service, start `node` from a shim so the
  plan shows a `BLOCKED` row, open the dialog, see the row, close `node`, *Check again*, tick
  nothing, Remove, allow the prompt; the application quits. Then `ls /usr/local/bin`,
  `pkgutil --pkg-info dev.mixengine.cli`, `/etc/hosts`, `ls /etc/resolver`,
  `security find-certificate -c "MixEngine Local CA"`, `ls /Library/PrivilegedHelperTools` and
  `ls ~/Library/LaunchAgents`. Once more declining the prompt: nothing changed, the item still
  there. Once more from a terminal over SSH: `mix uninstall --dry-run --package` shows the row.

## Checked by hand, 2026-10-07

Mac14,3, macOS 15.7.3, starting from the released v0.0.15 `.pkg` with a home holding two Laravel
sites, caddy and redis running. The walk used a `.pkg` built from this branch
(`MIX_MACOS_SLICES=aarch64`, still 0.0.15, helper 0.1.5) installed over the release; the daemon
reported the installed helper as 0.1.5 at its first start, with no prompt (T182b, D2's first row).

**The item and the plan (D1, D4 steps 1 and 2).** *MixLab ▸ Remove MixLab from this Mac…* sits
above Quit with a separator of its own. The dialog listed every row of the plan in the daemon's
order, the kept rows dimmed, the `package` row reading *the program itself: its commands, the
MixLab application and the package receipt* at `dev.mixengine.cli`, and one checkbox, since the
home has no relocated directory. With the data box unticked nothing was blocked: a kept home is not
searched (T182, D4). Ticked, five `node` processes running from `runtimes/node/24.21.0/bin/node`
(one started for the walk, four from this editor's Playwright plugin, which this Mac's `PATH`
resolves to MixEngine's `node`) appeared as banners with their pids, and **Remove MixLab** was
disabled. Closing them and *Check again* cleared the banners.

**The prompt declined (D4 step 4, P2).** The daemon raised its one prompt; Cancel left the receipt
at 0.0.15, the four binaries, the bundle, the helper, the resolver files, the authority, the daemon
(same pid) and the window (same pid) as they were, and the queue held only the three first-start
operations the daemon itself had queued (T182b, D6 restored it). Before the fix below the dialog
read the report as a failure, because after a decline the daemon settles the privileged rows as
`failed`, *still waiting for permission* (T182b, D7), and offered no way to try again; after it the
dialog says *You didn't allow the prompt, so nothing was removed. MixLab is still installed.* and
offers *Check again*. The MixEngine tab's own *MixEngine needs an administrator* dialog was drawn
over the removal while the queue held its operations; the removal dialog now sits above it.

**A finished run (D4 steps 4 to 6, P1), twice.** With the data box unticked: the application quit
on its own within seconds of the prompt. Afterwards `/usr/local/bin` held no `mix*`,
`/Applications/MixLab.app` was gone, `pkgutil --pkg-info dev.mixengine.cli` answered *No receipt*,
`/etc/hosts` and `/etc/pf.conf` named nothing of MixEngine's, `/etc/resolver` kept only another
product's `lc`, the pf anchor and `/Library/LaunchDaemons/dev.mixengine.pf.plist` were gone, the
helper was gone, no LaunchAgent, no `mixlab` or `mixengined` process, and the System keychain held
two *MixEngine Local CA* authorities where it had held three: this home's went, the other two are
T182c's. The home (956 MB) and the window's data directory stayed, as the box said. **The first run
left four six-byte lock files in `/Library/Logs/MixEngine`** (`hosts.lock`, `resolver.lock`,
`trust.lock`, `port-access.lock`), written by the helper's own operations beside the audit log and
never removed by any uninstall before this; `audit-log-remove` now takes the helper's `.lock` files
with the log, and the second run left no directory at all.

**The data box ticked (D4 steps 3 to 6, T182's D4), on a later build.** Same home, caddy and redis
running, the box ticked, the prompt allowed. Before the fixes below the two *credential* rows,
`mariadb@main/root` and `mariadb@main/laravel-2`, settled as `failed` and the window stayed with
the report, three faults deep. First, the T186 vault answered nothing for an item an older daemon
had kept flat in the Keychain under `<home>/<rest>`, so the store never tried to forget it; the
vault now reads and forgets a listed flat item too. Second, the keyring crate's delete reads the
secret before it deletes, which runs the item's access control and, for an item another build
wrote, fails with an access prompt the daemon cannot answer (*UNIX[No such file or directory]*);
the macOS store now deletes by service and account, which asks nothing. Third, `SecItemDelete`
refuses an item another program wrote with `errSecInvalidOwnerEdit`, which is what every item the
window or an earlier build created is; the store now hands that one refusal to
`/usr/bin/security delete-generic-password`, which this Mac let through without a prompt. The
credential row's failure also quotes the store's whole error chain now, where before it read only
*the credential store failed*. With all three, the run finished: every row `removed`, the
application quit on its own, the home (956 MB) and the window's three directories
(`Application Support`, `Caches`, `WebKit` under `io.github.mixnz.mixlab`) were gone, the
Keychain held neither `MixLab/vault` nor this home's two `mixengine` items, `/etc/resolver` kept
only `lc`, no helper, no log directory, no LaunchAgent, no process, and the two other homes'
authorities stayed (T182c). The Keychain also holds some thirty `mixengine` items under home ids
no install here ever had (`<id>/extensions/phpmyadmin/config`), written by a test run that used a
temporary home against the real Keychain; this walk left them, and they are a task of their own.

**From a terminal over SSH (D5).** With the test build: `mix uninstall --dry-run --package` showed
the `package` row as `would`; without the flag it showed `kept`, *it stays unless asked for:
`mix uninstall --package` removes it as well*. The released 0.0.15 `mix`, tried first by mistake,
refused `--package` as an unknown argument, which is what an unreleased flag does.

**Reinstall.** The released `.pkg` installed again over the empty machine; the daemon started on the
kept home with both sites listed and queued the resolver, packet-filter and authority operations
for the next prompt, as a first start does.

## Documentation, when it lands

- [client-surface.md](../features/client-surface.md), item 7a: the macOS entry point, and that
  the two methods are the window's as well as `mix`'s.
- The handbook's removal page, English and Vietnamese: the macOS section first, the menu item, the
  `--package` flag, `pkgutil --forget` on the `sudo` line, and Linux's printed command.
- `CHANGELOG.md`, `### Added`: *On a Mac, MixLab ▸ Remove MixLab from this Mac… removes everything in
  one go, after showing you what it will undo; `mix uninstall --package` does the same from a
  terminal.*
- [phase 9](../roadmap/phase-9-ship.md): T182a ticked, T182j added beside it.
