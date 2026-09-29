# 0058. The tray is MixLab's, and a module lends it a section

**Status**: Accepted
**Date**: 2026-09-29

## Context

T168 gave MixLab a tray icon
([the design](../specs/2026-09-19-t168-mixengine-in-the-tray-design.md)), and with it three things
a desktop application does in the background: closing the main window hides it instead of quitting,
MixLab can start at login with no window
([ADR 0042](0042-mixlab-starts-at-login-when-a-person-asks-it-to.md)), and a panel opens from the
icon. All three were built as MixEngine's:

- **The icon exists only while a visible module has a tray panel**, and only `mixengine` has one.
  With the module hidden there is no icon.
- **Close-to-tray follows the icon** (T168's D6). Without it, closing the main window quits the app.
- **The login switch sits in the `mixengine` module's Settings screen** (T168's D7), next to the
  daemon's own switch.
- **The whole panel is the module's component**, including the parts that are about MixLab rather
  than about the engine: the header with the MixLab mark and slogan, **Open MixLab** and **Quit
  MixLab**.

[ADR 0056](0056-mixlab-stands-without-mixengine.md) came a week later and says that nothing a MixLab
user needs from MixLab itself may depend on MixEngine. Measured against it, the tray fails. A
person on the *Database tools* preset has no icon, cannot start MixLab at login, and loses every
open terminal session, database connection and REST response when they close the window, because
closing is quitting. A person with MixEngine visible closes the same window and keeps all of them.
Whether a terminal survives the close button depends on whether a module the person does not use
is switched on.

## Decision

**Running in the background is MixLab's. The tray's icon, its frame and the login switch belong to
the shell; a module may lend the panel a section, and never owns the panel.**

1. **The icon is the shell's.** The main window turns it on whatever modules are visible. Whether a
   session can show one at all is still decided in `tray.rs` (T168's D8), and where it cannot,
   nothing here changes: no icon, and closing quits.
2. **The close button always hides the main window while there is an icon.** It is not a setting.
   Quitting is **Quit MixLab** in the icon's menu or the panel, or `⌘Q` on macOS.
3. **The panel exists only when a visible module lends it a section.** `ModuleDefinition` carries
   `TraySection`, not `TrayPanel`. The shell draws the frame (the window, its slide, its dismissal and a
   header holding the MixLab mark, **Open MixLab** and **Quit MixLab**) and every visible module's
   section inside it, in the registry's order. With no section, clicking the icon brings the main
   window back instead of opening an empty panel.
4. **Quitting is always one click from the icon.** On macOS and Windows a secondary click opens a
   native menu with **Open MixLab** and **Quit MixLab**, with or without a panel. The Linux menu
   keeps its three items, and drops **Open control panel** when there is no panel.
5. **The login switch is the shell's**, in a **General** pane of the shell's Settings. ADR 0042's
   decision is otherwise unchanged: off by default, never turned on by an installer, separate from
   the daemon's switch, `--hidden`, and it reads what the operating system holds.

## Consequences

**Easy.** Every MixLab user gets the same background behaviour whatever modules they use, and the
question ADR 0056 asks of every feature has the answer it should. A second module that wants to be
in the panel, for example the terminal listing its open sessions, adds one component and never
touches the frame. The shell no longer loads a module's code to draw its own header.

**Hard, and accepted.**

- **A person on the *Database tools* preset who used to quit with the close button now hides the
  window.** It is a change in behaviour for them. It is also what the MixEngine preset already did,
  and what every tray application does. The changelog says so.
- **The panel's footer goes.** With the header holding the shell's two actions, **Stop MixEngine**
  moves into the engine's card, and its confirmation with it. **Quit MixLab** belongs to the shell,
  which knows nothing about the daemon, so its tooltip no longer says that MixEngine keeps running.
- **T168's D6 and D7, and ADR 0042's placement of the switch, no longer describe the code.** They
  are not edited. This record and its design are where the current behaviour is written down.

## Alternatives considered

**Keep the panel whole in the module and only decouple the icon.** It is the smallest change: the
icon and close-to-tray follow the shell, and the panel stays `mixengine`'s. It was rejected because
the header and the ways out of the panel are MixLab's, and would stay drawn by a module that ADR 0056
calls optional.

**A setting for the close button, hide or quit.** It would have kept the old behaviour for those who
had it. It was rejected because a background application whose close button sometimes quits is
harder to predict than one whose close button never does, and quitting is one click from the icon.

**A footer slot that a section borrows.** It would have kept the panel's layout as it was. It was
rejected because the frame would have to decide which of several sections gets to hold the footer,
for the sake of one button that fits in the engine's card.

**Ask before quitting when terminal sessions are open, and change nothing else.** It protects the
sessions from a mistaken click, but it does not keep them. It does not run MixLab in the background
either.
