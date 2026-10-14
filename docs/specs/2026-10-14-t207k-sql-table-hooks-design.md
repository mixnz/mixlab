---
status: implemented
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

| File | What it holds | Lines |
| --- | --- | --- |
| `cells.ts` | `nextSort`, `normalizeCellValue`, `displayValue`, `isMultilineType` (pure; moved as they are) | 22 |
| `filters.ts` | `RememberedFilters`, `FilterCache`, `rememberedFilters` (moved; `index.ts` re-exports the two types from here) | 37 |
| `useTableData.ts` | **What is on screen and where it came from**: page, size, sort, the bar's rows and applied conditions, rows, columns, column metadata, primary key, AUTO_INCREMENT column, total, loading, the reload token, `scrollPosRef` and `requestRef`; the render-time swap to another table and to another schema token; filing the grid and the bar away (on a swap and on unmount); restoring a remembered table; the fetch | 408 |
| `useCellEditing.ts` | **Editing a cell**: the open cell and its text, the staged row (`PendingRow`), `commitEditingCell`, `flushPendingRow`, `commitAndExit`, `cancelEdit`, `moveEditTo`, the input's change, key and blur handlers, the focus and autosize effects, and closing the editor when the table or the schema token changes | 257 |
| `useRowSelection.ts` | **Selecting rows**: the set, the shift-click anchor, `clearSelection`, `selectRow`, `selectAllRows`, `selectOnly`, `selectedRowsInOrder` | 69 |
| `useGridLayout.ts` | **Laying the grid out**: the size constants (`VIRTUAL_FROM`, `ROW_HEIGHT`, `EDIT_COLUMN`, …), header refs, measured widths, pinned widths (`gridColumns`), `headerExtras`, the two measuring layout effects and the resize listener, putting the scroll position back, the row and column windows, `handleScroll`, `editingColumnExtra`, `drawnColumns` | 286 |
| `GridHeader.tsx` | the `<thead>`: sort chevron, FK chip, the two tooltips, sort click and header right-click | 100 |
| `GridBody.tsx` | the `<tbody>`: spacers, row windows, the cell being edited (input or textarea), the read cells | 181 |
| `RowMenu.tsx` | the cell right-click menu (and its `CellMenu` type): expand, copy value, follow the key, the copies, reload | 147 |
| `DeleteRowsDialog.tsx` | the delete confirmation and its two options | 79 |
| `SqlTable.tsx` | the component: calls the four hooks, owns the menus, dialogs and the delete and insert state; the actions that combine several hooks (`reload`, `applyFilters`, `toggleSort`, insert, delete, `openReferenced`, copy, the cell mouse and context handlers, the grid's keys, the three shortcuts); and the JSX that puts the parts together: filter bar, scroll box, table, footer, header menu, insert dialog, cell dialog | 790 |

Every comment moves with the code it describes, word for word. A comment that points at a
position ("the swap above", "the layout effect below") and now crosses files is rewritten to name
what it meant, in a commit of its own, as T207l did.

## D2 — what each hook takes and gives

Each hook is called where the state it holds was declared, so the hooks run in this order:
`useTableData`, `useCellEditing` (which needs the rows), `useGridLayout` (which needs the open
cell), `useRowSelection`. Each takes one argument object and returns one object, which the
component destructures. A hook that reports an error reads `t` from `useTranslation` itself.

- `useTableData({ api, active, connectionId, selectedDb, selectedTable, schemaToken, filterCache,
  tableCache, onError })` returns the state above with the setters callers use: `setPage`,
  `setPageSize`, `setSort`, `setRows`, `setFilterRows`, `setAppliedFilters` and `setReloadToken`.
  It also returns `scrollPosRef`. `filterOperators` and `filterBarRef` stay in the component,
  because only its JSX reads them.
- `useCellEditing({ api, connectionId, selectedDb, selectedTable, schemaToken, rows, setRows,
  columns, primaryKey, tableCache, readOnly, onError })` returns the open cell, its text, the input
  ref, `editingCellRef`, `pendingRowRef`, `commitAndExit`, `cancelEdit`, `moveEditTo`, and the
  three input handlers.
- `useGridLayout({ scrollRef, scrollPosRef, rows, columns, columnMeta, editingCell, active,
  layoutWidth, selectedDb, selectedTable })` returns what the table element, the header and the body
  read.
- `useRowSelection(rows)` returns `selectedRows`, `clearSelection`, `selectRow`, `selectAllRows`,
  `selectOnly` (a right-click outside the selection) and `selectedRowsInOrder`.

`scrollRef` belongs to the component: the scroll box is its element, and two hooks read it.

Some functions had to become stable. Two were plain functions inside the component, which lint
does not require an effect to list. Now they come from a hook, and the effect that calls them
lists them. So `clearSelection` and `setEditingCell` are `useCallback`s with no dependencies.
`scrollRef` and `scrollPosRef` are listed in the scroll-restoring layout effect, now that they are
parameters. All four are stable, so none of these effects runs at a moment it did not run before.

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
- **What does move**: passive effects that do not read one another's state. The editor's
  three effects and its close-on-another-table effect run where its state was declared, so
  closing the editor still runs before the restore effect and the fetch, as it did when it was
  their first two lines. The data hook's restore and fetch effects now run before the layout hook's
  resize listener and before the selection-clearing effect, which stays in the component. Neither
  of those reads anything the fetch or the restore writes, and the restore still runs before the
  fetch.
- **Every handler's behaviour**: the same state changes in the same order. An `async` function
  awaits what it awaited.
- **Rendered output**: the same DOM for the same state. Checked by `npm run screenshots -- --check`
  and by the run below.
- **User-visible strings**: the same `t("…")` keys. The set of string literals before equals the set
  after, checked as T207l checked it.
- **Tests and lint**: `npm run build` (strict TS, no unused locals), `npm test`, `npm run lint`
  (hook dependency rule included: every `useEffect`/`useCallback`/`useMemo` keeps its dependency
  list, or lint fails).

**The run in a browser.** This was run, not just planned. The demo harness (`npm run dev`,
`demo/demo.html`, the sample PostgreSQL connection) shows the Data tab of `acme_shop.orders`,
40 rows. A script drove 26 steps and recorded the grid after each one:

- sort a column through all three states;
- select rows with click, Ctrl-click and Shift-click, then `Ctrl+A`, then Escape;
- open the cell menu and copy a row as TSV. The webview refuses the clipboard, and the error
  banner says so;
- double-click a cell, type, Tab (the cell is staged and marked dirty), Escape (the row comes
  back);
- double-click, type, Enter. The demo has no `postgres_update_row`, so the write fails, the error is
  shown, and the row comes back;
- open the insert form and cancel it; open the delete confirmation and cancel it;
- go to the next page; type a filter value and apply it.

After each step it recorded: header sort states, row count, three rows' text, the selection, dirty
cells, the editor's value and focus, menu entries, dialogs, alerts, the pager, the footer buttons'
enabled state, the bar's values, and the table's DOM length. The recording on `master` (served
from a separate worktree) and the recording on this branch are **identical in all 26 steps**, and
the console showed no errors. `npm run screenshots -- --check` renders 12/12 scenes.

A 40-row page is below `VIRTUAL_FROM`, so the windowed path is not drawn there. That path is
`useVirtualRows`/`useVirtualColumns` in `core/virtualRows.ts`, which this task does not touch,
called with the same arguments. `gridRows.test.ts` still guards the row height.

## D5 — commits

1. `cells.ts` and `filters.ts` (moves). 2. `useRowSelection`. 3. `useGridLayout`.
4. `useCellEditing`. 5. `useTableData`. 6. `GridHeader` and `GridBody`. 7. `RowMenu` and
`DeleteRowsDialog`. 8. Comment pointers. 9. Roadmap. `SqlTable.tsx` left the baseline at commit 4,
when it reached 1,363 lines. Each commit passed `npm run build`, `npm test` (2,880) and
`npm run lint`. The 116 string literals of the old file are the same 116 across the new files.

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
