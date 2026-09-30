---
status: implemented
date: 2026-09-10
task: T109
---

# T109 — The default tab follows the profile

Roadmap task [T109](../roadmap/phase-13-profiles.md), on
[the desktop client design](2026-09-08-the-desktop-client-in-this-repository-design.md)'s D11 and on
[T108](2026-09-09-t108-a-module-visibility-setting-design.md). 2026-09-10.

T108 gave the window a profile and left one function a task short of it. `defaultModuleId` clamps
the registry's default to something visible — enough that hiding the database module cannot open a
tab of it — but the constant behind the clamp is still `"db"`, so a person who picked *MixEngine*
and a person who picked *Everything* both get a window whose new tabs are the database client's.

This task is that one function. **No screen is added, no API method is touched, no stored value
changes shape**, and `MODULES` still lists all five in every build.

## What is already true

Written down so nothing below is built twice:

- **The order is the registry's, always.** T108's D1: `visibleModules(enabled)` is a filter over
  `MODULES`, and `MODULES` leads with `mixengine`. Nothing reads an order out of the stored set.
- **One function decides what a new tab is.** `Workspace.newTab()` defaults its `moduleId`
  parameter to `defaultModuleId(visible)`, and every tab that nobody named a module for comes
  through it.
- **Session restore already wins.** `readSession(visibleIds)` runs in a `useState` initializer, and
  `newTab()` is reached only when it answers `null` — an unreadable session, a session whose every
  tab belonged to a module this profile hides, or no session at all.
- **`DEFAULT_MODULE_ID` has exactly one reader**, `defaultModuleId` in `shell/profiles.ts`, and one
  assertion about it in `shell/registry.test.ts`.

## D1 — The default is the first module the profile shows

```ts
export function defaultModuleId(visible: ModuleDefinition[]): string {
  return visible[0].id;
}
```

D11's three rows fall out of that one rule, because the registry's order is already the app's
order:

| Preset | Visible, in registry order | The default tab |
| --- | --- | --- |
| **MixEngine** | MixEngine | MixEngine — the Dashboard |
| **Everything** | MixEngine, Database, REST, Terminal, Tools | MixEngine — the Dashboard |
| **Database tools** | Database, REST, Terminal, Tools | Database |

A MixEngine tab opens on its Dashboard screen on its own — `MixEngineTab` reads
`parseMixEngineTabState(restored)?.screen ?? "dashboard"` — so "opens on the Dashboard" needs
nothing here beyond opening a tab of that module.

**The rejected alternative is a table.** `PRESET_DEFAULTS: Record<PresetId, string>`, looked up
through `presetOf(enabled)`, gives the *same three answers* — and adds a second hand-written list of
module ids that has to agree with the registry's order forever, plus a branch for the set of
someone's own, which `presetOf` answers `null` for. A table that can only ever restate a rule the
order already carries is a table that will one day contradict it.

**What a set of someone's own gets** is the same rule and no special case: five checkboxes with
`db` and `terminal` ticked open a Database tab, because that is the first module such a window
draws. Nothing has to decide anything.

## D2 — What moves with it, and why that is right

`defaultModuleId` has five call sites, all of them through `newTab()`:

1. the very first tab, when there is no session to restore;
2. the tab that replaces the last one closed;
3. the tab that replaces the last one when a module is turned off and takes its tabs with it —
   T108's visibility effect, which is `closeTab`'s rule applied to a whole module;
4. `Ctrl/Cmd+T`;
5. the `[+]` button while the profile shows fewer than two modules — it opens a tab outright
   instead of a menu of one.

The roadmap names the first two. The rest move as well, because they are the same function,
and that is the intended reading rather than a side effect: T108's own comment on this function
says it is "what `Ctrl/Cmd+T` and a plain `[+]` open", and *the tab this window opens by default*
being one module while *the tab `Ctrl+T` opens* is another would be two defaults where the product
has one.

**This is visible to an existing *Everything* user**: `Ctrl/Cmd+T` opened a Database tab and now
opens MixEngine. It is the same deliberate change as T108's remap of `Ctrl/Cmd+1`, it lands in the
same release, and it goes in the changelog beside it. Their window itself does not move — a session
is restored before any default is consulted (D3).

## D3 — What does not move: session restore

