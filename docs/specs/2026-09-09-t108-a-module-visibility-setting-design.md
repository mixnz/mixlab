---
status: implemented
date: 2026-09-09
task: T108
---

# T108 — A module visibility setting with three presets

Roadmap task [T108](../roadmap/phase-13-profiles.md), on
[the desktop client design](2026-09-08-the-desktop-client-in-this-repository-design.md)'s D11 and
on [T104](2026-09-09-t104-the-application-is-mixlab-design.md)'s import. 2026-09-09.

ADR 0027 opens with two complaints. The first — *two downloads* — was answered by phase 12. This
task answers the second: **a database client I did not want**. A person who installed MixEngine to
run PHP opens MixLab and finds a tab bar offering five things, four of which are somebody else's
product, and a Settings dialog with four panes about them.

Nothing here is a build flag. **One build, one artifact, and what differs is a setting** — ADR
0027's rule 4 — so this is a list of module ids in the shell's settings, three presets that write
it, and one screen on first run that asks which. No module is touched, no API method is added, and
`MODULES` still lists all five in every build.

## What is already true

Written down so nothing below is built twice:

- **`shell/registry.ts` is the only file outside `src/modules/` that names a module.** The eslint
  boundary in `apps/desktop/eslint.config.js` enforces the import half of that; the naming half is
  a convention. Presets are a list of module ids, so they belong in that file and nowhere else.
- **The shell keeps its own settings in `localStorage`**, not in the store plugin — `shell/theme.ts`
  and `shell/session.ts` say why: a handful of strings about the window, read once on the way up,
  and an async file read would mean an empty tab bar for the first frame of every launch.
- **Everything that enumerates modules already reads one list.** `shell/shortcuts.ts` derives the
  number chords from `MODULES`, `App.tsx` registers its handlers from that same derivation,
  `SettingsModal` builds its pane column from `MODULES`, and `session.ts` is handed the ids rather
  than reading them. There is no second list to find.
- **T104's import leaves a marker.** `src-tauri/src/import.rs` writes `<old>-import.json` into the
  application-data directory when, and only when, a standalone-client user's stores came across. Its doc
  comment already says it is there for this task.
- **The webview's `localStorage` did not come across.** The import is explicit about it: a profile
  keyed by the bundle identifier, unreachable from the process, and theme and the last tab strip
  are not things a person made. So an imported user's window has store files on disk and an empty
  `localStorage` — which is exactly the state a brand-new install is in, and the reason the marker
  has to be readable from the frontend.

## D1 — The order: MixEngine first

`MODULES` becomes `[mixengine, db, rest, terminal, tools]`.

D11's table asks for *Everything* to be "all five, the standalone client's order with `mixengine` first". That
ordering can live in the stored value or in the registry, and only one of those two is safe.

In the stored value, order becomes user state with no control that sets it: the checkboxes below
would have to decide where a re-enabled module lands, `Ctrl/Cmd+1` would move when a module three
places away is toggled, and a hand-edited or half-written array would carry an order nothing can
check. In the registry it is one line, and all three rows of D11's table fall out of a single rule:

```ts
visibleModules(enabled) === MODULES.filter((m) => enabled.includes(m.id))
```

| Preset | `enabledModules` | Visible, in order |
| --- | --- | --- |
| **MixEngine** | `["mixengine"]` | MixEngine |
| **Everything** | all five | MixEngine, Database, REST, Terminal, Tools |
| **Database tools** | `["db", "rest", "terminal", "tools"]` | Database, REST, Terminal, Tools |

`enabledModules` is therefore a **set**, stored as an array and read as one. Order never comes out
of it.

**This remaps `Ctrl/Cmd+1 … 5` for anyone who was using the standalone client**: `Ctrl+1` was Database and becomes
MixEngine. It is a deliberate change and it goes in the changelog. The `[+]` menu leads with
MixEngine for the same reason — this is MixEngine's window now.

## D2 — `enabledModules`, and where it lives

`localStorage`, key `<old>-modules`, beside `<old>-theme`, `<old>-accent`, `<old>-glass` and
`<old>-session`. The prefix is the origin's rather than the product's; renaming all five is a
migration for nothing, and mixing two prefixes in one origin is worse than an old name.

