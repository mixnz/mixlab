---
status: approved
date: 2026-09-29
task: T192
---

# T192 — The tray is MixLab's

Roadmap task [T192](../roadmap/phase-32-the-tray-is-mixlabs.md), phase 32. 2026-09-29. Decision:
[ADR 0058](../decisions/0058-the-tray-is-mixlabs-and-a-module-lends-it-a-section.md). It builds on
[T168](2026-09-19-t168-mixengine-in-the-tray-design.md) and changes that design's D1, D3, D6, D7 and
D9. The rest of it (the panel window, its position, D5's stream per window, D8's Linux checks) stands
as built.

**The case this comes from.** On the *Database tools* preset, a person with three terminal sessions
open closes the window, and the sessions die. Nothing in the terminal module decides that: the tray
icon exists only while the `mixengine` module is visible (`hasTrayPanel` in
`src/shell/Workspace.tsx`), close-to-tray follows the icon (`on_window_event` in
`src-tauri/src/tray.rs`), and without it `WindowEvent::Destroyed` on `main` calls `app.exit(0)`. The
same person with MixEngine switched on closes the same window and keeps the sessions.

## Principle

**Running in the background is MixLab's; a module only adds to what the tray shows.** The shell owns
the icon, the close button's meaning, the login switch and the panel's frame. A module owns a section
of the panel and nothing around it.

## D1. The icon and close-to-tray follow the shell

- The main window calls `tray_configure` on start, whatever modules are visible. Rust creates the
  icon whenever `has_host()` says the session can show one. This is unchanged from T168's D8, so a
  Linux session with no StatusNotifierItem host still gets no icon.
- `TrayState` holds two flags where it held one:

  | Flag | True when | Governs |
  |---|---|---|
  | `icon` | the icon is up | close-to-tray, `hidden_start` |
  | `panel` | a visible module lends a section (D3) | what a click opens, the Linux menu |

- **The close button hides `main` whenever `icon` is true.** It is not a setting (ADR 0058, point 2).
  On macOS hiding still switches the activation policy to `Accessory`, and `⌘Q` still quits.
- `tray_configure { panel, labels }` replaces `tray_configure { enabled, labels }`. When `panel`
  turns false while the panel is open, the panel is hidden. The icon stays.
- **A login start now works on every preset.** `hidden_start` waits for the icon as before; the icon
  no longer waits for a module. Where there is no host, `tray_configure` still brings the window up
  at once (the `was || waiting` branch), and the eight-second grace still covers a page that never
  calls it.

## D2. What a click on the icon does

| | `panel` true | `panel` false |
|---|---|---|
| **macOS, Windows**: primary click | Toggles the panel, as in T168 | `bring_to_front` |
| **macOS, Windows**: secondary click | Native menu: **Open MixLab**, **Quit MixLab** | The same menu |
| **Linux**, any click | Menu: **Open control panel**, **Open MixLab**, separator, **Quit MixLab** | Menu: **Open MixLab**, separator, **Quit MixLab** |

- The secondary-click menu is what makes quitting reachable when there is no panel. On Windows the
  main window has no menu bar, so without it the only way out would be Task Manager.
- T168 opened the panel on either button, on the grounds that a right click on a Windows tray icon is
  expected to do something. It now does something else: it opens the menu. `on_icon_event` ignores
  `MouseButton::Right`, and the menu is attached with `show_menu_on_left_click(false)`, which is
  Tauri's way of saying "the secondary click".
- **Which action a click means is a pure function**, `click_action(panel, button) -> ClickAction`
  (`TogglePanel`, `OpenMain`, `Ignore`), unit-tested in `tray.rs` beside `panel_position`. The
  secondary click is `Ignore`: the operating system opens the menu itself, and anything done here
  as well would open the panel behind it.
- The Linux menu is rebuilt by `refresh_menu` whenever `panel` or the language changes.
- The labels travel from the frontend as today. `openPanel`, `openMain` and `quit` are already in the
  shell's dictionary (`tray.*`), so Rust still holds no words.

