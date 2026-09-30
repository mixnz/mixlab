# Phase 13 — Profiles

*Goal: a person who never wanted a database client never sees one; a person who does turns it on.*

Part of the [build plan](todo.md). Legend: `[ ]` todo · `[~]` in progress · `[x]` done · **(P)** =
has a platform-layer component and needs verification on Windows + macOS + Linux.

Design: [2026-09-08-the-desktop-client-in-this-repository-design.md](../specs/2026-09-08-the-desktop-client-in-this-repository-design.md),
on [ADR 0027](../decisions/0027-the-desktop-client-lives-in-this-repository.md).

---

**One build, one artifact, and what differs is a setting.** ADR 0027's rule 4. This phase adds
no screen to any module and changes no API; everything here lives in `src/shell/`, which is the
one place that is allowed to know the modules exist. Redesigning the window *for* the MixEngine
profile — a sidebar instead of a tab strip, say — is not this phase and has no spec yet.

- [x] **T108** A module visibility setting with three presets (D11). `enabledModules` in the shell
      settings store; `visibleModules()` is what the `[+]` menu, `Ctrl/Cmd+T`, `Ctrl/Cmd+1…N`, the
      Settings dialog's per-module panes and session restore all read. *MixEngine* is `mixengine`
      alone; *Everything* is all five with `mixengine` first; *Database tools* is the four toolbox
      modules. The Settings pane shows the presets and the five checkboxes underneath them. Turning
      a module off closes its tabs after a confirmation. A **first-run screen** — one question,
      three presets, one click — appears when there are no settings; T104's import skips it and
      picks *Everything*. A fresh machine that skips it gets *MixEngine*.
      Design: [2026-09-09-t108-a-module-visibility-setting-design.md](../specs/2026-09-09-t108-a-module-visibility-setting-design.md).
      **Two things this task settled.** The order is the registry's and never the stored set's —
      `MODULES` leads with `mixengine` and `visibleModules()` is a filter over it, which is what
      keeps a checkbox from moving `Ctrl/Cmd+1` and makes a hand-edited value able to be wrong about
      which modules but never about their order. And "no settings" turned out to be three questions
      rather than one: a MixLab install from before this task has a session and no `enabledModules`,
      and answering it *MixEngine* would have made its database tabs disappear on an upgrade — so
      any other shell key means *Everything*, and only a profile with nothing at all in it reaches
      the import marker and then the screen. The number chords are a function of the visible list
      now, but the loop that registers them is still over `MODULES` and passes `enabled` instead:
      hooks may not change in number between renders.

- [x] **T109** The default tab follows the profile. *MixEngine* and *Everything* open on the
      Dashboard, *Database tools* on a Database tab; `DEFAULT_MODULE_ID` becomes a function of the
      setting. Session restore still wins when there is a session, so the default only ever decides
      the very first tab and the tab after the last one is closed.
      Design: [2026-09-10-t109-the-default-tab-follows-the-profile-design.md](../specs/2026-09-10-t109-the-default-tab-follows-the-profile-design.md).
      **What this settled.** There is no `DEFAULT_MODULE_ID` any more, and no table of a preferred
      module per preset either: the default is the first module the profile shows, which the
      registry's order already decides, and all three rows of D11 fall out of that one line. The
      five places a tab opens without a module named — the first tab, the tab after the last one is
      closed, the tab after a module is turned off, `Ctrl/Cmd+T` and a one-module `[+]` — move
      together, because a window whose first tab is the Dashboard while `Ctrl/Cmd+T` opens a
      database client has two defaults. And the empty visible list stays unguarded on purpose: a
      default outside `enabled` would have `Workspace` open a tab its own effect drops and reopen
      it forever, so the guards stay at T108's three edges.

- [x] **T110** The bridge when a module is hidden (D11, last paragraph). The Services screen's
      *open* button, drawn from `database.client` as today, offers two things when `db` is off:
      enable the built-in client and open the tab, or hand off to an external client through
      `database.open` where one is installed. A `<old>://` handoff that arrives with `db` hidden
      enables it for that tab and says so in the tab. Nothing the daemon answers changes.
      Design: [2026-09-10-t110-the-bridge-when-a-module-is-hidden-design.md](../specs/2026-09-10-t110-the-bridge-when-a-module-is-hidden-design.md).
      **Two things this task settled.** The enabling belongs at the tab-request queue and not at
      either caller: the *open* button and a `<old>://` URL already push through the same queue, so
      one step in `Workspace`'s drain covers both, generically, for any module — and it fixes what
      T108 left, where a handoff to a hidden module opened a tab the visibility effect dropped on
      the same commit. And "enables it for that tab" is the setting and not a second, tab-scoped
      notion of visible: two notions would have the tab strip, the number chords and the Settings
      checkbox saying three different things about one question, so what belongs to the tab is the
      *reason* — one line saying the module was turned on, which earns its place because turning
      one on renumbers `Ctrl/Cmd+1 … N`.

**Milestone M13** — a fresh install shows the first-run screen; choosing *MixEngine* leaves a
window whose every tab, shortcut and Settings pane is about the daemon; turning *Database tools*
on in Settings brings the four modules back with everything a standalone-client user had saved; turning them
off again closes their tabs and hides them without deleting anything.