A new file, `src/shell/profiles.ts`, holds the mechanism. It names no module — the ids come from
`registry.ts` — and every function that decides anything is pure and takes what it needs:

```ts
/** The ids in `stored`, as a set this build can act on, or `null` when there is nothing usable.
 *  Unknown ids are dropped, duplicates collapse, and an empty result is `null` rather than []. */
export function normalize(stored: unknown, knownIds: string[]): string[] | null;

/** Which preset this set is, or `null` for a set of someone's own. Set equality, not order. */
export function presetOf(enabled: string[]): PresetId | null;

/** The modules to draw, in registry order. */
export function visibleModules(enabled: string[]): ModuleDefinition[];

/** What `Ctrl/Cmd+T` and a plain `[+]` open: the registry's default when it is visible, and the
 *  first visible module when it is not. T109 makes the choice itself a function of the profile. */
export function defaultModuleId(visible: ModuleDefinition[]): string;
```

`normalize` returning `null` for an empty list is the first of three guards against a window with
no modules in it. The other two are in D5 and D6.

## D3 — First run, and the three things that are not one

D11 says the screen appears on "a directory with no settings". Taken literally — *no
`<old>-modules` key* — it fires in a case it must not: a **MixLab user upgrading past this task**
has a theme, a session and four modules' worth of saved work, and no `enabledModules`, and would
be handed the *MixEngine* preset and watch their database tabs disappear. That is the exact user
phase 12 spent itself bringing across.

So "no settings" is asked as three questions, in order, and only the third is a screen:

```
enabledModules stored and usable      → use it.                       (synchronous)
else some other shell key is present  → Everything, written down.     (synchronous)
else the import marker is there       → Everything, written down.     (one invoke)
else                                  → the first-run screen.
```

**Question 2** is `<old>-session`, `<old>-theme`, `<old>-accent`, `<old>-glass` or `<old>-lang`:
any of them present means this webview profile has been used by a build older than this task.
None of them can be written before the first-run screen is answered — `theme.ts` only ever
*removes* a key for a default value, the language is changed from inside Settings, and
`writeSession` runs in an effect of the workspace, which D4 does not mount until the profile is
decided. So the evidence is one-directional and cannot be manufactured by the screen itself.

**Question 3** is the import marker, and it is the only asynchronous step. It costs one `invoke` on
a genuinely fresh profile and nothing at all on every other launch — the fast path never awaits.

**Nothing leaves the screen without writing `enabledModules`.** D11's "a fresh machine that skips it
gets *MixEngine*" is a decision, not a deferral: if it were not written, question 2 would answer
*Everything* on the next launch, because the workspace would by then have written a session. The
screen has three buttons and no dismiss; `resolve(null)` answers the *MixEngine* preset for any
caller that needs a value without asking.

### The command

`src-tauri/src/import.rs` gains one:

```rust
/// Whether a standalone-client user's data was brought across on this machine — the marker T104 leaves.
#[tauri::command]
pub fn import_happened(app: AppHandle) -> bool;
```

Registered in `modules::handler()`'s `── shared ──` block. The decision itself is a free function
over a path so it can be unit-tested without a runtime, the way the rest of that module is. The
marker is written synchronously inside `setup()`, before the event loop can deliver an IPC message,
so by the time the webview can ask, the answer is final. A failed `invoke` — `npm run dev` in a
browser, a command that is somehow not registered — reads as `false`, which shows the screen: one
click, and the wrong answer is a screen rather than a silently wrong profile.

## D4 — The gate: `App` decides, `Workspace` runs

`App.tsx`'s body moves to `shell/Workspace.tsx` unchanged, and `App` becomes the gate:

```tsx
function App() {
  const startup = useStartupProfile();
  if (startup.status === "deciding") return null;           // one invoke, fresh profiles only
  if (startup.status === "asking") return <FirstRun onChoose={startup.choose} />;
  return <Workspace enabled={startup.enabled} onEnabledChange={startup.setEnabled} />;
}
```

