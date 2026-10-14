---
status: draft
date: 2026-10-14
task: T207k
---

# The `db` module's data grid: hooks for what it does, parts for what it draws (design)

`apps/desktop/src/modules/db/components/SqlTable/SqlTable.tsx` is 1,843 lines and holds one
component, `SqlTable`. That component is the Data tab of every SQL engine. Its body is about 1,140
lines of state, effects and handlers, and they share about 60 locals. Then come 480 lines of JSX:
the filter bar, the table (header, rows, the cell being edited), the footer, two context menus and
three dialogs. A component's callbacks close over its state, so this cannot be split by moving
lines. As the roadmap planned, this task introduces **custom hooks for what the grid does and
components for what it draws**. The folder already has two pure halves, `request.ts` and
`rowText.ts`, and the new files sit beside them.

Roadmap: [phase 45](../roadmap/phase-45-files-that-fit-in-one-read.md), task T207k. Like T207l,
this changes code, and D4 says what it must not change.

## D1 — the files

Two precedents shape the folder. `shell/components/SettingsModal/` keeps the parts of one component
as sibling `.tsx` files that share its stylesheet. `components/TabStrip/` keeps its hooks as
`useX.ts` beside the component. Neither the folder's public entry (`index.ts`) nor
`SqlWorkspace.tsx`'s import changes.