## D3. The panel is a frame the shell draws, with sections modules lend

**`ModuleDefinition`** (`src/shell/module.ts`) replaces `TrayPanel?: ComponentType` with:

```ts
/** What the tray's frame hands every section it draws. */
export interface TraySectionProps {
  /** The card is on screen. Always true on Linux, where the panel is a window and never slides. */
  shown: boolean;
  /** The panel's window has focus. A section reads its state again when this turns true. */
  focused: boolean;
  /** Slides the panel out and hides it, as Esc and a click elsewhere do. */
  dismiss: () => void;
}

TraySection?: ComponentType<TraySectionProps>;
```

**The frame**, `src/shell/tray/TrayFrame.tsx`, drawn by `src/tray.tsx`, takes over everything in
T168's `TrayPanel` that is about the window rather than the engine:
- the stage and the card, the slide in and out (`SLIDES`, `SLIDE_OUT_MS`), `dismiss`, the
  `tray://dismiss` listener, Esc and the focus listener;
- **the header**: on the left, the MixLab mark, **MixLab** and the slogan; on the right, **Open MixLab**
  (a small button with its label, being the action people reach for most) and **Quit MixLab** (an
  icon button with `PowerIcon`, its tooltip **Quit MixLab**). The slogan ellipsizes before either
  button is squeezed;
- below the header, every visible module's `TraySection`, in `MODULES` order, each inside its own
  `ErrorBoundary`, so that a section that throws leaves the header and its two ways out working.

**There is no footer.** The header holds the shell's two actions, and what the footer held for the
engine moves into the engine's section (D4).

**The panel exists only with a section.** `Workspace.tsx` computes `hasTraySection` from the visible
modules and sends it as `panel` (D1). `tray.tsx` draws nothing when there is none, as it does today,
and the backend never shows the window then.

**Strings.** `mixengine.tray.slogan` moves to the shell as `tray.slogan`. The frame uses the shell's
existing `tray.openMain` and `tray.quit`. `mixengine.tray.openMain` stays, because the gate's *Set up
in MixLab* and *Open MixLab* buttons are the section's. `mixengine.tray.quit` and
`mixengine.tray.quitAlone` go. Both languages change together.

**Styles.** `TrayPanel.module.css` splits in two: the stage, card, slide and header go to
`TrayFrame.module.css`; the engine card, gate, lists and rows stay with the section. The frame's
header is the part the css-on-whole-pixels skill applies to, at 125% and 150%.

## D4. The engine's section

`src/modules/mixengine/tray/TrayPanel.tsx` becomes `TraySection.tsx`. It takes `shown`, `focused` and
`dismiss` from the frame instead of owning them, and keeps everything else as built: the engine card,
the gate, services with *Stop all*, sites, the event stream, metrics only while `shown && focused`,
the poll while the daemon is down, and its own `ErrorBanner`.

- **Its refresh on show** moves from its own focus listener to an effect on `focused` turning true,
  together with clearing the shutdown report and putting away an armed question when it turns false.
- **Stop MixEngine moves into the engine card**, as its last row, under the CPU and memory strips.
  Armed, the row becomes the question *Stop MixEngine and every service it runs?* with **Cancel** and
  **Stop**, and goes back after `CONFIRM_TIMEOUT_MS` or when the panel loses focus. That is T168's D4
  with a different place; `trayModel.ts` is unchanged. The card only exists while the daemon runs,
  and so does the button, as before.
- The gate keeps its buttons (**Start MixEngine**, **Set up in MixLab**, **Open MixLab**), since they
  answer the gate's sentence.

## D5. The login switch is in the shell's Settings

- `LoginItemSection` moves from `src/modules/mixengine/screens/Settings/` to
  `src/shell/components/SettingsModal/`, in a new pane, **General**, listed directly under
  **Appearance**. It is the pane for MixLab-wide behaviour that is not how MixLab looks.