This split is not cosmetic. `Workspace` decides its whole initial state in `useState` initializers —
which tabs to restore, which of them is active, which one to mount — and every one of those reads
the visible module list. A gate that ran *inside* the component would have to run before its own
first render, which is what a parent is.

`useStartupProfile` resolves synchronously whenever it can, so an existing user never renders the
`deciding` state and there is no flash. `deciding` renders `null` rather than a spinner: it lasts
one IPC round trip on a window that has nothing on it yet, and a spinner that appears and vanishes
inside a frame is worse than a blank one.

## D5 — What reads through `visibleModules()`

Five places, and they are the five D11 lists.

**The `[+]` menu.** Built from the visible list; the `MODULES.length < 2` shortcut that opens a tab
outright instead of a menu of one becomes `visible.length < 2`, which is the *MixEngine* profile's
normal state.

**`Ctrl/Cmd+T`.** Opens `defaultModuleId(visible)`.

**`Ctrl/Cmd+1 … N`.** The chords index into the visible list, so both the chord a module carries and
the set of live chords change with the profile — and this is the one part of the task with a
constraint of its own. `App.tsx` registers these handlers in a `for` loop, which is legal today only
because `MODULE_TAB_SHORTCUTS` is a module-level constant with a fixed length. A list that shrinks
with the profile would change the hook count between renders and break React outright.

So the two halves are separated:

- the **loop stays over `MODULES`** — five iterations, every render, for the life of the app — and
  each registration passes `enabled: visible.includes(module.id)`. `useShortcut` registers nothing
  when `enabled` is false, so a hidden module's chord simply has no listener.
- the **catalogue becomes a function**: `moduleTabShortcuts(visible)` gives each visible module
  `chord: { key: String(i + 1) }` and keeps its id, `app.newTab.<moduleId>`. Ids stay named after
  the module, so nothing that files against an id — a future remap screen — has to know about
  positions.

`shortcutsFor(visible)` assembles the whole catalogue and is memoized in `Workspace` on the joined
id list, because `useShortcutDispatcher` documents that its argument must be stable. That same
memoized value is handed to `SettingsModal` and on to `ShortcutsSection`, which imports
`ALL_SHORTCUTS` today: the shortcut table's whole claim is that it cannot say one thing while the
app does another, and a module-level constant beside a catalogue that changes would break exactly
that. Passed rather than read back through `currentCatalogue()`, because the dispatcher writes that
in an effect — one render later than the table wants it.

**The Settings dialog's panes.** `SECTIONS` stops being a module-level constant and is computed
from the visible list inside the component. A module hidden while its own pane is open would leave
`section` pointing at nothing, so the pane actually drawn is derived rather than stored:
`SECTIONS.some(s => s.id === section) ? section : "appearance"`. Derived and not repaired in an
effect, so there is no frame in which the dialog has no pane.

**Session restore.** `readSession` is already handed the ids it should accept; it is handed the
visible ones instead of all five. A tab of a hidden module is dropped rather than mounted blind —
which is the same treatment `parseSession` already gives a tab of a module that no longer exists,
and it needs no new code, only a different argument.

## D6 — Turning a module off

Two things happen and they happen in this order: the setting changes, and then the tabs close.

**The confirmation gates the setting**, in the Settings pane, before `onEnabledChange` is called —
it is the only moment at which the answer can still be "no". It appears only when a module being
turned *off* has at least one open tab, it names those modules, and it says that nothing saved is
deleted — which is the question a person actually has at that moment. Turning a module *on* never
asks.

**The closing is an effect** on the visible id list, in `Workspace`:

```ts
useEffect(() => {
  setTabs((prev) => {
    const kept = prev.filter((tab) => visibleIds.includes(tab.moduleId));
    return kept.length === prev.length ? prev : kept.length > 0 ? kept : [newTab()];
  });
  setMounted((prev) => prev.filter(/* … */));
}, [visibleIds]);
```

An effect rather than a branch inside the Settings handler, because there is more than one way the
list can change — the presets, the checkboxes, and T110's "a handoff enables the module for that
tab" — and a rule enforced at one call site is a rule that the second call site breaks. Returning
`prev` unchanged when nothing was dropped keeps it from writing the session on every render.
`activeId` needs no work: the `useLayoutEffect` that already keeps it pointing at a real tab does
it, and it did before this task.

