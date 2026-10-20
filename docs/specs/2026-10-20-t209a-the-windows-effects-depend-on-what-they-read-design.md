---
status: approved
date: 2026-10-20
task: T209a
---

# The window's effects depend on what they read (design)

The frontend turns off `react-hooks/exhaustive-deps` nine times and `react-hooks/rules-of-hooks`
once. Each disable is a dependency list nobody checks any more: the next edit to the effect can
read something new and the lint says nothing. Five of the ten are in `shell/Workspace.tsx`, three
are the same "`lang` beside `t`" workaround copied into three files, and two are single effects
elsewhere. `Workspace.tsx` (625 lines, 11 effects) and `modules/rest/RestTab.tsx` (790 lines, 14
effects) are also the two largest components that have not been split into hooks and parts yet.

Roadmap: [phase 47](../roadmap/phase-47-code-that-is-safe-to-change.md), task T209a. M47 asks that
`Workspace.tsx` and `RestTab.tsx` turn off no React hooks lint. The rule for new code is in
[`docs/standards/desktop/component-structure.md`](../standards/desktop/component-structure.md),
*State and effects*.

## What was measured

`grep -rn "eslint-disable" src | grep react-hooks` in `apps/desktop` on `master` at `c242ddea`:

| # | Where | Rule | Why it was turned off |
| --- | --- | --- | --- |
| 1 | `shell/Workspace.tsx:68` | exhaustive-deps | `visible` is memoized on `enabled` flattened to a string, not on the array |
| 2 | `shell/Workspace.tsx:140` | exhaustive-deps | `openable` is memoized on the open module ids flattened, not on `tabs` |
| 3 | `shell/Workspace.tsx:202` | rules-of-hooks | `useShortcut` in a loop over `MODULES` |
| 4 | `shell/Workspace.tsx:302` | exhaustive-deps | the visibility effect calls `newTab`, rebuilt every render |
| 5 | `shell/Workspace.tsx:355` | exhaustive-deps | the tab-request drain listens once and calls `openTab`, rebuilt every render |
| 6 | `modules/db/components/CollationSelect/CollationSelect.tsx:83` | exhaustive-deps | `lang` beside `t` in a `useMemo` |
| 7 | `modules/db/components/QueryEditor/QueryEditor.tsx:402` | exhaustive-deps | `lang` beside `t` in a `useMemo` |
| 8 | `modules/db/redis/RedisWorkspace.tsx:419` | exhaustive-deps | `lang` beside `t` in a `useMemo` |
| 9 | `modules/db/tab/useTabArrival.ts:112` | exhaustive-deps | the handoff runs once per tab and calls `connect` and two callbacks rebuilt every render |
| 10 | `components/dialogMotion.ts:53` | exhaustive-deps | `enterModal(visible)` once per mount |

None of the ten gives a reason a stable callback or an effect event cannot fix. So the target is
**no `react-hooks` disable anywhere in `src/`**, and the list in the phase file of disables that
stay is empty.

## D1 — the ten disables

Each fix keeps the effect running at exactly the moments it runs today. The tool for "a function
rebuilt every render that the effect must not re-run for" is `useEffectEvent` (React 19.2, already
used 25 times here and understood by `eslint-plugin-react-hooks` 7).

- **#1 `visible`.** `useMemo(() => visibleModules(idsOf(enabledKey)), [enabledKey])`, where
  `idsOf` splits the key back into ids (`""` gives `[]`). The memo reads only the key, so the key
  is its whole dependency list and the lint has nothing to say.
- **#2 `openable`.** The same: `openableModules(visible, idsOf(openModuleKey))`.
  `openableModules` in `shell/tabs.ts` takes the open module ids instead of the tabs, which is all
  it ever read of them. Its test changes its arguments with it.
