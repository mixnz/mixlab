---
status: implemented
date: 2026-09-19
task: T168
---

# T168 — MixEngine in the tray

Roadmap task [T168](../roadmap/phase-22-mixengine-in-the-tray.md), phase 22. 2026-09-19. Decision:
[ADR 0042](../decisions/0042-mixlab-starts-at-login-when-a-person-asks-it-to.md).

**The case this comes from.** Every other local-server tool a person has used lives in the tray or
the menu bar: one click shows what is running, starts and stops it, and opens the main window. MixEngine
has no such place. To stop MariaDB a person opens MixLab, waits for the window, finds the Dashboard,
and closes the window again, which quits the app. The CLI can do all of it, but a person who reaches for the
tray is not going to open a terminal.

[`client-surface.md`](../features/client-surface.md) already says what such an item
needs: *"no more than the dashboard does: overall state, stop-all, and the site list."* The
[desktop roadmap](../roadmap/desktop/mixengine-module.md) left it open as a decision
about the shell rather than about the module. This design makes that decision.

## Principle

**The tray is one more screen of MixLab, not a second client.** It is drawn by the same React code, from the
same `bindings/` types, through the same `invoke` commands as the Dashboard. It reads everything
it shows from the daemon and infers nothing. The one thing it adds that has nothing to do with the daemon
is that MixLab can start at login with no window. That is a switch a person turns on, as ADR 0016
already requires for the daemon's own autostart.

## What is already true

- **The API is complete for this.** `daemon.status`, `service.list`, `service.start`, `service.stop`
  (no `service` means every declared service, see `service_action_call` in
  `src-tauri/src/modules/mixengine/commands.rs`), `site.list` and `daemon.shutdown` (which answers
  `DaemonShutdown` after everything has stopped). No new API method is needed, so *no client-only
  capability* holds trivially.
- **MixLab ships in every installer** (T105), and Tauri 2 has a tray behind the `tray-icon` feature.
  The Linux CI legs already install `libappindicator3-dev`.
- **A second copy of MixLab hands its start to the first.** `launch::forward` calls `bring_to_front`
  (`src-tauri/src/launch.rs`). A hidden window can therefore always be brought back by opening MixLab again, even
  where no tray icon is visible.
- **Starting the daemon from MixLab exists:** `mixengine_start`, preceded by the storage picker on a
  home that has never started (`mixengine_storage`).
- **Missing today:**
  - MixLab has no command for `daemon.shutdown`.
  - Closing the window quits the app.
  - Nothing in MixLab knows about a second window.
  - **The event stream has exactly one slot.** `MixEngineState` keeps one `CancellationToken`, and
    `mixengine_watch` says *"re-opening closes the one that is open"*. A tray panel that opened its own stream
    would close the main window's.

## D1. The panel is a webview on all three systems. Only how it opens differs

| | macOS, Windows | Linux |
|---|---|---|
| Clicking the icon | Opens the panel beside the icon | Opens a three-item native menu: **Open control panel**, **Open MixLab**, **Quit MixLab** |
| The panel | Undecorated, always on top, anchored to the icon, hides when it loses focus | A small ordinary window with decorations, placed by the window manager, hidden by its close button or Esc |
| Content | Identical everywhere (D3) | Identical everywhere (D3) |

**Why Linux cannot have the popover.**
- Tauri's Linux tray is AppIndicator / StatusNotifierItem through `libayatana-appindicator`. There,
  clicking the icon always opens a menu and the application is never told about the click, so no click can
  open a window directly.
- An application is not told where its icon is. On Wayland, the default on current Ubuntu and Fedora, an application
  cannot position its own window at all.

Anchoring a panel to the icon is therefore not something Linux allows. An ordinary window, reached through a
menu item, is the most that can be done. **Open MixLab** is in that menu so the most common action
is not two clicks deep.

**Why not a native menu everywhere.** A native menu cannot put Start and Stop on a service's row.
A menu built in Rust would also have to parse service payloads and translate its own strings,
both of which the webview already does. Using one panel means one set of components and one dictionary, and
`events.rs` stays a pipe that carries raw JSON.

## D2. One window, created at startup, hidden

