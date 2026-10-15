---
status: implemented
date: 2026-10-15
task: T208a
---

# The `db` tab: a hook for the connection, parts for the form side (design)

`apps/desktop/src/modules/db/DbTab.tsx` is 1,038 lines and holds one component, `DbTab`. It is the
whole `db` tab: the saved list, the connection form and its feedback, connecting and disconnecting,
coming back after a restart or a handoff, the tab bar's badges, and the choice of workspace. Its
handlers share about thirty locals. Seventeen of those are `useState`. A component's callbacks
close over its state, so as with `SqlTable` in T207k this is not a move. It becomes **custom hooks
for what the tab does and components for what it draws**. The tab keeps only the choice between
the form and the workspace.

Roadmap: [phase 46](../roadmap/phase-46-a-database-module-of-one-shape.md), task T208a. M46 asks for
`DbTab.tsx` under 500 lines. D4 says what must not change.

## D1 — the files

The new files go into `src/modules/db/tab/`. T208c names `tab/` as the folder the tab's files end
up in, so they are not moved twice. `DbTab.tsx` stays where it is, together with `index.ts`'s
`lazy(() => import("./DbTab"))`. T208c moves it with the rest of the module's root.

| File | What it holds | Lines (est.) |
| --- | --- | --- |
| `tab/mongoUri.ts` | `mongoUriHost`, `mongoUriDatabase` and their regex (pure; moved as they are), with a `mongoUri.test.ts` for the seed-list and percent-encoding cases its comment names | 30 |
| `tab/useConnection.ts` | **A connection's lifecycle**: the connection id and its ref, the closed-tab flag and the unmount cleanup that hangs up, the one-dial-at-a-time ref and `connecting`, `status`, `error`, the tunnel test's `TunnelStatus`; `clearFeedback`, `testTunnel`, `connect`, `disconnect`; the fallback title a connection with no name gets | 220 |
| `tab/useConnectionForm.ts` | **What the form holds**: the form value, `set`, `changeKind`, the name box, `editingId`, the saved snapshot, `stale`, `focusPassword`; `applySaved`, `newForm`, `reloadSaved`, `fill` (a handoff), `save`, `saveAsNew`, `saveDisabled`; the effect that follows the saved entry when sync or another tab changes it | 200 |
| `tab/useTabArrival.ts` | **How the tab arrives**: the restore of last launch's connection and the take of a handed-over one, once, through one shared `restoreTried` ref | 90 |
| `tab/useTabBadges.tsx` | the badges memo and the effect event that reports them | 55 |
| `tab/useSavedList.ts` | **The saved list's view**: sorted order, the search and the engine chips, `engines`, `shown`, the context menu's position, the remembered scroll offset; pin, read-only and duplicate; the list's element refs and the layout effect that brings the marked row into view or restores the offset | 180 |
| `tab/SavedConnectionList.tsx` | the `<aside>`: header, search, chips, the two groups and their rows, the empty-filter state | 170 |
| `tab/SavedConnectionMenu.tsx` | the row context menu: pin, read-only, duplicate, delete | 60 |
| `tab/ConnectionWorkspace.tsx` | the dispatch to `SqlWorkspace` (in its `SqlProvider`), `MongoWorkspace` or `RedisWorkspace`, the "no workspace" fallback, and the two writes a workspace asks of the saved entry (sidebar width, Redis scan limit) | 140 |
| `DbTab.tsx` | calls the hooks, glues them (`openAndConnect`, Connect from the form, delete-then-reset), the form's notices, and the one choice: form side or workspace | 230 |

Every comment moves with the code it describes, word for word. A comment that points at a position
("the restore above", "the unmount above") and now crosses files is rewritten to name what it
meant. That goes in a commit of its own, as T207k did. `engines.test.ts` says it mirrors "the
if-chain at the end of `DbTab`". It will name `ConnectionWorkspace` instead.

## D2 — what each hook takes and gives

The hooks run in this order: `useConnection`, `useConnectionForm`, `useSavedList`,
`useTabArrival`, `useTabBadges`. Each takes one argument object and returns one object. A hook that
writes a sentence reads `t` from `useTranslation` itself.

- `useConnection({ onTitleChange, onStateChange })` → `{ connectionId, connecting, status,
  setStatus, error, setError, tunnelStatus, clearFeedback, isClosed, testTunnel, connect,
  disconnect }`.
  - It does not read the form. That removes the cycle a straight cut would have, where the form
    clears the connection's feedback and the connection reads the form. `connect` takes
    everything it used to read off the form as one request: `{ config, keyringRef, savedId, title?,
    name }`. Here `savedId` is the explicit id or `editingId`, and `name` is the name box, used for
    the fallback title.
  - `testTunnel(ssh)` takes the SSH half of the config, or nothing.
  - `disconnect(entry)` takes the saved entry the form holds, for the title and the tab state it
    writes back.
- `useConnectionForm({ savedConnections, savedConnectionsLoaded, onTitleChange, onStateChange,
  onReplaced })` → the form's state and the functions above. `onReplaced` is
  `connection.clearFeedback`. Loading another entry, or a new form, clears what the connection was
  saying, and `stale` is cleared by the hook itself. Together these are what `clearFeedback` did.
- `useSavedList({ savedConnections, savedConnectionsLoaded, connectionId, editingId })` → what `SavedConnectionList` and `SavedConnectionMenu` read.
  It is called in `DbTab`, not in the list, because the search, the chips and the scroll offset
  outlive the list. Today the list unmounts behind a workspace and comes back on Disconnect with
  the filters and offset it had. Held in the list component, they would reset.