Unchanged, and stated here so it is not "tidied up" later. `Workspace` decides its initial tabs in
a `useState` initializer:

```ts
const [restored] = useState(() => readSession(visibleIds));
const [tabs, setTabs] = useState<TabInfo[]>(() =>
  restored ? restored.tabs.map((tab) => ({ ...tab, badges: [] })) : [newTab()],
);
```

So the default only ever decides **the very first tab of a profile that has no session**, and **the
tab after the last one is closed**. An imported the standalone client user — T104's marker, T108's *Everything* —
has a session, and sees the tabs they left open, in the order they left them, exactly as before.

## D4 — `DEFAULT_MODULE_ID` goes

The constant is deleted from `shell/registry.ts` rather than left at `"db"` for nobody, and rather
than moved to `"mixengine"`: with D1 in place it has no reader, and a constant nothing reads is a
claim nothing checks. What replaces it is a sentence in the registry's own doc comment — **the
first module listed is also the tab this window opens by default** — next to the sentence that
already says the order is the app's.

`registry.test.ts` loses its `has a default module it also lists` case with the constant. What that
case protected — that the default is a module the registry actually has — is now true by
construction, because `defaultModuleId` returns an element of a list built by filtering `MODULES`.

## D5 — An empty list is a precondition, not a branch

`defaultModuleId([])` throws. It is left throwing, deliberately.

The obvious defensive line is `(visible[0] ?? MODULES[0]).id`, and it is worse than the throw. A
module id that is not in `enabled` makes `newTab()` build a tab the visibility effect in
`Workspace` immediately drops, which replaces it with another tab of the same hidden module — a
render loop, in place of one crash the error boundary above it cannot even help with. Trading a
loud stop for a silent spin is not defence.

The state is unreachable, three times over, and all three guards are T108's: `normalizeModules`
answers `null` for a set that leaves nothing to draw, the last remaining checkbox in the Modules
pane is disabled, and `useStartupProfile` never resolves to an empty list. So the function carries
a documented precondition — **`visible` is non-empty** — and the guards stay where they are, at the
edges where a bad value actually arrives.

## D6 — What this is not

**It is not T110.** A `<old>://` handoff that arrives while `db` is hidden still opens its tab:
`takeTabRequests` is handed all five ids and names the module itself, so no default is consulted.
What that tab should say, and what the Services screen's *open* button should offer, is T110's.

**It is not a redesign.** The *MixEngine* profile gets the same tab strip, now opening on the
Dashboard. A sidebar instead of a tab strip has no spec and is not this phase.

**It changes no stored value.** `<old>-modules` keeps its shape and its meaning, and a profile
written by T108 is read by this build with no migration.

## Verification

Pure logic, in vitest — the desktop frontend has no jsdom and no component tests, and this task
adds neither.

- `defaultModuleId` — **by name, per preset**: `MODULE_PRESETS.mixengine` → `"mixengine"`,
  `MODULE_PRESETS.everything` → `"mixengine"`, `MODULE_PRESETS.databaseTools` → `"db"`. Named
  rather than positional on purpose: the default is now implicit in the registry's order, and a
  module inserted at the head of `MODULES` would move it silently. These three cases are what say
  so out loud.
- `defaultModuleId` — the answer is the *first visible* module and not merely a visible one: a set
  of someone's own (`["terminal", "db"]`, given in that order) answers `"db"`, because
  `visibleModules` sorts it to the registry's order first.
- `registry.test.ts` — `leads with MixEngine` stays and now carries the default too; the
  `DEFAULT_MODULE_ID` case goes with the constant.

Nothing here says anything about CSS or about what a window looks like on the way up. Two paths are
worth walking by hand with `npm run dev:app`, on a profile with `<old>-session` cleared:

- *Everything* opens on a MixEngine tab showing the Dashboard, and `Ctrl/Cmd+T` opens another;
- *Database tools* opens on a Database tab, and `Ctrl/Cmd+T` opens another.

## What lands with it

- `apps/desktop/CLAUDE.md`'s layout table, which names `DEFAULT_MODULE_ID` as one of the three
  things `registry.ts` holds.
- The root `CHANGELOG.md`, under `## Unreleased` → `### Changed`, beside T108's line about the
  number chords.
- T109 ticked in `docs/roadmap/phase-13-profiles.md`, with what the task settled.