- A second webview window, label `tray`, is **created hidden in `setup`** and never destroyed.
  Creating it on the first click costs a visible half-second on the first open. Creating it once is
  roughly one more renderer process, which WebView2 and WKWebView share with the main window's
  process group.
- **Size:** 540 × 600 logical pixels on macOS and Windows. On Linux, where it is a normal window,
  the minimum size is the same. (360 × 520 was the first measure; a service row with *Resting —
  starts on the next visit* in it pushed its own name out and its button off the card; 440 cut off
  the memory reading of the CPU and memory strips.)
- **macOS and Windows window attributes:** `decorations: false`, `transparent`, no system shadow,
  `always_on_top`, `skip_taskbar`, `resizable: false`, and on macOS `visible_on_all_workspaces`, so
  it opens over a full-screen app. **The window is transparent**, which on macOS takes
  `macos-private-api` and so rules out the Mac App Store, where MixLab is not. It is what lets the
  page draw a rounded card with its own shadow 10px inside the window, and **slide it in from the
  right** each time the window is shown (560 ms, eased out; a plain fade under reduced motion).
- **Showing and hiding:**
  - A click on the icon toggles the panel.
  - **The page hides the window, not Rust.** Every way the panel goes — a click elsewhere
    (`WindowEvent::Focused(false)`, macOS and Windows), a click on the icon, Open MixLab, Esc —
    becomes one `tray://dismiss` event: the page slides the card out (420 ms, eased in) and only
    then calls `tray_hide_panel`. A window hidden mid-show keeps its last frame, so hiding first
    made the next show flash the card in place before it slid. Rust hides the window itself after
    1.2 s if the page has not, unless it was shown again meanwhile.
  - Clicking the icon while the panel is open raises both events, and both mean "away": the panel
    is still visible while it slides out, so the click dismisses rather than re-opens.
- **Position: the corner, not the icon.** The window goes in the right-hand corner of the usable
  area on the bar's side — under the menu bar on macOS, against a bottom taskbar in the bottom
  corner, under a top one in the top corner — the way the system's own tray panels sit. Two pure
  functions, `bar_at_top` (from where the icon is, or where the work area starts when that makes
  no sense) and `panel_position`, carry it and are tested on their own.
  - **Which monitor** is found by asking every monitor whether it holds the click, as a physical
    point and as a logical one: the point a tray event carries is physical on Windows and logical on
    macOS, and `monitor_from_point` with the wrong one finds nothing — the first build opened the
    panel wherever the window manager put it. The primary monitor answers when none does.

## D3. What the panel shows

From top to bottom:

0. **Header: the application.** The MixLab mark — 38px, the height of the two lines beside it — the name
   **MixLab**, and under it, in the muted secondary style, the slogan **"For Developers. By
   Developers."**
   - The slogan is a brand line, not a sentence about state, so it never changes and nothing
     depends on it. It goes through `t("...")` like every other string; both dictionaries carry it
     in English, because a slogan is not translated.
   - It sits in the header rather than the footer, which holds the actions. It stays pinned while
     the lists below scroll.
1. **The engine, as a card of its own.** Running: the engine's mark (`EngineIcon`), **MixEngine**,
   a *Running* pill and *Services running: N/M*, and under them two strips of CPU and memory in the
   Dashboard's own drawing (`DaemonUsage`, shared): the daemon's, and every service's added up
   (`servicesTotal` — only what the frame measured; a service it did not measure is not a 0). That strip holds `/metrics` open — which is
   what makes the daemon sample every second — only while the panel is shown, focused and the
   daemon is up; `MetricsState` keeps one stream per window, like `MixEngineState` (D5), so the
   panel's never closes the Dashboard's. The header names the application and the card
   names what it drives, each with its own mark, so "MixLab managing MixEngine" is read off the
   layout rather than spelled out in a sentence.
   - Not running: MixLab's own gate, centred in the panel — the engine's mark, the gate's sentence
     (*MixEngine is installed but not running.*) and **Start MixEngine**, drawn as the tab draws
     them, without the four folders. On a home that has never started, with `mixengine_storage`
     saying the choice is still free, the button is **Set up in MixLab** instead: the storage choice
     belongs to MixLab's screen, not a panel. *Not installed* and *Not answering* use their gate
     sentences and offer **Open MixLab**.