**A window with no modules is impossible**, three times over: `normalize` refuses an empty set, the
last remaining checkbox is disabled with a title saying why, and `resolve` never returns an empty
list. The first is the one that matters — it is what catches a hand-edited `localStorage` and a
half-written value.

**Nothing is deleted.** Not a store file, not a keyring entry, not a saved connection. M13's
"turning them off again closes their tabs and hides them without deleting anything" is the whole
of what off means, and turning the module back on finds everything where it was.

## D7 — The screens

**First run** — `shell/components/FirstRun/`, one question and three cards:

> **What will you use MixLab for?**
> *MixEngine* — Sites, runtimes and services. The window MixEngine ships with.
> *Everything* — MixEngine, and the database, REST and terminal tools.
> *Database tools* — The database client, REST client, terminal and tools.
>
> You can change this in Settings at any time.

Full-window rather than a modal over a workspace: a dialog would need a workspace behind it, and
the workspace cannot be built until the question is answered. A heading, three buttons, the first
one focused, and no dismiss control.

**Settings → Modules** — a new pane, and it goes **first**, above Appearance. It is the setting
that decides which of the panes below it exist, and a reader who watches panes appear and disappear
should find the control above them rather than below.

The three presets as a row of choices with the matching one marked — `presetOf` returns `null` for
a set of someone's own, and then none is marked — and the five checkboxes under them, in registry
order, each labelled with the module's own name from the registry.

Both screens' strings go into `i18n/en.ts` and `i18n/vi.ts` under a new `profiles` block.

## D8 — What this is not

**It is not a security boundary.** A hidden module keeps its Tauri commands registered, its state in
the backend and its files on disk; `enabledModules` is a list the frontend draws from. Anything
running in the webview can invoke a hidden module's commands exactly as it could before. Visibility
is the whole claim, and it is the right claim — M13 requires that turning a module off delete
nothing — but it must not be mistaken for a capability gate later.

**It is not T109.** `defaultModuleId` is clamped to something visible here because hiding `db` while
`DEFAULT_MODULE_ID` is `"db"` would open a tab of a module the user just turned off. Which module
the *profile* prefers — the Dashboard for *MixEngine* and *Everything*, a Database tab for
*Database tools* — is T109's, and it changes this one function.

**It is not T110.** A `<old>://` handoff that arrives while `db` is hidden still opens its tab:
`takeTabRequests` is handed all five ids, so nothing is left rotting in the backend's queue, and
`moduleById` draws the tab because `MODULES` is unchanged. What that tab should *say*, and what the
Services screen's *open* button should offer instead, is T110's.

**It redesigns no window.** The *MixEngine* profile gets the same tab strip with one entry in the
`[+]` menu. A sidebar instead of a tab strip has no spec and is not this phase.

## Verification

Pure logic, in vitest:

- `normalize` — an unknown id dropped; duplicates collapsed; `[]`, `null`, a string, an object and a
  half-written array all `null`; a known set through unchanged.
- `presetOf` — each preset recognised from its set in any order; a set of someone's own `null`.
- `visibleModules` — registry order regardless of the order of the ids given.
- `defaultModuleId` — the registry default when visible, the first visible module when not.
- `resolve` — the four branches of D3, against injected storage rather than the real one.
- `moduleTabShortcuts` — 1…N over the visible list; ids still named after the module; no chord
  claimed twice; every def on the catalogue `shortcutsFor` returns. The existing
  `shortcuts.test.ts` cases become these, parameterised by a visible list.
- `parseSession` — a tab of a module that is hidden is dropped exactly as one of a module that does
  not exist is. One case, because it is one code path.

In Rust: `import_happened`'s free function over a directory with and without the marker.

Neither says anything about CSS or about whether the command is registered — `npm run dev:app` and
a click are what prove those, on the four paths worth walking by hand: a profile wiped to nothing
(the screen), a profile with a session and no `<old>-modules` (*Everything*, no screen), turning
*Database tools* off with tabs open (one confirmation, four tabs gone, a MixEngine tab left), and
turning them back on (four panes in the `[+]` menu, every saved connection still there).
