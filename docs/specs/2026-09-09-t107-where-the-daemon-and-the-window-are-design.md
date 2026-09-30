---
status: implemented
date: 2026-09-09
task: T107
---

# T107 — Where the daemon is, and where the window is

Roadmap task [T107](../roadmap/phase-12-one-product.md), on
[the desktop client design](2026-09-08-the-desktop-client-in-this-repository-design.md)'s D9 and
D10, and on [T106](2026-09-09-t106-one-updater-design.md). 2026-09-09.

Two questions are answered by hand in this repository today, in two places that do not know about
each other.

The first is **where MixEngine is**. `apps/desktop/src-tauri/src/modules/mixengine/health.rs` keeps
a `well_known()` returning `%LOCALAPPDATA%\Programs\MixEngine\mixengined.exe` on Windows and
`/usr/local/bin`, `/usr/bin` on the two Unixes, with a comment explaining that `PATH` alone is not
enough — measured, on the machine that wrote it. It is right, and it is a copy of a fact the
packaging scripts own: `packaging/windows/mixengine.nsi` writes `InstallDir`,
`packaging/macos/build.sh` writes `/usr/local/bin`, `packaging/linux/build-deb.sh` writes
`/usr/bin`. Nothing connects the four.

The second is **where the window is**. `mix database open` starts a desktop client the daemon found
through `mixengine-platform`'s `DesktopApps::locate`, whose hint comes from a `desktop-app`
extension's manifest — `<old>.exe`, `io.github.haiquang9994.<old>`, `<old>.desktop`. Those three
name *standalone client*. The merged application is `mixlab`, `io.github.mixnz.mixlab`,
`mixlab.desktop`, it is installed by MixEngine's own installers, and no hint anywhere names it. On a
machine that installed MixEngine after T105, `mix database open mariadb` finds nothing at all unless
the person also installs the `<old>` extension and standalone client beside it — which is the two
downloads ADR 0027 exists to remove.

This task answers both from `mixengine-platform`, and holds the first answer to the packaging
scripts that write it.

## What is already true

Written down so nothing below is built twice:

- **The window is registered by every installer.** NSIS writes `Uninstall\MixEngine` with
  `InstallLocation`, the `.pkg` places `MixLab.app` in `/Applications`, and the `.deb` and `.rpm`
  place `/usr/share/applications/mixlab.desktop`. What is *not* registered anywhere is the window
  inside a portable archive or an AppImage, and that is the case the design below is shaped by.
- **The handoff already survives a running window.** `launch::forward` sends the `<old>://` URL
  *and the credential* over `instance`'s channel to the copy already running, which exits 0 — read
  by `desktop::launch` as `Started::HandedOn`. T83's "a tab in the running window, password in the
  environment and nowhere else" needs no new work; it needs the window to be *found*.
- **`<old>://` is the merged application's scheme.** NSIS registers `Software\Classes\<old>` at
  `mixlab.exe`, `packaging/linux/mixlab.desktop` declares `MimeType=x-scheme-handler/<old>`, and
  `modules/db/handoff.rs` refuses any other scheme.
- **`application_file_name` and `application_root` already exist** (T106), and are the two halves of
  "a windowed application on macOS is a directory". This task adds the third.

## D1 — One function for where a MixEngine program is

`mixengine_platform::install` gains two public items:

```rust
/// Every directory an installer of this operating system puts MixEngine's programs in, in the
/// order they are consulted.
pub fn program_dirs() -> Vec<PathBuf>;

/// Where `name` is on this machine: beside the running executable, then this operating system's
/// install location, then `PATH`. `None` when this machine has no such program.
pub fn program_path(name: &str) -> Option<PathBuf>;
```

`name` is a bare name out of `MIX_BINARIES` — `mixengined`, `mix`. The executable suffix is this
platform's and is appended here, so no caller spells `.exe`.

`program_dirs()`, per operating system:

| | Directories, in order |
| --- | --- |
| Windows | `%LOCALAPPDATA%\Programs\MixEngine` |
| macOS | `/usr/local/bin` |
| Linux | `/usr/bin`, `/usr/local/bin` |

**The order in `program_path` is the order of decreasing certainty**, and each step earns its place:

1. **The running executable's own directory.** This is the portable archive, the AppImage, and every
   `cargo run`, and it is the only step that cannot be wrong: a daemon started from a directory is
   the daemon of the install in that directory. `std::env::current_exe()` may fail on an exotic
   system, and its failure is a step skipped rather than an error.
2. **This operating system's install location.** The measured case the old comment argues: a per-user
   NSIS install edits the *user's* `PATH`, and a process — or an Explorer-started GUI — carries the
   `PATH` it inherited when it opened. Asking `PATH` alone answers "MixEngine is not installed" on a
   machine that has it.
3. **`PATH`**, for a distribution package, a Homebrew-style install, or anything a person arranged
   themselves.

**Empty `PATH` entries are dropped**, and that is a security fix rather than tidiness: an empty entry
means the current directory on both Windows and Unix, and the caller of this function *executes* what
it returns. `Command::new("mixengined")` — what `health.rs` does today — already has that exposure
on Windows, where the search includes the application directory and the current directory before
`PATH`. Resolving here, install location first, removes it.

## D2 — `health.rs` reads it, and stops asking a process

`well_known()` and `program()` go. Three call sites become one function each:

- `program()` → `install::program_path(DAEMON).map_or_else(|| DAEMON.into(), OsString::from)`. The
  bare name stays as the last resort: a `PATH` entry that appeared after this process started is
  still worth a spawn, and a spawn that fails says so.
- `installed()` → `install::program_path(DAEMON).is_some()`, and **it is no longer `async` and no
  longer spawns anything**. Today it runs `mixengined --version` and throws the output away; that is
  a process creation, a console window suppressed by hand, and up to a second of a tab's opening,
  to answer a question three `stat`s answer.
- The Windows-only test asserting the shape of `well_known()[0]` moves to `mixengine-platform`,
  where the function now lives, and is joined by the packaging pin in D6.

`Presence`'s four states, their `camelCase` wire spelling and `HEALTH_TIMEOUT` are untouched.

## D3 — The window this install has

The merged window is found by a lookup of its own, and **not** by adding a fourth OS table to
`DesktopApps::locate`. The trait gains one method:

```rust
/// The desktop application this MixEngine install has, if it has one — roadmap task T107.
fn locate_window(&self, executable: &str, bundle: &str) -> Result<Located>;
```

Two arguments and not zero, for `application_file_name`'s reason (T106): `cargo` names the binary
after `[package].name` and Tauri names the bundle after `productName`, `packaging/common.sh` declares
both, and the platform crate may not hold a name packaging owns. The caller holding them is
`mixengine-core`'s `window` module (D5).

The rule, in one sentence: **the window that belongs to the install the running program belongs to.**

1. Beside the running executable: `directory.join(application_file_name(executable, bundle))`. On
   Windows and Linux that is the file; on macOS it is `MixLab.app`, and the executable inside it is
   `Contents/MacOS/<executable>` — `application_executable`, the inverse of T106's
   `application_root`, added per OS beside it.
2. **macOS only**, and only when the running executable's directory is one of `program_dirs()`:
   `/Applications/<bundle>`. This is the one system where an installer splits the two — the `.pkg`
   puts the four binaries in `/usr/local/bin` and the bundle in `/Applications` — so it is the one
   system that needs a second step. Windows and Linux answer `None` here: their installers put the
   window beside the binaries, and step 1 has already looked.
3. Nothing. `Located::NotInstalled { searched }` naming the paths that were looked at, in this
   system's own currency, the way `locate`'s three implementations do.

**`PATH` is deliberately not consulted, and neither are the OS's registration tables.** A `mixlab`
first on `PATH`, or a `MixLab.app` Spotlight knows about, may belong to a different install of
MixEngine than the daemon doing the asking — and `mix database open` would then depend on which copy
a person's `PATH` happens to name. It would also make this lookup a property of the machine rather
than of the install, which is the thing that makes it untestable: `crates/mixengine-cli/tests/database.rs`
starts a real daemon out of `target/debug`, and on a developer's machine with MixEngine installed
(this is one) a `PATH` step would find the installed window and turn two green assertions red for a
reason that has nothing to do with the code. `target/debug` never holds `mixlab`: the desktop crate
is a workspace of its own and builds into `apps/desktop/src-tauri/target`.

The implementation is one shared function, `crate::desktop::locate_window`, called by all three
`Apps` — `launch`'s arrangement — with the per-OS half in `sys::install`.