2. **Services.** One row per service from `service.list`:
   - Name, version, and the state label and tone from `serviceStateLabel.ts`. The Dashboard's words,
     *Resting* included, are reused, not rewritten.
   - One button: **Start** when the service is stopped, **Stop** when it is running. While the call is
     in flight the row shows the pending state, as the Dashboard does (`pendingOps.ts`).
   - The list scrolls.
3. **Stop all.** It acts at once, as the Dashboard's does — the services start again with a click —
   and sends `service.stop` with no target —
   `mixengine_service_stop_all`, a command of its own for the reason `mixengine_service_start_project`
   is one: an empty id must never be the way to say "everything".
4. **Sites.** One row per site from `site.list`: the domain, which opens the site's URL in the default
   browser through `opener`. It has no controls. Sharing and anything else are MixLab's.
5. **Footer.**
   - **Open MixLab**: shows, unminimises and focuses the main window.
   - **Stop MixEngine**: asks for confirmation first (D4), then calls `daemon.shutdown`.
   - A small **Quit MixLab** icon button, which exits the app and leaves the daemon running. Its
     tooltip says the daemon keeps running only when there is one running.

The panel keeps no state of its own that the Dashboard does not also keep. It re-reads `daemon.status`,
`service.list` and `site.list` each time it is shown, and live updates come from D5.

## D4. Stop MixEngine is confirmed inside the panel

**Stop MixEngine** does not act on the first click; **Stop all** does, as it does on the Dashboard.
The footer turns into *"Stop MixEngine and every service it runs?"* with **Cancel** and **Stop**,
and goes back after 5 seconds or when the panel hides.

A native dialog would take focus from the popover and hide it in the middle of the question. It would also
look different on each system, so it is not used. The Linux panel is an ordinary window and could have
one, but the same inline row is used there too, so that the panel stays the same everywhere.

After `daemon.shutdown` answers, the panel shows the result — how many services stopped with it,
the failures that `DaemonShutdown.services` names, and `unordered` when it is set. It then
switches to the stopped state. It does not report the stop as done before the answer arrives.

**The two windows follow each other.** A daemon started from the panel is noticed by MixLab's
gate, which asks `/health` every 2 seconds while it is up; one stopped from the panel ends MixLab's
event stream, and the tab goes back to its gate. The panel does the same while it is open and the
daemon is down. Services need nothing extra: both windows apply the same events.

## D5. The event stream serves every window

`MixEngineState`'s single slot becomes **one stream per webview label**:
- `mixengine_watch` takes the calling `WebviewWindow`, and cancels and replaces only that window's token.
- `mixengine_unwatch` does the same.

Each window's `daemonWatch.ts` works as it does today: opened once for the life of its JavaScript
context. Two streams against the daemon cost one extra idle connection. The daemon already supports
several `/events` subscribers, since `mix` and MixLab watch at the same time.

**A stream that ended is reopened.** The panel outlives many daemons — it is never destroyed, and
*Stop MixEngine* ends the very stream it is listening on. So `daemonWatch.ts` forgets a channel that
reported `<old>_disconnected`, and the panel calls `ensureDaemonWatch()` whenever it finds the daemon
running again.

The alternative was one stream in Rust fanned out to every window. It was rejected because it moves
the "open once for the life of the app" logic from `daemonWatch.ts` into Rust for no gain.

## D6. Closing the main window hides it, when there is a tray to come back through

- **While the tray exists**, `WindowEvent::CloseRequested` on `main` is prevented and the window is hidden.
- **On macOS**, hiding the main window also switches `ActivationPolicy` to `Accessory` (no Dock icon),
  and showing it switches back to `Regular`. `⌘Q` still quits the whole app. That is the platform's
  meaning of quit, and the panel's **Quit MixLab** does the same thing.
- **When there is no tray**, closing quits exactly as today. There is no tray when:
  - the `mixengine` module is not visible (a MixLab used only as a database client, T108), or
  - on Linux, no StatusNotifierItem host is on the session bus (D8).
