# Adding a module

A module is one kind of thing a tab can hold. There are six — `mixengine`, `db`, `rest`, `terminal`,
`tools` and `tunnel` — and the steps below are what each of them did. Read the newest one alongside this: `terminal` is the
module that has been through the fewest changes of mind.

The shell knows only what [`src/shell/module.ts`](../../../apps/desktop/src/shell/module.ts) declares, so a module
is a folder plus a line in the registry — see [overview](../../architecture/desktop/overview.md) for why the
contract is as small as it is.

## Frontend

1. **`src/modules/<id>/index.ts`** exporting a `ModuleDefinition`:

   ```ts
   import type { ModuleDefinition } from "../../shell/module";
   import { TerminalIcon } from "../../icons";
   import TerminalTab from "./TerminalTab";

   export const terminalModule: ModuleDefinition = {
     id: "terminal",
     labelKey: "app.moduleTerminal",
     Icon: TerminalIcon,
     defaultTitleKey: "terminal.newTabTitle",
     Tab: TerminalTab,
   };
   ```

2. **The tab component** takes `ModuleTabProps` — `active`, `onTitleChange`, `onBadgesChange`,
   `restored`, `onStateChange` — and nothing else. `active` is false while the tab is behind
   another, and it stays mounted, so anything that grabs the keyboard has to check it. (A tab
   restored from the last session is not mounted until it is first picked — but nothing a module
   writes can tell: its first render is its first render either way.)

   Report a `TabBadge` for each mark the tab should carry. `className` styles the badge;
   `tabClassName` goes on the whole tab, for a mark that tints it. `label` is read aloud and
   `title` is the tooltip — the database module leaves `title` off the engine logo and sets it on
   the lock, which is what those two did before there was a contract.

   Report none and the shell draws the module's `Icon` instead, so a module that has no mark of
   its own needs no badge at all — and neither does one whose only mark would be that icon.

   `restored` / `onStateChange` are how a tab comes back to what it had open. Three rules, none of
   them typechecked:

   - **Read `restored` once, at mount** — a `useState` initializer — and work from that snapshot.
     Read it reactively and the module overwrites itself the moment it writes. Reading once is not
     acting once: a module whose store still has a file to read waits for it, which is what
     `useSavedConnectionsLoaded()` and its two siblings are for, and it keeps a ref so a snapshot
     published by another tab does not restore a second time.
   - **Call `onStateChange` from an event handler**, or from an effect that does not list it as a
     dependency. The shell compares state by identity, `App` hands down a fresh closure every
     render, and the two together are the loop named at the top of `shell/tabs.ts`.
   - **Ids only.** This is `localStorage`: no host, no password, no URL, no token. Put the shape in
     `modules/<id>/tabState.ts` with a `parseXTabState(value: unknown)` beside it — the shell
     passes the slot through without validating it, so that function is where the checking lives.
     See [the spec](../../specs/2026-08-23-tab-session-context-design.md).

3. **One line in [`src/shell/registry.ts`](../../../apps/desktop/src/shell/registry.ts)**, in `MODULES`. This is
   the only file outside `src/modules/` that may name a module.

4. **Strings** in `src/modules/<id>/i18n/{en,vi}.ts`, added to
   [`src/i18n/dicts.ts`](../../../apps/desktop/src/i18n/dicts.ts) — and to the hand-merged `error` group there if
   the module raises errors of its own. Its dictionaries must import nothing from `src/i18n/`; see
   [i18n](i18n.md).

5. **Global CSS**, if any, in `src/modules/<id>/<id>.css`, imported from the tab component.
   Component-scoped rules go in each component's CSS Module instead. Order between stylesheets is
   decided by Vite from the import graph — a rule that must beat one in `shell/App.css` wins on
   specificity, not on order.

6. **A Settings pane**, if the module needs one, through `ModuleDefinition.settings`. The shell's
   dialog builds its list from `MODULES`, so the module names its own pane and the shell never
   learns what is in it.

## Backend, if it needs one

7. **`src-tauri/src/modules/<id>/mod.rs`** with a `register`:

   ```rust
   pub fn register<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
       builder.manage(state::TerminalState::default())
   }
   ```

   Tauri keys managed state by type, so this never meets another module's.

8. **One line in `lib.rs`**: `let builder = modules::terminal::register(builder);`

9. **A block in `modules::handler()`** in `src-tauri/src/modules/mod.rs`. One list, because Tauri
   takes exactly one `invoke_handler` — but the blocks are per module and only that module edits
   its own. A command missing from the list does not exist at runtime and nothing at build time
   says so.

   `error.rs`, `secrets.rs` and `ssh/` are shared; use them rather than copying.

## Check the boundary before you commit

No file outside `src/modules/<id>/` may know that module's concepts, and **nothing typechecks
this** — a primitive importing from `modules/db/` compiles perfectly. One command:

```
npm run lint
```

`no-restricted-imports` in `eslint.config.js` covers every `.ts`/`.tsx` under `src/components`,
`src/core`, `src/icons`, `src/shell` and `src/i18n`, and fails on any import naming a module.
`src/shell/registry.ts` and `src/i18n/dicts.ts` are the two exceptions — the places a module is
joined to the app, one line per module in each.

CI runs it on every push, so the boundary no longer depends on anyone remembering this page. And
because it is the linter and not a script of our own, the editor says it while the import is being
typed rather than ten minutes later.

It matches import *specifiers* rather than the text `modules/`, which the two PowerShell greps
that used to be here did. Those matched any mention at all, including the prose in
`src/shell/shortcuts.ts` explaining this very rule — so they reported two violations on a
perfectly clean tree. A check that is red when the code is fine gets ignored, and then it catches
nothing. A bespoke script stood here in between, and it got this right too; the linter replaced it
because one rule deserves one checker, and this one now sits where every other rule about the code
already lives.

## What the second and third modules found

All three are settled now, and all three are worth knowing before adding a fourth:

- **The `[+]` menu.** With one module the button opens a tab outright; with more it opens a menu.
  Both branches are live in `shell/Workspace.tsx`, and they count the modules a profile leaves
  **visible** rather than every module in the registry — see `shell/profiles.ts`.
- **Shortcuts are contributed, not registered centrally.** A module's chords go in
  `src/modules/<id>/shortcuts.ts` and reach the dispatcher through `ModuleDefinition.shortcuts`;
  `src/core/shortcuts/` may not import from `shell/` or `modules/` at all. See
  [frontend](../../architecture/desktop/frontend.md).
- **Secrets and `ssh/` are shared, host lists are not.** The terminal keeps its own saved hosts
  rather than reaching into the database module's — two modules wanting the same *shape* is not two
  modules wanting the same *data*.