- The switch's label, *Open MixLab in the tray at login*, and the note under it where
  `has_tray_host` is false (*Your desktop shows no tray icons. MixLab will open its window at login
  instead.*) are unchanged.
- Its strings move from `mixengine.settings.loginItem.*` to the shell's dictionary. The daemon's own
  switch (`AutostartSection`) stays in the module, where ADR 0042 put it: the two switches are no
  longer side by side, and the General pane's switch says in one line that MixEngine has its own, in
  the MixEngine pane.

## D6. What does not change

- `create_panel` still creates the `tray` window hidden in `setup`, whether or not a section will
  ever use it. It costs a renderer process that is idle while hidden, and building it only when a
  section appears would bring back the half-second first open that T168's D2 removed.
- `capabilities/tray.json`, `panel_position`, `monitor_under`, D5's stream per window and D8's
  `dlopen` and D-Bus checks.
- `app_quit` leaves a running daemon running.

## MixLab

This is MixLab's alone and reaches no daemon method: the tray's frame, the click routing and the
General pane are the shell's. The one screen of a module that changes is the `mixengine` module's
tray section (D4), which calls the same methods as before (`daemon.status`, `service.list`,
`service.start`, `service.stop`, `site.list`, `daemon.shutdown`). `client-surface.md` needs no new
row. Its note on the tray item points at this design as well as T168's.

## Testing

- **Vitest.** Which sections the frame draws for a module set (`traySections`): with `mixengine`
  visible, and with the *Database tools* preset, where there are none and `hasTraySection` is false.
  `trayModel.test.ts` is unchanged. The suite has no DOM renderer, so that a section which throws
  leaves the header working is checked by hand below.
- **Rust.** `click_action` over both values of `panel` and every button, on the two systems it applies
  to. The existing `panel_position` tests stay.
- **By hand on Windows, then macOS; Linux through the menu.**
  - *Database tools*: the icon appears; the close button hides the window and a running terminal
    session is alive when the window comes back; a primary click brings the window back; a
    secondary click offers **Quit MixLab**, which quits.
  - *MixEngine*: the panel opens as before, with **Open MixLab** and **Quit MixLab** in its header and
    **Stop MixEngine** in the engine card; confirming stops the daemon and the card turns into the gate.
  - Switching MixEngine off in Settings while the panel is open hides the panel and keeps the icon.
  - A section made to throw leaves the header's **Open MixLab** and **Quit MixLab** working.
  - The login switch in General, with MixEngine hidden: after signing in, the icon and no window.

## Not in this design

- **A section for the terminal or any other toolbox module**, such as the open sessions listed in the
  panel. The frame makes it one component; which module wants one is a later question.
- **A setting for what the close button does** (ADR 0058, alternatives).
- **An icon that changes with state**, still out for T168's reason.

## Tasks

- **T192a** `tray.rs`: `icon` and `panel` in `TrayState`, `tray_configure { panel, labels }`,
  `click_action` with its tests, the secondary-click menu on macOS and Windows, and the Linux menu
  without **Open control panel** when there is no panel. **(P)**
- **T192b** The frame: `TraySectionProps` and `TraySection` on `ModuleDefinition`, `TrayFrame` with its
  header and per-section error boundaries, `tray.tsx` drawing it, `Workspace.tsx` sending `panel`, and
  the strings and styles moved (D3).
- **T192c** The engine's section: `TrayPanel` becomes `TraySection`, and **Stop MixEngine** with its
  confirmation moves into the engine card (D4).
- **T192d** The General pane and the login switch moved into it (D5).
- **T192e** `docs/architecture/desktop/frontend.md`, `client-surface.md`'s tray note, the handbook's
  tray page, and a `### Changed` line in `CHANGELOG.md` saying that closing the window keeps MixLab
  running in the tray on every preset.

**Milestone M32**: on Windows with the *Database tools* preset, a terminal session survives the close
button and the icon brings it back; with MixEngine visible the panel works as M22 describes, from
its new header and card.
