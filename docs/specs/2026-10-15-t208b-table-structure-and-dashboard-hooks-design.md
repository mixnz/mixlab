---
status: implemented
date: 2026-10-15
task: T208b
---

# Table Structure and the Dashboard: hooks for what they do, parts for what they draw (design)

Two components hold a whole screen each:

- `apps/desktop/src/modules/db/components/TableStructure/TableStructure.tsx` is 1,109 lines. It
  is the Structure tab of every SQL engine: the columns grid, the indexes grid, ClickHouse's skip
  indexes, five dialogs, and every `ALTER TABLE` they send. It has eleven `useState` calls.
- `apps/desktop/src/modules/mixengine/screens/Dashboard/Dashboard.tsx` is 1,107 lines. It is
  MixEngine's first screen: the daemon's header, the services table and the stream that keeps it
  current, the jobs list, `/metrics`, the disk panel, a row menu and four dialogs. It has
  twenty-four `useState` calls.

Their callbacks close over their state, so as with T207k and T208a this is not a move. Each
component gets **hooks for what it does and components for what it draws**. Each ends under
500 lines, which M46 asks of both.

Roadmap: [phase 46](../roadmap/phase-46-a-database-module-of-one-shape.md), task T208b. It
follows [T208a's design](2026-10-15-t208a-db-tab-connection-lifecycle-design.md). D4 says what
must not change.

## D1 — TableStructure's files

Everything stays in `components/TableStructure/`, beside the stylesheet the parts share. The
folder's `index.ts` keeps exporting `default`, `RememberedStructure` and `StructureCache`, so
`SqlWorkspace.tsx` does not change.

| File | What it holds | Lines (est.) |
| --- | --- | --- |
| `shape.ts` | Pure, moved as they are: `INDEX_KIND_LABEL`, `indexMethod`, `EXPRESSION_BADGE`, `isExpressionDefault`, `isFunctional`, `VIRTUAL_FROM`, `ROW_HEIGHT`, `CLICKHOUSE_REBUILD_ENGINES`. A `shape.test.ts` covers the three functions | 90 |
| `useTableShape.ts` | **The shape on screen and where it came from**: `RememberedStructure`, `StructureCache`, `STRUCTURE_CACHE_LIMIT`, the cache token, `loading`, the per-connection collations read, `tableKey`, `structure`, the read effect, `columns`, `indexes`, `skipIndexes`, `engine`, `reload` | 160 |
| `useStructureEdits.ts` | **What the panel sends**: `saving`, the column, index and skip-index dialog states, the ORDER BY dialog and its row-count read, `pendingDrop`, `apply`, `submitColumn`, `submitIndex`, `submitSkipIndex`, `submitOrderBy`, `confirmDrop` | 170 |
| `ColumnsPanel.tsx` | The columns `<section>`: the filter and its reset on another table, the match count, the reload and add buttons, the virtual window, the sizer row and the rows | 330 |
| `IndexesPanel.tsx` | The indexes `<section>`: its virtual window, sizer row and rows, and the sorting-key rule | 210 |
| `SkipIndexesPanel.tsx` | ClickHouse's skip indexes `<section>` | 90 |
| `StructureDialogs.tsx` | The column, index, skip-index, ORDER BY and drop dialogs, each shown on the state that opens it | 100 |
| `TableStructure.tsx` | Calls the two hooks, owns the reload shortcut and the read-only gate (`noWrites`, `noWritesHint`), and draws the panels, the overlay and the dialogs | 150 |

## D2 — the Dashboard's files

Everything stays in `screens/Dashboard/`, beside the parts it already has there (`QuickStart`,
`DiskUsagePanel`, `CleanupDialog`, …). `index.ts` and `MixEngineTab.tsx` do not change.

| File | What it holds | Lines (est.) |
| --- | --- | --- |
| `serviceView.ts` | Pure: `PENDING_LABEL`, `ServiceFilter`, `isServing`, `pillTone` (moved out of the component, now taking the state it reads), `stateLabelKey`, `stateHintKey`. A `serviceView.test.ts` covers `isServing`, `pillTone` and the failed-with-a-reason hint | 70 |
| `useServices.ts` | **The services table and the stream that keeps it current**: `status`, `rows`, `loaded`, the read order (`order`), `reload`, `busy`, `run`, `act`, `stopAll`, `setAutostart`, `jobs`, `cancelling`, `cancelJob`, the elevation queue (`waiting`, `pending`, `canPrompt`, `reason`, `announced`, `showWaiting`), the reload when `active` turns on, and the `subscribeDaemonWatch` effect | 380 |
| `useDiskUsage.ts` | `disk`, `refreshingDisk`, `readDisk`, `refreshDisk` | 50 |
| `useSites.ts` | `sites`, `readSites`, the read when `active` turns on | 35 |
| `useMetrics.ts` | `/metrics` held while `active && focused`, through its serial queue; returns `frame` | 60 |
| `useDatabaseClients.ts` | `database.client` asked once per id (`asked`, `databases`), `credentials`, `showCredentials`, `resetTarget`, `resetCredential` | 90 |
| `DashboardHeader.tsx` | The `PageHeader`: version and summary pills, home path and its copy button (with `homeCopied`), the daemon's usage, the header buttons | 130 |
| `ServicesCard.tsx` | The services `Card`: the filter (its state with it), the jobs list, the empty and loading states, the table and its rows, the footer | 260 |
| `ServiceRowMenu.tsx` | The row menu: logs, copy port, autostart, and the database entries | 120 |
| `Dashboard.tsx` | Calls the hooks, owns the one `error` banner, the row menu's position (`menu`, opened by `ServicesCard` and drawn by `ServiceRowMenu`) and the open dialogs (`creating`, `cleaning`), and draws the parts | 200 |

Every hook that reports an error takes `onError`, which is `Dashboard`'s `setError`. `reload` keeps
calling `readDisk` without awaiting it, so `useServices` takes `readDisk` from `useDiskUsage`.

## D3 — rules for both

- Every comment moves with the code it describes, word for word. A comment that points at a
  position and now crosses files is rewritten to name what it meant, in a commit of its own.
- The parts take plain props and call no hook except `useTranslation` and the hooks the moved code
  already called (`useVirtualRows` in the two grids). Each renders exactly the elements its
  component renders today, with the same classes, attributes and handlers, in the same order, and
  imports the same CSS Module. No CSS moves.
- State moves into a part only when that part is mounted for as long as the component is, so it
  lives exactly as long as before. That holds for `columnFilter` (`ColumnsPanel` is always drawn),
  `filter` (`ServicesCard`) and `homeCopied` (`DashboardHeader`, drawn whenever `status` is set,
  the only time its button exists). T208a showed what happens otherwise: an effect moved into a
  part that mounts later ran twice under StrictMode.
- A function that becomes a hook's return value and is listed in an effect is a `useCallback`
  with the dependencies it already had, so no effect runs at a moment it did not run before.

## D4 — what must not change

- **Effect order where it is load-bearing.**
  - TableStructure: the collations read, then the shape read, then the row-count read. All three
    keep their order inside their hooks, and `useTableShape` is called first. Clearing the filter
    on another table now runs as a child's effect, before the parent's. It writes nothing the
    others read.
  - Dashboard: the reload on `active` comes before the stream subscription, both in `useServices`.
    `readSites`, `/metrics` and the `database.client` questions read and write state of their own,
    and their order relative to each other is kept.
- **Every handler's behaviour.** The same state changes happen, in the same order, and an `async`
  function awaits what it awaited.
- **Rendered output.** The same DOM for the same state.
- **User-visible strings.** The same `t("…")` keys. Every string literal of the two old files is
  somewhere in the new ones.
- **Tests and lint.** `npm run build`, `npm test` (the count rises only by the two new pure test
  files) and `npm run lint`, with the hook dependency rule.

**The run in a browser.** T208a's harness, pointed at the two screens: a script drives the demo
(`npm run dev`, `demo/demo.html`), recording the DOM after each step, once on `master` before the
first commit and again after every commit that moves code. The recordings must be identical, and
the console must show no errors.

- Structure, on the demo's PostgreSQL `orders` table: open the tab, filter the columns, then press
  Escape; open and cancel the add-column, edit-column and add-index dialogs; open and cancel a drop
  confirmation; reload; switch to another table and back (the cache answers). A read-only
  connection, where every write button is grey with its hint.
- Dashboard, on the demo's services: filter running and stopped; open the row menu and each of
  its entries that opens something (credentials, reset, then cancel); toggle autostart; start and
  stop a service; Stop all; open and cancel New service; refresh the disk panel; open and cancel
  cleanup.

**As run.** The demo answers neither `postgres_table_structure` nor four of the Dashboard's
commands, so the scripts answer them in the page in the bindings' shapes. `customers` answers 65
columns, so the windowed grid is drawn. Both recordings were identical on `master` twice and after
every commit. The Structure run also turned up duplicate React keys in the sizer row: an old bug in
`widestValues`, fixed in its own commit, and the baseline was taken again after it.

The roadmap asks for the Dashboard to be checked with MixEngine running and stopped. The demo
covers it running. Stopped, `MixEngineTab` shows its gate and not this screen, so what is checked
there is the way back: the daemon is stopped by a stub of `daemon.status` in the page, the gate
shows, the stub is lifted, and Dashboard comes back with its table. The real window (`npm run
dev:app`) is checked by hand the same way, against a real `mixengined`.

`npm run screenshots -- --check` renders 12/12 scenes. The `hero` scene is the Dashboard.

## D5 — commits

On a branch of its own, `t208b-structure-dashboard-hooks`, since this works from a spec. Each
commit passes `npm run build`, `npm test` and `npm run lint`.

1. `shape.ts` and its test.
2. `useTableShape`.
3. `useStructureEdits`.
4. The four TableStructure parts.
5. `serviceView.ts` and its test.
6. `useDiskUsage`, `useSites`, `useMetrics`.
7. `useDatabaseClients`.
8. `useServices`.
9. The three Dashboard parts.
10. Comment pointers.
11. Roadmap and this spec's status.

## MixLab

The first half is MixLab's own `db` module. It works with `mixengined` absent and touches nothing
of MixEngine. The second half is the `mixengine` module's Dashboard, which only exists while the
daemon answers. Its calls stay the ones `api.ts` makes today, so no daemon method is added or
lost and `check-client-surface` has nothing new to say. A user sees the same two screens. Nothing
goes in `CHANGELOG.md`. `mix` has no part in this.

## Not in scope

- Moving the folders (T208c).
- Tests for the hooks. A hook test needs a renderer this project's vitest setup does not have.
- `ServicesDetail.tsx` and the other MixEngine screens. They are under the limit.

## Is there an ADR here? No

## Documentation, when it lands

The phase file ticks T208b and links this spec, and this spec flips to `implemented`.