- `useTabArrival({ restored, savedConnections, savedConnectionsLoaded, form, connection,
  openAndConnect, onTitleChange, onStateChange })` has no return value. It takes `parseDbTabState(restored)` once, as
  `useState` did.
- `useTabBadges({ connectionId, kind, readOnly, onBadgesChange })` has no return value.

`clearFeedback`, `isClosed` and `setStatus` become `useCallback`s with no dependencies. `isClosed`
reads the ref. That way the effects that now receive them as arguments list them without running
any more often than before.

## D3 — the components

`SavedConnectionList`, `SavedConnectionMenu` and `ConnectionWorkspace` take plain props. They call
no hook except `useTranslation`. Each renders exactly the elements `DbTab` renders today, with the
same classes, attributes and handlers, in the same place in the DOM: the menu still comes after
`<section className="login-form">`, which is why it is a component of its own. They use the global classes of `db.css`, as today. No CSS moves.

The list's layout effect stays keyed on `connectionId` and lives in `useSavedList`, which `DbTab`
calls, not in `SavedConnectionList`. In the component, StrictMode's second run of a layout effect
on mount found the list already seen and scrolled to the marked row over the offset it had just
put back. The browser run caught it: 30 against 526 after Disconnect. In `DbTab` the effect runs
when it always ran: on the commit that mounts the list again, once.

## D4 — what the change must not alter

- **Effect order where it is load-bearing.**
  - The connection id reaches its ref, and the unmount cleanup is registered, before anything can
    connect. Both stay first, in `useConnection`.
  - The restore and the handoff share `restoreTried` and keep their order inside `useTabArrival`.
- **What does move.** The follow-the-saved-entry effect now runs before the restore and the
  badges, not after. It returns at once while `editingId` is `null`, which it is at mount. It
  writes nothing the restore or the badge effect reads in the same commit, because each of them
  only sets state for the next render.
- **Every handler's behaviour.** The same state changes happen, in the same order, and an `async`
  function awaits what it awaited. One difference: Connect reads the form (`configFrom`) in
  `DbTab`, before `connect` checks whether a dial is already running. Today it reads the form after
  that check. `configFrom` is pure and has no throwing path, so the difference cannot be seen.
- **Rendered output.** The same DOM for the same state.
- **User-visible strings.** The same `t("…")` keys. The set of string literals before equals the set
  after, checked as T207k checked it.
- **Tests and lint.** `npm run build`, `npm test` (the count rises only by `mongoUri.test.ts`) and
  `npm run lint`. The hook dependency rule is included: every `useEffect`, `useCallback` and
  `useMemo` keeps its dependency list, or lint fails.

**The run in a browser.** As in T207k, a script drives the demo harness (`npm run dev`,
`demo/demo.html`). It is run once on `master` before the first commit and again at the end, and
after every step it records the tab title, the tab state sent to the shell, the badges, the list's
rows, marks, counts and `scrollTop`, the form's fields, Save's enabled state, the banners and
whether the workspace is showing. The steps:

- search, toggle an engine chip, clear filters;
- click a saved connection; edit a field (Save turns on); type the name back (Save turns off);
- pin, unpin and mark read-only from the context menu (the lock appears in the row and, once
  connected, in the tab bar);
- duplicate, then delete the copy;
- double-click to connect, then disconnect (title, state and list offset come back);
- scroll the list, connect and disconnect (the offset is the one it had);
- Test tunnel on a connection with SSH (the demo's answer, whichever it is);
- New connection (the form clears and the title says so).

The two recordings must be identical, and the console must show no errors.

**As run.** The demo has no fixture for `secrets_*` or `test_ssh_tunnel`, so the script answers
those four commands in the page. It also saves 18 extra connections so the list is long enough
to scroll. It took 26 snapshots, and those on `master` and on the finished branch are identical.
They were also identical after every commit from `useConnection` onwards. That is how the
StrictMode scroll issue under D3 was caught.
`npm run screenshots -- --check` renders 12/12 scenes. The real window (`npm run dev:app`) is
checked by hand for what the demo does not have: a real connection through an SSH tunnel, closing
a tab while it dials (the backend log shows the disconnect), and restarting the app with one tab
connected and one disconnected.

## D5 — commits

On a branch of its own, `t208a-db-tab-hooks`, since this works from a spec.

1. `mongoUri.ts` and its test.
2. `useTabBadges`.
3. `useConnection`.
4. `useConnectionForm`.
5. `useTabArrival`.
6. `useSavedList`, `SavedConnectionList`, `SavedConnectionMenu`.
7. `ConnectionWorkspace`.
8. Comment pointers, and `engines.test.ts`'s wording.
9. Roadmap and this spec's status.

Each commit passes `npm run build`, `npm test` and `npm run lint`.

## MixLab

This is MixLab's own `db` module. It touches nothing of MixEngine and works with `mixengined`
absent. The handoff `useTabArrival` takes is held by MixLab's backend, not the daemon. A user sees
the same tab. Nothing goes in `CHANGELOG.md`, because there is no change a user would notice. `mix`
has no part in this.

## Not in scope

- Moving `DbTab.tsx` itself, or any other root file, into `tab/`. That is T208c.
- Tests for the hooks. A hook test needs a renderer this project's vitest setup does not have.
  The pure halves (`connectionForm`, `savedListScroll`, `followSaved`, `tabState`,
  `handoffArrival`, `badges`) already have theirs.
- `TableStructure.tsx` and `Dashboard.tsx`. That is T208b.

## Is there an ADR here? No

## Documentation, when it lands

The phase file ticks T208a and links this spec, and this spec flips to `implemented`.
`apps/desktop/CLAUDE.md`'s layout tree gains `tab/` under `modules/db/`.