| File | What it holds | About |
| --- | --- | --- |
| `cells.ts` | `nextSort`, `normalizeCellValue`, `displayValue`, `isMultilineType` (pure; moved as they are) | 25 |
| `filters.ts` | `RememberedFilters`, `FilterCache`, `rememberedFilters` (moved; `index.ts` re-exports the two types from here) | 40 |
| `useTableData.ts` | **What is on screen and where it came from**: page, size, sort, the bar's rows and applied conditions, rows, columns, column metadata, primary key, AUTO_INCREMENT column, total, loading, the reload token, `scrollPosRef` and `requestRef`; the render-time swap to another table and to another schema token; filing the grid and the bar away (on a swap and on unmount); restoring a remembered table; the fetch | 470 |
| `useCellEditing.ts` | **Editing a cell**: the open cell and its text, the staged row (`pendingRowRef`), `commitEditingCell`, `flushPendingRow`, `commitAndExit`, `cancelEdit`, `moveEditTo`, the input's change, key and blur handlers, the focus and autosize effects, and clearing all of it when the table or the schema token changes | 260 |
| `useRowSelection.ts` | **Selecting rows**: the set, the shift-click anchor, `clearSelection`, `selectRow`, select-all, `selectedRowsInOrder` | 70 |
| `useGridLayout.ts` | **Laying the grid out**: the size constants (`VIRTUAL_FROM`, `ROW_HEIGHT`, `EDIT_COLUMN`, …), header refs, measured widths, pinned widths (`gridColumns`), `headerExtras`, the two measuring layout effects and the resize listener, putting the scroll position back, the row and column windows, `handleScroll`, `editingColumnExtra`, `drawnColumns` | 260 |
| `GridHeader.tsx` | the `<thead>`: sort chevron, FK chip, the two tooltips, sort click and header right-click | 90 |
| `GridBody.tsx` | the `<tbody>`: spacers, row windows, the cell being edited (input or textarea), the read cells | 140 |
| `RowMenu.tsx` | the cell right-click menu: expand, copy value, follow the key, the copies, reload | 110 |
| `DeleteRowsDialog.tsx` | the delete confirmation and its two options | 50 |
| `SqlTable.tsx` | the component: calls the four hooks, owns the menus, dialogs and the delete and insert state; the actions that combine several hooks (`reload`, `applyFilters`, `toggleSort`, insert, delete, `openReferenced`, copy, the cell mouse and context handlers, the grid's keys, the three shortcuts); and the JSX that puts the parts together: filter bar, scroll box, table, footer, header menu, insert dialog, cell dialog | 550 |

Every comment moves with the code it describes, word for word. A comment that points at a
position ("the swap above", "the layout effect below") and now crosses files is rewritten to name
what it meant, in a commit of its own, as T207l did.

## D2 — what each hook takes and gives

Hooks are called in the order the component needs them: `useTableData`, then `useCellEditing`
(needs the rows), then `useRowSelection`, then `useGridLayout` (needs the open cell). Each takes
one argument object, and returns one object the component destructures.

- `useTableData({ active, connectionId, selectedDb, selectedTable, schemaToken, filterCache,
  tableCache, onError })` returns the state above with its setters. The setters callers use are
  `setPage`, `setPageSize`, `setSort`, `setRows`, `setFilterRows`, `setAppliedFilters` and
  `bumpReload` (today's `setReloadToken((n) => n + 1)`, under a name). It also returns
  `scrollPosRef`, `requestRef`, `filterOperators` and `filterBarRef`.
- `useCellEditing({ rows, setRows, columns, primaryKey, api, connectionId, selectedDb,
  selectedTable, schemaToken, tableCache, readOnly, onError })` returns the open cell, its text, the
  input ref, `pendingRowRef`, the five operations, and the three input handlers.
- `useRowSelection({ rows })` returns `selectedRows`, `selectRow`, `clearSelection`,
  `selectAllRows`, `selectedRowsInOrder`, `selectOnly` (a right-click outside the selection), and
  the anchor.
- `useGridLayout({ scrollRef, rows, columns, columnMeta, editingCell, active, layoutWidth,
  selectedDb, selectedTable, scrollPosRef })` returns what the table element, the header and the
  body read.

`scrollRef` is the component's: the scroll box is its element, and two hooks read it.

## D3 — the components

`GridHeader`, `GridBody`, `RowMenu` and `DeleteRowsDialog` take plain props. They hold no state of
their own and call no hook except `useTranslation`, so each renders exactly the elements
`SqlTable` renders today, with the same classes, attributes and handlers. They import
`SqlTable.module.css`, so no CSS moves. That also means the class names `headerExtras` looks up
(`styles.sortIcon`, `styles.fkBadge`) are the ones the header renders.

The `<input>` and `<textarea>` of the cell being edited move into `GridBody` unchanged. They are
raw elements, and they predate the shared `Input`/`Textarea`. Replacing them is not this task (see
Not in scope).

## D4 — what the change must not alter

- **Effect order where it is load-bearing**, kept by the hook call order and by order inside each
  hook:
  - The scroll position is put back (a layout effect) before `useVirtualRows`. Both now live in
    `useGridLayout`, in that order.
  - The remembered table is restored before the fetch effect reads `requestRef`. Both live in
    `useTableData`, in that order.
  - The two render-time swaps run before any state they reset is read. They stay at the top of
    `useTableData`.
- **One order does change.** Closing the cell editor when the table changes was the first two lines
  of the restore effect. It becomes an effect of its own in `useCellEditing`, with the same
  dependencies, so it now runs after the fetch effect rather than before it. Nothing the fetch reads
  is touched by it: the fetch reads the request fields, `requestRef` and `scrollPosRef`, never the
  editor. Clearing the selection was already its own effect, and it runs after the fetch as before.
- **Every handler's behaviour**: the same state changes in the same order. An `async` function
  awaits what it awaited.
- **Rendered output**: the same DOM for the same state. Checked by `npm run screenshots -- --check`
  and by the run below.
- **User-visible strings**: the same `t("…")` keys. The set of string literals before equals the set
  after, checked as T207l checked it.
- **Tests and lint**: `npm run build` (strict TS, no unused locals), `npm test`, `npm run lint`
  (hook dependency rule included: every `useEffect`/`useCallback`/`useMemo` keeps its dependency
  list, or lint fails).

**The run in a browser.** `npm run dev` with the demo harness (`demo/demo.html`, the sample
PostgreSQL connection) shows the Data tab of `acme_shop.orders`, 40 rows. The same steps run there
before the change and after it, and their outcomes are compared:

- sort a column through all three states;
- apply a filter;
- select rows with click, modifier-click and shift-click, then `Ctrl+A`, then Escape;
- open the cell menu and copy a row as TSV;
- double-click a cell, type, Tab, Escape;
- double-click, type, Enter. The demo has no `update_row`, so the write fails and the row has to
  come back with the error shown;
- open the insert form and cancel it;
- open the delete confirmation and cancel it;
- change the page size.

A 40-row page is below `VIRTUAL_FROM`, so the windowed path is not drawn there. That path is
`useVirtualRows`/`useVirtualColumns` in `core/virtualRows.ts`, which this task does not touch,
called with the same arguments. `gridRows.test.ts` still guards the row height.

## D5 — commits

1. `cells.ts` and `filters.ts` (moves). 2. `useRowSelection`. 3. `useGridLayout`.
4. `useCellEditing`. 5. `useTableData`. 6. `GridHeader` and `GridBody`. 7. `RowMenu` and
`DeleteRowsDialog`. 8. Comment pointers. 9. Roadmap, and the file out of the baseline.
Each commit gets `npm run build`, `npm test` and `npm run lint`; the browser run happens after commit
8; and `npm run screenshots -- --check` and the root gate run before the push.

## MixLab

This is MixLab's own `db` module and touches nothing of MixEngine. A user sees the same Data tab:
same rows, keys, menus and dialogs. Nothing goes in `CHANGELOG.md`, because there is no change a
user would notice.

## Not in scope

- Replacing the raw `<input>`/`<textarea>` of the cell editor with the shared primitives. That is a
  change to behaviour: `Textarea` grows to fit, and this one is sized by its own effect.
- Tests for the hooks. The pure halves already have theirs, and a hook test needs a renderer this
  project's vitest setup does not have.
- `NoSqlTable`, the Mongo grid, which is under the limit.

## Is there an ADR here? No

## Documentation, when it lands

The phase file ticks T207k, and this spec flips to `implemented`.