- **#3 the number chords.** A new hook in `core/shortcuts`, `useShortcuts(entries: { id, handler,
  enabled }[])`, registers a list in **one** effect. Its dependency is the enabled ids flattened to a
  string; handlers are read through a ref, as `useShortcut` reads its one. `Workspace` calls it once
  with one entry per `MODULES` item. One difference: turning a module on or off re-registers all five
  chords, in registry order, where today only that module's is torn down and added at the end. The
  order of registrations matters only to two ids that answer the same chord (`decide.ts`), and no
  chord in the catalogue is a digit except these five, each on its own digit. So nothing a user can
  press behaves differently.
- **#4 `newTab`.** A `useCallback` over `[visible, t]`. The visibility effect lists it. `visible`
  changes in the same commit as `visibleIds`, which is already listed, and `t` never changes, so the
  effect runs no more often than it does now.
- **#5 the drain.** The effect keeps an empty dependency list and keeps listening once. What it does
  with a batch of requests (the union of modules to turn on, `onEnabledChange`, `openTab`, the
  notices) becomes an effect event called after the `await`. It reads `enabled` as it is at the
  moment the batch arrives, which is what `enabledRef` exists for today, so `enabledRef` goes.
- **#6–#8 `lang` beside `t`.** `t` is one function for the life of the app on purpose
  (`i18n/index.tsx`). A `useMemo` that has turned words into a value has to be rebuilt on a language
  switch, and today says so by listing `lang` it does not read. One shared hook replaces the three
  copies: `useLanguageT()` in `i18n/index.tsx` returns a translate function that is **a new function
  per language**, built with `useMemo` over `lang` from that language's dictionary. A memo that uses
  it lists it like any other value and is rebuilt on a switch for that reason. The words come from
  the same lookup `t` uses (`resolve` and `interpolate`, shared, not copied). `t` itself does not
  change. `Workspace`'s tray effect, which lists `lang` beside `t` with no disable (an effect may
  list more than it reads), moves to the same hook so there is one way to say it.
- **#9 the handoff.** The body after the guard becomes an effect event; the effect keeps
  `[restoredState]` and the shared `restoreTried` ref. It runs once per tab, as now.
- **#10 the modal count.** `const enter = useEffectEvent(() => enterModal(visible))`, and the effect
  with `[]` calls it. `enterModal` is handed the `visible` of the first render, as now.

## D2 — `Workspace.tsx` into hooks and parts

Under the `splitting-a-react-component` skill, as T208a split `DbTab.tsx`. The new files go into
`src/shell/workspace/`. `Workspace.tsx` stays where it is, since `App.tsx` imports it.

| File | What it holds | Lines (est.) |
| --- | --- | --- |
| `workspace/useTabs.ts` | **The strip's state**: `tabs`, `activeId`, `mounted`, `enabledFor`; `newTab`, `openTab`, `openOrGoTo`, `closeTab`, rename, badges, state; the layout effect that keeps `activeId` on a real tab, the effect that mounts what comes to the front, the visibility effect, and the session write | 190 |
| `workspace/useTabRequests.ts` | the drain of tabs the backend asks for (#5) | 70 |
| `workspace/useSyncClosing.ts` | `startSync`, the signed-in server's closing date read at launch and on `SYNC_NOW_EVENT`, and its dismissal for this run | 50 |
| `workspace/useWindowChrome.ts` | what the window tells the OS: the tray's menu words, the uninstall menu item and its request, the refused reload keys | 60 |
| `workspace/WorkspaceStrip.tsx` | the `TabStrip` with its brand, Settings button, `[+]`, the tabs, and the `[+]` menu (its position is this part's state: the strip is always drawn) | 170 |
| `workspace/TabPanes.tsx` | `tab-content`: one `ErrorBoundary` + `Suspense` + module `Tab` per mounted tab, the notice wrapper, `DialogHost` | 90 |
| `Workspace.tsx` | `visible`/`openable`/`shortcuts`, the shortcuts, Settings and its section, updates, the remove dialog, the closing strip; calls the hooks and draws the parts | 220 |

Hooks run in this order: `useSyncClosing`, `useWindowChrome`, `useTabs`, `useTabRequests`. Each
takes one argument object and returns one object. A function a part or an effect receives from a
hook is a `useCallback` with the dependencies it had.

## D3 — `RestTab.tsx` into hooks and parts

The same skill. The new files go into `src/modules/rest/tab/`.

| File | What it holds | Lines (est.) |
| --- | --- | --- |
| `tab/useOpenRequests.ts` | **The request strip**: `openIds`, `activeId`, the restore of last launch's strip and its two refs, the state reported to the shell, the effects that drop a deleted request's tab and move the choice; `tabs`, `activeRequest`, `currentId`, `open`, `close` | 150 |
| `tab/useSends.ts` | `sends`, `inFlight` and the unmount cleanup that cancels them, `send`, `cancel`, dismissing an error | 160 |
| `tab/useEnvironmentChoice.ts` | `envId`, the one-time seed from `lastEnvId`, `env`, `chooseEnv` | 40 |
| `tab/usePasteRequest.ts` | `pasteInto`, `pasteIntoUrl` and its flag, the swap offer, the document `paste` listener of an empty workspace | 90 |
| `tab/usePaneSizes.ts` | the sidebar width and split ratio, following the workspace file, and the drag handlers for the two splitters | 60 |
| `tab/RequestPane.tsx` | the request `<section>`: `UrlBar`, `UrlPreview`, the Params/Body/Headers/Auth strip and its body. Takes props only: it is drawn only while a request is open, so the chosen pane per request stays in `RestTab` | 120 |
| `tab/SwapDialog.tsx` | the "put the variables back" confirmation and its list | 50 |
| `RestTab.tsx` | calls the hooks, the derived `folded`/`resolved`/`preview`/`blocked`, the title, the four shortcuts, and lays out sidebar, splitters, panes and dialogs | 250 |

`requestTabs`, `preferredModes` and `headersOpen` stay in `RestTab`, keyed by request id, so
switching requests keeps each one's pane, as today.

## D4 — what the change must not alter

- **When each effect runs.** Every effect keeps the moments it runs at. Where its dependency list
  changes in text (#1–#5, #9, #10), D1 says why the moments do not. Effects that move into a hook
  keep their relative order inside it; any effect that now runs in a different order relative to
  another is written down in the commit that moves it, with why nothing it reads changed.
- **Rendered output.** The same DOM for the same state, the same classes, the same strings. The set
  of string literals before equals the set after, checked with `grep -o` and `comm` as the skill
  says.
- **Comments.** Every comment moves with its code. One that points at a position ("the effect
  above", "the drain below") and now crosses a file names what it meant.
- **Tests and lint.** `npm run build`, `npm test` and `npm run lint` pass after every commit. The
  test count rises only by tests added for `idsOf`, `useLanguageT`'s pure half and
  `openableModules`' new signature.

**The run in a browser.** A step script on the demo harness (`npm run dev`, `demo/demo.html`), as
the skill describes, recorded twice on `master` to prove it deterministic, then after every commit.
It records the shell strip (titles, badges, active tab), the session written to `localStorage`, the
REST tab's strip, URL box, preview line, pane tab and response, and the console between two marks.
The steps:

- shell: open a tab of each module from `[+]`, `Ctrl+T`, `Ctrl+1`…`5`; close, reorder by drag,
  `Ctrl+Tab`; turn a module off in Settings (its tabs go) and on again; switch language (tab titles,
  the tray words, a collation list, the Redis limit picker change language); reload and check the
  session comes back;
- REST: New request (the URL box has the keyboard); type a URL with a query; pick an environment;
  switch the pane strip; paste a cURL command into an empty workspace and into a blank request (the
  swap offer when the environment matches); send against the demo's answer; open two requests,
  close one, delete one from the sidebar; drag both splitters; reload and check the strip comes
  back.

The recordings must be identical, and the console must show no new errors or warnings.
`npm run screenshots -- --check` renders every scene. The real window (`npm run dev:app`) is checked
by hand for what the demo cannot do: a `mixlab://` handoff to a module that is turned off (it opens
with the notice), the tray menu's words after a language switch, and closing a REST tab while a
send is in flight.