- `tauri-plugin-window-state` keeps working. Hiding is not closing, so the maximised flag is
  saved at exit.

## D7. MixLab at login, a switch in Settings

- **Setting:** *Open MixLab in the tray at login*, in the mixengine module's Settings screen next to
  `AutostartSection` (the daemon's switch). **Off by default.** No installer turns it on
  ([ADR 0042](../decisions/0042-mixlab-starts-at-login-when-a-person-asks-it-to.md)).
- **Two switches, not one:**
  - *Start MixEngine at login* (`autostart.enable`, the daemon's, unchanged).
  - *Open MixLab in the tray at login* (this one, the window's).

  MixLab does **not** start the daemon when it starts at login. If the daemon is not running, the
  panel says so and offers **Start MixEngine**. A person may want the daemon without the window, and
  the other way round.
- **Mechanism:** `tauri-plugin-autostart`, with the argument `--hidden`:

  | OS | Entry the plugin writes |
  |---|---|
  | macOS | A LaunchAgent in `~/Library/LaunchAgents` |
  | Windows | `HKCU\…\Run` |
  | Linux | `~/.config/autostart/<name>.desktop` |

- **The switch reads what is on disk, not a remembered value.** Settings shows `isEnabled()` every time
  it opens, because a person can remove the entry from System Settings, Task Manager or their desktop
  environment without MixLab knowing.
- **`--hidden`** is read in `launch::Opening::from_process` next to the URL:
  - `main` is declared `visible: false` in `tauri.conf.json`, and `setup` shows it unless the start was
    hidden. The window then never flashes at login.
  - A `--hidden` start that is forwarded to a running copy is a no-op; it does not bring the window up.
  - A relaunch after an update (T106) shows the window as it does today, even if the window was hidden.
- **Things that are easy to get wrong:**
  - **AppImage:** the entry has to name `$APPIMAGE`, the file the person keeps, not the path it was extracted
    to under `~/.cache`. That path changes with every version.
  - **Development builds** must not overwrite the release entry. The entry's name carries the
    same suffix a development build's home does (ADR 0024, ADR 0040), and `npm run dev:app` never
    registers one.

## D8. Linux: the icon may have nowhere to appear

GNOME without the AppIndicator extension has no StatusNotifierItem host, and an icon there is not drawn.
Hiding the main window on close would then leave a running app with no visible way back other than
opening MixLab again.

- **Before creating the tray, MixLab asks the session bus whether `org.kde.StatusNotifierWatcher` has an
  owner.** If it has none, there is no tray and no close-to-tray (D6). The login switch (D7) is still
  offered but says *"Your desktop shows no tray icons. MixLab will open its window at login instead."*,
  and a `--hidden` start then shows the window.
- **The runtime library** is a **weak** dependency: `Recommends:` in the `.deb` and the `.rpm`,
  which apt, dnf and zypper install by default. It is not `Depends:`, because MixLab runs without it
  and only the icon is lost; a hard dependency would refuse the whole window on a distribution that
  does not package it. The AppImage uses the system's copy, as ADR 0028 decided for WebKitGTK, and
  `AppRun` prints one line naming the package when `ldconfig` cannot find it, then starts the window
  anyway.
- **Measured (from the source, 2026-09-19): a missing library is a panic, not an error.**
  - `libappindicator-sys` 0.9.0, the version `tray-icon` 0.24.2 pulls in, loads the library in a
    `Lazy` static. It tries `libayatana-appindicator3.so.1`, then `libappindicator3.so.1` (and, behind
    its `backcompat` feature, the two unversioned names), and calls `panic!` when none loads.
  - That panic happens on the first call into the library, inside `TrayIconBuilder::build`, on the
    main thread. It takes the application down.
  - So `tray.rs` loads the same names itself with `dlopen` before building a tray. When none loads, it
    treats the session as having no host.
  - With the library present and no StatusNotifierItem host (GNOME without the extension),
    `libayatana-appindicator` falls back to a GtkStatusIcon that GNOME Shell does not draw, and `build`
    returns `Ok`. That is why the D-Bus check is needed as well as the `dlopen` one.

## D9. Where the code goes

**Frontend:**
- `ModuleDefinition` (`src/shell/module.ts`) gains `TrayPanel?: ComponentType`. The shell never names
  the module that supplies it.
- A second Vite entry, `tray.html` → `src/tray.tsx`, renders the `TrayPanel` of the first visible
  module that has one. It goes through `shell/registry.ts`, the one import from `modules/` the lint rule allows.
- The panel lives in `src/modules/mixengine/tray/`. It uses `components/` primitives (the
  `using-shared-components` skill applies) and the module's existing `api.ts`, `serviceStateLabel.ts`,
  `pendingOps.ts` and `daemonWatch.ts`.
- Strings go in the module's `i18n/`, in both `en.ts` and `vi.ts`.

**Rust:**
- `src-tauri/src/tray.rs`, a sibling of `launch.rs`: the icon, the Linux menu, the panel window, its
  show, hide and position logic, close-to-tray and the activation policy.
- The labels of the three Linux menu items come from the frontend through `tray_configure { enabled,
  labels }`, which the main window calls on start and whenever the language or the visible modules
  change. Rust holds no dictionary. The same command is what turns the tray off when the `mixengine`
  module is hidden.
- New commands, following the five-place convention in `adding-a-command.md`:
  - `tray_configure`
  - `tray_open_main`
  - `tray_hide_panel`
  - `app_quit`
  - `mixengine_shutdown` (`daemon.shutdown`, in `modules/mixengine/commands.rs`)
  - `login_item_status` / `login_item_set`
- **Capabilities:** a new `capabilities/tray.json` for the `tray` window, with `core:default`,
  `opener:allow-open-url` (sites) and `log:default`. It has no `dialog`, `store` or `clipboard` permissions.
- **Dependencies:**
  - the `tray-icon` feature on `tauri`
  - `tauri-plugin-autostart`
  - on Linux only, a D-Bus call for D8, using `zbus`. `Cargo.lock` already carries it, at 4.4 and 5.18,
    through `keyring`'s `sync-secret-service` and the WebKitGTK stack. Naming whichever version is already
    in the tree adds nothing new to build.
- `tests/layering.rs` stays as it is: nothing here reaches past `mixengine-proto` and
  `mixengine-platform`.

**Icons:**
- A monochrome template image for the macOS menu bar (`icon_as_template(true)`, so it follows light and
  dark mode).
- The app icon for Windows and Linux.
- Both come from `npm run icons`, next to the ones it already builds ([icons.md](../standards/desktop/app-icon.md)).

## Not in this design

- **An icon that changes with state** (daemon down, a site shared on the LAN, as `lan-sharing.md`
  suggests). Rust would need to read events to do it, which D1 avoids. A later task can have the
  panel's window, or the main window, send the state to `tray.rs`.
- **Project-level start and stop, logs, restart.** These are MixLab's. The panel is the dashboard's summary, not the
  dashboard.
- **A keyboard shortcut that opens the panel globally.**

## Tasks

- **T168a** The event stream per window (D5), with its tests in `state.rs`.
- **T168b** `mixengine_shutdown`, `app_quit`, `tray_open_main`, `tray_hide_panel`.
- **T168c** `tray.rs`: icon, panel window, `panel_position` with unit tests, blur-hide with its debounce,
  close-to-tray and the macOS activation policy. **(P)**
- **T168d** Linux: the three-item menu, the StatusNotifierWatcher check, the packaging dependency and the
  `AppRun` message. **(P)**
- **T168e** The frontend: `TrayPanel` in `ModuleDefinition`, the `tray.html` entry, the panel
  (D3), inline confirmation (D4), strings in both languages.
- **T168f** Login item: `tauri-plugin-autostart`, `--hidden`, `main` hidden until shown, the Settings
  switch, AppImage and development-build names (D7). **(P)**
- **T168g** The tray and login icons from `npm run icons`, `client-surface.md` and the desktop roadmap's
  open question updated to point here, and a handbook page covering GNOME.

**Milestone M22:**
- On macOS and Windows, a person who ticked the login switch sees the icon after signing in with no
  window. From the panel they can stop MariaDB, stop everything, and shut MixEngine down, confirming the
  last two, without opening MixLab.
- On Ubuntu the same works through **Open control panel**.
