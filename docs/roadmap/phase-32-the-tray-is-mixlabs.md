# Phase 32 — The tray is MixLab's

*Goal: MixLab runs in the background whatever modules a person uses. The close button hides the
window on every preset, the icon brings it back, MixLab starts at login from the shell's Settings,
and a module adds a section to the panel instead of owning it.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Design: [2026-09-29-t192-the-tray-is-mixlabs-design.md](../specs/2026-09-29-t192-the-tray-is-mixlabs-design.md).
Decision: [ADR 0058](../decisions/0058-the-tray-is-mixlabs-and-a-module-lends-it-a-section.md).

---

- [ ] **T192a** `tray.rs`: `icon` and `panel` in `TrayState`, `tray_configure { panel, labels }`,
      `click_action` with its tests, the secondary-click menu on macOS and Windows, and the Linux
      menu without **Open control panel** when there is no panel. **(P)**
- [ ] **T192b** The frame: `TraySection` on `ModuleDefinition`, `TrayFrame` with its header and
      per-section error boundaries, `tray.tsx` drawing it, `Workspace.tsx` sending `panel`, and the
      strings and styles moved.
- [ ] **T192c** The engine's section: `TrayPanel` becomes `TraySection`, and **Stop MixEngine** with
      its confirmation moves into the engine card.
- [ ] **T192d** The shell's General pane, and the login switch moved into it.
- [ ] **T192e** The architecture notes, `client-surface.md`, the handbook's tray page, and the
      changelog line.

**Milestone M32**: on Windows with the *Database tools* preset, a terminal session survives the close
button and the icon brings it back; with MixEngine visible the panel works as M22 describes, from
its new header and card.