The mock gains a window of its own, set by `MockHost::with_window`, separate from the application
`with_desktop_app` installs: a test has to be able to say *this machine has MixLab and not the standalone client*,
*the standalone client and not MixLab*, and *both*.

## D4 — The daemon prefers its own window, and says when it did not

`Databases::locate_client` today reads the first installed `desktop-app` extension, takes its per-OS
hint, and asks `locate`. It gains a step in front, and the two steps together answer one question:

```
the window answers when
    there is no `desktop-app` extension installed
  or the installed one's scheme is the window's own (`<old>`)
  and `locate_window` found it
otherwise the extension answers, exactly as it does today
```

**Why the scheme condition and not "the window always wins".** A `desktop-app` extension is a general
mechanism: a future entry could name some other client, for some other scheme, and a window asked
first unconditionally would shadow it — silently, since both arms of `DesktopClient` look the same to
a reader. The window is MixEngine's client *for `<old>://`*, and that is what the condition says. It
also leaves `crates/mixengine-cli/tests/database.rs`'s `nowhere` fixture — scheme `nowhere` —
answering exactly what it answers today.

**Why the window wins over the extension and not the other way round.** On a machine with both, MixLab
is the client MixEngine installed, updates and supports; standalone client is the one this phase
exists to stop requiring. `mixnz/<old>` is archived the day M12 ships.

Three consequences, each a sentence in the code:

- **`DesktopClient::Installed` gains an optional extension.** `extension: Option<ExtensionId>`, with
  `skip_serializing_if = "Option::is_none"` so the JSON an extension produces is byte-for-byte what
  it is today, and the window's simply has no such key. The alternative — a third arm — would
  touch every match on the enum in the CLI and the shell to say something neither of them renders:
  the CLI prints `name` and `program` from that arm and nothing else. `NotInstalled` keeps its
  required `extension`, because it can only ever come from one.
- **The scheme comes from the window when the window answered.** `Databases::scheme()` — a second
  read of the extension store, which could already return `Internal` for "the desktop client vanished
  between two reads" — goes. `locate_client` returns the scheme it resolved beside the state and the
  application, in one private struct, and that race goes with it.
- **`NoClient` is unchanged**, and so is its advice. A headless install has no window, no extension
  and nothing to open a database with, and `mix extension install <old>` is still what to do.

## D5 — The window's four names live in one module

`mixengine_core::updates::apply` holds `WINDOW` and `WINDOW_BUNDLE` today, pinned to
`packaging/common.sh` by `crates/mixengine-core/tests/packaging.rs`. This task needs two more — the
display name and the URL scheme — and neither is an updater's business.

A new `crates/mixengine-core/src/window.rs` holds all four:

```rust
pub const EXECUTABLE: &str = "mixlab";     // packaging/common.sh's MIX_WINDOW
pub const BUNDLE: &str = "MixLab.app";     // packaging/common.sh's MIX_WINDOW_APP
pub const NAME: &str = "MixLab";           // tauri.conf.json's productName
pub const SCHEME: &str = "<old>";          // what every installer registers
```

`updates::apply` re-exports the first two under the names it already publishes
(`pub use crate::window::{BUNDLE as WINDOW_BUNDLE, EXECUTABLE as WINDOW};`), so every existing path,
link and test keeps working and there is one definition.

**`SCHEME` stays `<old>` and is not renamed to `mixlab`.** Every the standalone client install on every machine has
registered `<old>://` with its operating system, links in the wild use it, and a scheme is a name
other software already holds — D10 says it stays, and this is the constant that says so.

## D6 — The install location is declared once, and packaging reads it

`packaging/common.sh` gains three assignments beside `MIX_WINDOW`:

```bash
MIX_INSTALL_WINDOWS='Programs\MixEngine'   # under %LOCALAPPDATA%, per user
MIX_INSTALL_MACOS=/usr/local/bin
MIX_INSTALL_LINUX=/usr/bin
```

Windows is the sub-path and not the whole one because the base is a variable both sides spell in
their own language — `$LOCALAPPDATA` to NSIS, `%LOCALAPPDATA%` to a person, `std::env::var_os` to
this crate — and only the part after it is a decision.

Read by:

- `packaging/windows/build.sh` → `-DINSTALL_SUBDIR`, and the `.nsi`'s
  `InstallDir "$LOCALAPPDATA\${INSTALL_SUBDIR}"`.
- `packaging/macos/build.sh`, where `$root/usr/local/bin` becomes `$root$MIX_INSTALL_MACOS`.
- `packaging/linux/build-deb.sh`, the same way, and `build-rpm.sh` through one more `sed`
  substitution (`@BINDIR@`) into `mixengine.spec.in`.

And pinned by a new test in `crates/mixengine-core/tests/packaging.rs`, which asserts *this*
operating system's first `program_dirs()` entry against the assignment for it — expanding
`%LOCALAPPDATA%` on Windows. `assigned()` learns to trim single quotes as well as double, which is
what the Windows value needs in order to keep its backslashes and its literal text through bash.

**What this stops.** An installer that moved and a lookup that did not is a window whose Start Menu
shortcut works and whose `mix database open` says *MixLab is not installed*, on the machine it is
installed on. It is exactly the failure `the_window_is_named_the_same_on_both_sides` was written for,
one directory up.

A second, smaller pin: `window::SCHEME` against `packaging/linux/mixlab.desktop`'s
`MimeType=x-scheme-handler/…`. The daemon writes that scheme into every handoff URL; the desktop
entry is what makes a `<old>://` link reach the window at all; a rename of one alone is a URL nobody
answers.

## Error handling

Nothing here returns a new error. `program_path` and `locate_window` answer `Option` and `Located`
respectively — *not installed* is an answer and never a fault, `locate`'s existing rule. The one
error either can produce is the `Error::Io` a `DesktopApps` implementation already produces, and
neither of these two touches the filesystem in a way that can raise one: `is_file()` is false for
every reason a `stat` can fail.

`Located::NotInstalled { searched }` from `locate_window` phrases its answer the way the three
existing lookups do, naming the paths rather than the mechanism, so `mix database client` prints
something a person can check.

## Testing

- **`mixengine-platform`, unit.** `program_path`'s search is split so the directory list is an
  argument: given tempdirs, the first that holds the file wins, an empty entry is not the current
  directory, and the executable suffix is appended. `program_dirs()` is non-empty and absolute on
  every system. `application_executable` round-trips with `application_root` on all three, which is
  how the macOS bundle rule is tested off macOS.
- **`mixengine-platform`, per OS.** The Windows list names `Programs\MixEngine`; the macOS list names
  `/usr/local/bin`; the Linux list names `/usr/bin` first. `locate_window` finds a file staged beside
  a fake "running executable" — the shared function takes the directory as an argument so the test
  does not have to be the program it is testing.
- **`mixengine-daemon`, on the mock.** The window answers and carries no extension; the window
  answers even with the `<old>` extension installed and the standalone client present; an extension for another
  scheme is *not* shadowed; no window and no extension is `NoClient`; the URL a window handoff
  produces carries the `<old>` scheme and the credential goes in the environment and not the
  arguments — the existing recorder assertion, against the new path.
- **`mixengine-core`, packaging.** The two pins in D6.
- **`apps/desktop`.** `health.rs`'s remaining test — there is always a program to try — and the four
  `Presence` spellings. The Windows path assertion moves to the platform crate.
- **`crates/mixengine-cli/tests/database.rs`** is expected to pass unchanged, and that is a result
  rather than an omission: a daemon out of `target/debug` has no window beside it, and the `nowhere`
  fixture's scheme is not the window's.

## What this task does not do

- **It does not change `<old>://`, `launch.rs` or `instance.rs`**, which the roadmap says stay. The
  forwarding, the credential over the channel and the single-instance judgement are T83's and T106's
  and are already what M12 needs.
- **It does not make `mix database open` install anything.** A machine with no window and no
  extension is told what to install, as today.
- **It does not teach `NotInstalled`'s `searched` to mention both lookups.** When the `<old>`
  extension is installed, the standalone client is missing *and* this install has no window, what is printed is where
  the standalone client was looked for. Naming both places is a better sentence and a bigger change to a proto field
  two clients render; it belongs with the profile work in T108 if anywhere.
- **It does not touch `mixengine_core::elevation::helper`'s "beside the program" fallback**, which
  answers a different question — which file to hand the elevation prompt — and is argued in T85's D1.