## D5 — commits

On a branch of its own, `t209a-window-effects`, since this works from a spec.

1. `useLanguageT`, and #6–#8 plus the tray effect use it.
2. #9 and #10.
3. `useShortcuts`, and #3.
4. `idsOf`, `openableModules` by ids, and #1, #2.
5. `newTab` and the drain (#4, #5).
6. `Workspace` → `useSyncClosing`, `useWindowChrome`.
7. `Workspace` → `useTabs`, `useTabRequests`.
8. `Workspace` → `WorkspaceStrip`, `TabPanes`.
9. `RestTab` → `usePaneSizes`, `useEnvironmentChoice`.
10. `RestTab` → `useOpenRequests`.
11. `RestTab` → `useSends`, `usePasteRequest`.
12. `RestTab` → `RequestPane`, `SwapDialog`.
13. Comment pointers that crossed files.
14. Roadmap, `apps/desktop/CLAUDE.md`'s layout tree, and this spec's status.

## MixLab

This is MixLab's own shell and `rest` module. It touches nothing of MixEngine and works with
`mixengined` absent: the drain of tab requests reads MixLab's backend, not the daemon. A user sees
the same window. Nothing goes in `CHANGELOG.md`, because there is no change a user would notice.
`mix` has no part in this.

## Not in scope

- The forms with many `useState` (`Languages.tsx`, `TargetForm.tsx`, `RedisWorkspace.tsx`, …).
  That is T209f, which comes after this task.
- Tests for the hooks themselves. A hook test needs a renderer this project's vitest setup does not
  have; the pure halves keep theirs.
- Moving any other module's components.

## Is there an ADR here? No

## Documentation, when it lands

The phase file ticks T209a, links this spec, and says no hooks-lint disable is left. This spec
flips to `implemented`. `apps/desktop/CLAUDE.md`'s layout tree gains `shell/workspace/` and
`modules/rest/tab/`. `component-structure.md`, *State and effects*, names `useLanguageT` as the
shared workaround for a memo that turns words into a value.
