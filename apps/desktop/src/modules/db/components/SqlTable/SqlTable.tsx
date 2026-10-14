import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import ActionBar from "../../../../components/ActionBar";
import CellDialog from "../../../../components/CellDialog";
import ConfirmDialog from "../../../../components/ConfirmDialog";
import ContextMenu from "../../../../components/ContextMenu";
import FilterBar, { type FilterBarHandle } from "../FilterBar";
import InsertRowsDialog from "../InsertRowsDialog";
import LoadingOverlay from "../../../../components/LoadingOverlay";
import Pagination from "../../../../components/Pagination";
import Tooltip from "../../../../components/Tooltip";
import Checkbox from "../../../../components/Checkbox";
import { ChevronDownIcon, ChevronUpIcon, CopyIcon, PlusIcon, ReloadIcon, TrashIcon } from "../../../../icons";
import { useTranslation } from "../../../../i18n";
import { errorMessage } from "../../../../core/errors";
import { copyText } from "../../../../core/clipboard";
import { useSqlApi, useSqlDialect } from "../../sql/context";
import { IS_MAC, hasPrimaryModifier } from "../../../../core/platform";
import { useReloadShortcut, withReloadShortcut } from "../../../../core/reload";
import { useShortcut } from "../../../../core/shortcuts";
import { filterRowFor, toQueryFilters } from "../../filters";
import { FILTER_OPERATORS, operatorArity, type FilterOperator } from "../../sql/filters";
import { cellPreview, gridStyle } from "../../../../core/virtualRows";
import type { TableCache, TableRequest } from "./request";
import { csvText, insertStatements, jsonText, spreadsheetText } from "./rowText";
import { displayValue, isMultilineType, nextSort, normalizeCellValue } from "./cells";
import type { FilterCache } from "./filters";
import useCellEditing from "./useCellEditing";
import useGridLayout, { EDIT_COLUMN, ROW_HEIGHT } from "./useGridLayout";
import useRowSelection from "./useRowSelection";
import useTableData from "./useTableData";
import styles from "./SqlTable.module.css";

/** Which cell the right-click menu was opened on, and where the pointer was when it was. Null
 *  while the menu is closed. */
interface CellMenu {
  rowIndex: number;
  col: string;
  x: number;
  y: number;
}

interface Props {
  /** Whether this is what the user is actually looking at — the Data tab, in the connection tab the
   *  tab bar is showing. This stays mounted behind both, so it is what says when a page of rows is
   *  worth reading, when a read the user cannot see would be wasted, and which of the panes mounted
   *  at once `Ctrl+R` belongs to. */
  active: boolean;
  connectionId: string;
  selectedDb: string;
  selectedTable: string;
  onError: (message: string) => void;
  layoutWidth?: number;
  /** Where the filter bar is kept between visits — see {@link FilterCache}. */
  filterCache: FilterCache;
  /** Where the rows themselves are kept between visits — see {@link TableCache}. */
  tableCache: TableCache;
  /** Which shape of this database the cache is allowed to speak for. The workspace moves it
   *  whenever the app changes that shape, and everything remembered under the shape before is then
   *  read again rather than shown — see {@link TableRequest.schemaToken}. */
  schemaToken: number;
  /** Told when rows have been inserted or deleted here. The grid catches up with itself; what the
   *  table weighs is the Statistics tab's business, and the workspace is what holds those figures. */
  onRowsChanged?: () => void;
  /** Told to follow a foreign key out of this grid: open `table` on the Data tab with its filter
   *  bar already asking for `column = value`. Only the workspace can do it — it owns which table is
   *  selected — and a key pointing back at this same table never gets here; see
   *  {@link SqlTable.openReferenced}. */
  onOpenRelated?: (table: string, column: string, value: string) => void;
  /** The saved connection is marked as one nothing is written to. The grid still reads, sorts,
   *  filters and pages exactly as it does otherwise — what goes is every door out of read mode. */
  readOnly?: boolean;
}

const PAGE_SIZES = [100, 200, 500, 1000, 5000];

function SqlTable({
  active,
  connectionId,
  selectedDb,
  selectedTable,
  onError,
  layoutWidth,
  filterCache,
  tableCache,
  schemaToken,
  onRowsChanged,
  onOpenRelated,
  readOnly = false,
}: Props) {
  const { t } = useTranslation();
  const api = useSqlApi();
  const dialect = useSqlDialect();
  /* SQLite parses `REGEXP` but ships no implementation of it, so the two rows would always come
     back as "no such function". Dropped from the dropdown rather than left to fail — see
     `regexpFilter` on the dialect. */
  const filterOperators = useMemo(
    () =>
      dialect.regexpFilter
        ? FILTER_OPERATORS
        : FILTER_OPERATORS.filter((op) => op.id !== "regexp" && op.id !== "notRegexp"),
    [dialect.regexpFilter]
  );
  const {
    page,
    setPage,
    pageSize,
    setPageSize,
    rows,
    setRows,
    columns,
    columnMeta,
    primaryKey,
    autoIncrementColumn,
    total,
    sort,
    setSort,
    filterRows,
    setFilterRows,
    appliedFilters,
    setAppliedFilters,
    loading,
    reloadToken,
    setReloadToken,
    scrollPosRef,
  } = useTableData({
    api,
    active,
    connectionId,
    selectedDb,
    selectedTable,
    schemaToken,
    filterCache,
    tableCache,
    onError,
  });

  const scrollRef = useRef<HTMLDivElement>(null);
  /** The filter bar above, so `Ctrl+F` can put the caret in it — see {@link FilterBarHandle}. */
  const filterBarRef = useRef<FilterBarHandle>(null);

  const {
    editingCell,
    editValue,
    editingCellRef,
    pendingRowRef,
    editInputRef,
    commitAndExit,
    cancelEdit,
    moveEditTo,
    handleInputBlur,
    handleEditChange,
    handleEditKeyDown,
  } = useCellEditing({
    api,
    connectionId,
    selectedDb,
    selectedTable,
    schemaToken,
    rows,
    setRows,
    columns,
    primaryKey,
    tableCache,
    readOnly,
    onError,
  });
  const {
    tableRef,
    thRefs,
    columnWidths,
    gridColumns,
    virtual,
    view,
    columnsVirtual,
    colView,
    handleScroll,
    editingColumnExtra,
    drawnColumns,
  } = useGridLayout({
    scrollRef,
    scrollPosRef,
    rows,
    columns,
    columnMeta,
    editingCell,
    active,
    layoutWidth,
    selectedDb,
    selectedTable,
  });

  const { selectedRows, clearSelection, selectRow, selectAllRows, selectOnly, selectedRowsInOrder } =
    useRowSelection(rows);
  /** The right-click menu, and where it was opened — see {@link CellMenu}. */
  const [menu, setMenu] = useState<CellMenu | null>(null);
  const closeMenu = useCallback(() => setMenu(null), []);
  /** The header's own right-click menu: which column it was opened on, and where the pointer was.
   *  Separate from {@link CellMenu} rather than a variant of it — nothing in the cell menu means
   *  anything without a row, and every entry there would have to learn it might not have one. */
  const [headerMenu, setHeaderMenu] = useState<{ col: string; x: number; y: number } | null>(null);
  const closeHeaderMenu = useCallback(() => setHeaderMenu(null), []);
  /** The cell being read in a dialog of its own, by row on the page and column. Held rather than
   *  its value: an edit committed underneath it should be what the dialog shows. */
  const [expandedCell, setExpandedCell] = useState<{ rowIndex: number; col: string } | null>(null);
  const [confirmingDelete, setConfirmingDelete] = useState(false);
  const [deleteWholeTable, setDeleteWholeTable] = useState(false);
  const [resetAutoIncrement, setResetAutoIncrement] = useState(false);
  const [deleting, setDeleting] = useState(false);
  // What the insert form opens on — a blank row, or a copy of each selected row. `null` is the
  // form being closed. The rows a clone starts from are read at render time rather than stored
  // here, so they are the ones on screen when the form mounts, staged edits included.
  const [insertMode, setInsertMode] = useState<"new" | "clone" | null>(null);

  /** Hands the keyboard back to the grid. A dialog opened from here takes focus and leaves it on
   * the body when it closes, which would silence the grid's own shortcuts until the next click. */
  function focusGrid() {
    scrollRef.current?.focus({ preventScroll: true });
  }

  // The indices in `selectedRows` only mean anything for the rows currently on
  // screen, so any refetch — a new page, a new size, a new order, a reload, a
  // new table — drops the selection rather than carrying it onto different rows.
  //
  // The right-click menu goes with it: it is opened on one row of one page, and every entry in it
  // acts on the selection the same indices name. So does an open cell dialog, which names a row the
  // same way — left up, it would go on showing a heading from the old page over a value from the
  // new one.
  useEffect(() => {
    clearSelection();
    setMenu(null);
    setHeaderMenu(null);
    setExpandedCell(null);
  }, [selectedDb, selectedTable, page, pageSize, sort, appliedFilters, reloadToken, clearSelection]);

  /** Refetches the table from its first page. Waits for a staged edit to be written first, so the
   * rows that come back include it rather than overwriting it with the pre-edit values.
   *
   * Back to page one on purpose: a reload is asked for when the table is expected to have moved
   * underneath the grid, and the rows the user wants to see after that are the newest ones — a page
   * counted off a table that has since grown or shrunk names different rows anyway. The token is
   * bumped as well as the page reset, so a reload pressed on page one is still a refetch rather than
   * a no-op. */
  async function reload() {
    await commitAndExit();
    // And back to the head of it: what the first page is worth is the rows at its top, and an offset
    // carried over from halfway down page five would land the grid in the middle of page one instead.
    // The horizontal position stays — that says which columns are being read, which a reload does not
    // change. Read by the layout effect above when the new rows arrive.
    scrollPosRef.current = { ...scrollPosRef.current, top: 0 };
    setPage(0);
    setReloadToken((n) => n + 1);
  }

  // Gated on the same state the button below is: a refetch asked for while one is already out, or
  // over rows being deleted, is one the button would refuse. Not while the delete confirmation is
  // up either — it names the selected rows, and a reload underneath it would leave it naming rows
  // that are no longer the ones selected.
  useReloadShortcut(active && !confirmingDelete, () => {
    if (loading || deleting) return;
    void reload();
  });

  /** Runs the filter bar's rows against the table. Like a reload, a staged edit is written first:
   * the rows come back filtered, and the pending row index would no longer point at the row that
   * was edited. The result is a different set of rows, so the grid goes back to the first page —
   * the page the user was on need not even exist under the new conditions. */
  async function applyFilters() {
    await commitAndExit();
    // A new array every time on purpose: pressing Apply twice on the same conditions is a
    // request to refetch, and an equal-but-identical array would be a no-op.
    setAppliedFilters(toQueryFilters(filterRows, operatorArity));
    setPage(0);
  }

  /** Moves the clicked column to its next sort state. Like a reload, a staged edit is written
   * first — the rows come back in a new order, and the pending row index would no longer point
   * at the row that was edited. Reordering also reshuffles which rows land on the current page,
   * so the grid goes back to the first one. */
  async function toggleSort(column: string) {
    await commitAndExit();
    setSort((current) => nextSort(current, column));
    setPage(0);
  }

  /** Identifies one row to the server: its primary key columns, or — when the table has none —
   * every column, the same fallback an update uses. */
  function rowKey(row: Record<string, unknown>): Record<string, string | null> {
    const keyCols = primaryKey.length > 0 ? primaryKey : columns;
    const key: Record<string, string | null> = {};
    for (const c of keyCols) key[c] = normalizeCellValue(row[c]);
    return key;
  }

  /** Opens the insert form, either on a single blank row or on a copy of each selected row —
   * a clone gets looked over, and its unique columns changed, before it is written. */
  async function openInsert(mode: "new" | "clone") {
    // Write out a staged edit first: the form is modal, and the blur that would otherwise flush
    // it never comes while it is up.
    await commitAndExit();
    setInsertMode(mode);
  }

  /** Hands the form's rows to the server. Errors are left to reject so the form can show them
   * and stay open with the typed values still in it. */
  async function submitInsert(newRows: Record<string, string | null>[]) {
    await api.insertRows(connectionId, selectedDb, selectedTable, newRows);
    setInsertMode(null);
    focusGrid();
    setReloadToken((n) => n + 1);
    // The table holds more rows and takes more disk than the Statistics tab was told. That is the
    // workspace's to answer: a change this app made itself is the one thing not waited on.
    onRowsChanged?.();
  }

  async function openDeleteConfirm() {
    // Write out a staged edit first: it may touch a key column, which would leave the key we
    // are about to send pointing at a row that no longer looks like that.
    await commitAndExit();
    setDeleteWholeTable(false);
    setResetAutoIncrement(false);
    setConfirmingDelete(true);
  }

  async function confirmDelete() {
    const keys = selectedRowsInOrder().map(rowKey);
    const wholeTable = deleteWholeTable;
    setConfirmingDelete(false);
    focusGrid();
    setDeleting(true);
    try {
      await api.deleteRows(
        connectionId,
        selectedDb,
        selectedTable,
        keys,
        wholeTable,
        canResetAutoIncrement && resetAutoIncrement,
      );
      if (wholeTable) setPage(0);
      setReloadToken((n) => n + 1);
      // Same as an insert: what the table weighs has changed, and the figures are not this grid's.
      onRowsChanged?.();
    } catch (e) {
      onError(errorMessage(t, e));
    } finally {
      setDeleting(false);
    }
  }

  function handleCellMouseDown(e: React.MouseEvent<HTMLTableCellElement>, rowIndex: number, col: string) {
    // A secondary click belongs to `handleCellContextMenu`, which decides the selection for itself.
    // On a Mac that gesture is also `Ctrl+Click`, and WebKit spells it as a plain button-0
    // mousedown holding `Ctrl` — so `e.button` alone lets it through to the selection code below,
    // where it would collapse forty selected rows onto the one under the pointer before the menu
    // that was about to act on them ever opened.
    if (e.button !== 0 || (IS_MAC && e.ctrlKey)) return;
    const current = editingCellRef.current;
    if (current && current.rowIndex === rowIndex && current.col === col) {
      return;
    }
    if (e.detail >= 2) {
      e.preventDefault();
      void moveEditTo(rowIndex, col);
      return;
    }
    if (current) {
      e.preventDefault();
      void commitAndExit();
    }
    // Shift- or modifier-click means "extend the row selection" here, so the browser must not read
    // it as "extend the text selection" as well. A plain click keeps its normal text-dragging.
    if (e.shiftKey || hasPrimaryModifier(e)) e.preventDefault();
    selectRow(rowIndex, e);
    // The grid owns the select-all shortcut, and a shortcut only reaches it while it holds focus.
    // Clicking a row is what tells us the user is working in the grid — but only on a single
    // click: the second click of a double-click goes to edit mode, whose input takes focus itself.
    scrollRef.current?.focus({ preventScroll: true });
  }

  /**
   * The right-click menu, opened on the cell under the pointer.
   *
   * The selection moves onto that row first when the row was not already in it, the way a list
   * does: every entry below the two cell ones acts on the selected rows, and a menu acting on rows
   * other than the highlighted ones would be read as acting on the wrong ones. A right-click
   * *inside* the selection leaves it alone — that is what makes "copy these forty rows" one
   * gesture rather than a re-selection followed by a copy of a single row.
   */
  function handleCellContextMenu(
    e: React.MouseEvent<HTMLTableCellElement>,
    rowIndex: number,
    col: string,
  ) {
    e.preventDefault();
    // A staged edit is written out first, exactly as leaving the cell by any other route does: the
    // entries below copy `rows`, and a value still sitting in the input is not in them yet.
    void commitAndExit();
    if (!selectedRows.has(rowIndex)) selectOnly(rowIndex);
    setMenu({ rowIndex, col, x: e.clientX, y: e.clientY });
  }

  /** Puts text on the clipboard and closes the menu it was chosen from. A webview that refuses the
   *  clipboard outright is worth saying out loud — the alternative is a copy that silently did
   *  nothing and a paste of whatever was there before. */
  function copyToClipboard(text: string) {
    closeMenu();
    closeHeaderMenu();
    void copyText(text).catch((e) => onError(errorMessage(t, e)));
  }

  /**
   * Follows a foreign key: the table it points at, opened on the row it points to.
   *
   * A key pointing back at this very table — a `parent_id`, a `replied_to` — never changes which
   * table is on screen, so the workspace has nothing to swap and this grid would not be re-mounted
   * with the new conditions. Its bar is set here instead, exactly as Apply sets it.
   */
  function openReferenced(fk: { table: string; column: string; value: string }) {
    closeMenu();
    if (fk.table !== selectedTable) {
      onOpenRelated?.(fk.table, fk.column, fk.value);
      return;
    }
    setFilterRows([filterRowFor<FilterOperator>(fk.column, "eq", fk.value)]);
    setAppliedFilters([{ column: fk.column, operator: "eq", value: fk.value }]);
    setPage(0);
  }

  function handleGridKeyDown(e: React.KeyboardEvent<HTMLDivElement>) {
    // While a cell is open for editing its input owns the keyboard: select-all takes the text,
    // and Escape backs the edit out.
    if (editingCellRef.current) return;
    // The right-click menu answers the keyboard while it is up — Escape closes it. Clearing the
    // selection as well would be two things done by one key, and the selection is what the menu is
    // about to act on. The cell dialog is the same story: it takes Escape, and what it was opened
    // from is still selected behind it.
    if (menu !== null || headerMenu !== null || expandedCell !== null) return;
    // Ctrl/Cmd+C puts the selected rows on the clipboard as TSV — the same thing the menu's TSV
    // entry does, and the same chord the query tab's result answers.
    //
    // Not a registered shortcut, because it must not answer from wherever the focus happens to be:
    // Ctrl+C in the filter bar copies the text in the box, and copying forty rows instead would be
    // this grid taking a key that was never about it. Here it is the grid's own, and only while
    // the grid holds the keyboard.
    if (hasPrimaryModifier(e) && (e.key === "c" || e.key === "C")) {
      // A value dragged out of a cell is still the browser's copy to make. The grid keeps the
      // keyboard through such a drag — clicking a row is what hands it over — so without this the
      // chord would put whole rows on the clipboard instead of the words under the pointer.
      if (selectedRows.size === 0 || (window.getSelection()?.toString() ?? "") !== "") return;
      e.preventDefault();
      copyToClipboard(spreadsheetText(columns, selectedRowsInOrder()));
      return;
    }
    if (e.key === "Escape" && selectedRows.size > 0) {
      e.preventDefault();
      clearSelection();
      return;
    }
    // Delete/Backspace on a selection is the toolbar's delete button, down to the confirmation
    // step — so it is gated on exactly what disables that button.
    if (e.key === "Delete" || e.key === "Backspace") {
      if (readOnly || loading || deleting || selectedRows.size === 0) return;
      e.preventDefault();
      void openDeleteConfirm();
    }
  }

  /**
   * The two chords the Data tab answers from wherever the focus happens to be: `Ctrl+A` — `⌘A` on a
   * Mac — for every row on the page, and `Ctrl+F` for the filter bar.
   *
   * Answered from the window rather than from the scroll box, because "click the grid first" is not
   * something the user should have to know: the tab is the one on screen, so the tab is what the
   * chord is about. `active` is what keeps that unambiguous — every background connection tab has a
   * grid mounted too, and each would otherwise answer the same keystroke alongside this one.
   *
   * What used to sit here as well was a guess at everything standing over the grid: a scan of the
   * document for `[role="dialog"]`, and a check on this component's own right-click menu. Both are
   * now the dispatcher's business, and both are counted rather than sniffed — see
   * `src/core/shortcuts/`.
   */
  useShortcut(
    "grid.selectAll",
    () => {
      if (rows.length === 0) return;
      selectAllRows();
      // Delete and Escape act on the selection and are the grid's own keys, so the keyboard is
      // handed to it — otherwise a selection made from across the pane could not be acted on
      // without a click.
      focusGrid();
    },
    active,
  );

  useShortcut(
    "grid.focusFilter",
    () => {
      // The grid is being left for the bar above it, so whatever cell is open goes back as it was
      // rather than being written out by the blur that follows — see {@link cancelEdit}.
      cancelEdit();
      filterBarRef.current?.focusValue();
    },
    active,
  );

  /** The row the menu was opened on, or null when it is closed — and null again if that row has
   *  gone from under it, which is what stops an entry acting on a row that is no longer there. */
  const menuRow = menu === null ? null : (rows[menu.rowIndex] ?? null);

  /** Where the cell under the menu points, when it points anywhere: the referenced table and
   *  column, and the value to look up there. Null for a column that is not a foreign key, and for
   *  one that is but holds nothing — a key with no value points at no row. */
  const menuForeignKey = (() => {
    if (menu === null || menuRow === null) return null;
    const fk = columnMeta[menu.col]?.foreignKey ?? null;
    if (fk === null) return null;
    const value = normalizeCellValue(menuRow[menu.col]);
    return value === null || value === "" ? null : { table: fk.table, column: fk.column, value };
  })();

  /** The rows the menu's row entries act on: the selection, which the right-click has already moved
   *  onto the row under the pointer if it was outside it. */
  const menuRows = (() => {
    if (menuRow === null) return [];
    const selected = selectedRowsInOrder();
    return selected.length > 0 ? selected : [menuRow];
  })();

  /** The columns an `INSERT` copied from here may name. A generated column is MySQL's to compute,
   *  and a statement that names one is refused outright — the AUTO_INCREMENT column is not, which
   *  is why it stays in and is only dropped by the second of the two copies. */
  const insertableColumns = useMemo(
    () => columns.filter((c) => {
      const meta = columnMeta[c];
      return meta === undefined || !dialect.isGenerated(meta);
    }),
    [columns, columnMeta, dialect],
  );

  /** The columns whose values came over base64-encoded, and which a copied `INSERT` therefore has
   *  to decode again rather than quote as the text they look like. */
  const binaryColumns = useMemo(
    () => new Set(columns.filter((c) => {
      const meta = columnMeta[c];
      return meta !== undefined && dialect.isBinary(meta);
    })),
    [columns, columnMeta, dialect],
  );

  const pageCount = Math.max(1, Math.ceil(total / pageSize));
  const allPageRowsSelected = rows.length > 0 && selectedRows.size === rows.length;
  // Under a filter, `total` counts the matching rows rather than the table's — and a whole-table
  // delete would still take every row, matching or not. Neither of the two shortcuts below can
  // say what it means then, so both are withheld until the filters are cleared.
  const unfiltered = appliedFilters.length === 0;
  // Only worth offering while the page is a window onto a bigger table: with a single page,
  // deleting every selected row already is deleting the whole table.
  const canDeleteWholeTable = unfiltered && allPageRowsSelected && pageCount > 1;
  // Resetting the counter only means anything when the delete leaves the table empty — with
  // rows still in it, the next insert has to keep clearing the ids that are already there.
  const canResetAutoIncrement =
    autoIncrementColumn !== null &&
    unfiltered &&
    (deleteWholeTable || (allPageRowsSelected && pageCount === 1));

  return (
    <div className={styles.sqlTable}>
      {columns.length > 0 && (
        <FilterBar
          ref={filterBarRef}
          fields={columns}
          operators={filterOperators}
          defaultOperator="eq"
          operatorLabel={(op) => t(`sqlTable.op.${op}`)}
          rows={filterRows}
          onChange={setFilterRows}
          onApply={() => void applyFilters()}
          applyDisabled={loading || deleting}
        />
      )}
      <div className={styles.scrollWrap}>
        <div
          className={styles.scroll}
          ref={scrollRef}
          tabIndex={-1}
          onKeyDown={handleGridKeyDown}
          onScroll={handleScroll}
        >
          <table
            ref={tableRef}
            // What a row would hold if all of it were built. Once the columns are windowed the body
            // rows hold only the drawn ones, and a reader counting cells would otherwise be told
            // the table is sixteen columns wide and put the wrong header to every value in it. The
            // header row is exempt from the matching `aria-colindex` below: it never drops a cell,
            // so its columns are already where they are counted to be.
            aria-colcount={columns.length}
            // Rows pinned whenever they are windowed; columns pinned once they have been measured.
            // Two classes because they answer to different conditions — the measuring can fail in a
            // way the windowing cannot, and a windowed grid whose rows were left to size themselves
            // is the one state that must not exist.
            className={
              [virtual && styles.gridRows, gridColumns && styles.gridFixed]
                .filter(Boolean)
                .join(" ") || undefined
            }
            style={
              virtual
                ? gridStyle(
                    ROW_HEIGHT,
                    gridColumns === null ? null : gridColumns.total + editingColumnExtra
                  )
                : undefined
            }
          >
            {gridColumns && (
              <colgroup>
                {gridColumns.widths.map((width, c) => (
                  // The column being edited opens to hold the input, and every other column stays
                  // exactly where it was — the table is widened by the difference rather than
                  // taking it out of its neighbours.
                  <col
                    key={columns[c]}
                    style={{
                      width: columns[c] === editingCell?.col ? Math.max(width, EDIT_COLUMN) : width,
                    }}
                  />
                ))}
              </colgroup>
            )}
            {/* Every column, drawn or not, and on purpose. The header is one row where the body is
                fifty, so windowing it would save a fiftieth of what windowing the body saves — and
                it would cost the two things that are measured off the DOM rather than out of the
                data: `headerExtras` reads the chevron and the FK chip out of each header cell, and
                `measureColumnWidths` reads each column's laid-out width for the edit input. Both
                would come back short for every column that had been left out, and the widths they
                feed are what the window itself is worked out from. */}
            <thead>
              <tr>
                {columns.map((c) => {
                  const sorted = sort?.column === c ? sort : null;
                  const foreignKey = columnMeta[c]?.foreignKey ?? null;
                  return (
                    <th
                      key={c}
                      ref={(el) => {
                        if (el) thRefs.current.set(c, el);
                        else thRefs.current.delete(c);
                      }}
                      className={styles.headerCell}
                      // `aria-sort` is what tells a screen reader the grid is ordered by this
                      // column; the chevron only says it to the eye.
                      aria-sort={sorted ? (sorted.desc ? "descending" : "ascending") : "none"}
                      // On a Mac the secondary click is also `Ctrl+Click`, which WebKit reports as
                      // an ordinary click on top of the `contextmenu` — without this, asking the
                      // header for its menu would re-sort the grid underneath it.
                      onClick={(e) => {
                        if (IS_MAC && e.ctrlKey) return;
                        void toggleSort(c);
                      }}
                      // The header name cannot be selected — `.headerCell` turns that off so that
                      // clicking a column repeatedly to cycle its sort does not start highlighting
                      // it — so the menu is the only way to take a copy of it.
                      onContextMenu={(e) => {
                        e.preventDefault();
                        setHeaderMenu({ col: c, x: e.clientX, y: e.clientY });
                      }}
                    >
                      {/* Around the name rather than around the whole cell, and not only because a
                          hook cannot be called from this loop: the FK chip beside it has a tooltip
                          of its own, and a cell-wide one would still count as hovered while the
                          pointer sat on the chip — two bubbles at once. Side by side, entering one
                          leaves the other. */}
                      <Tooltip
                        text={t(
                          sorted
                            ? sorted.desc
                              ? "sqlTable.sortDesc"
                              : "sqlTable.sortAsc"
                            : "sqlTable.sortNone",
                          { column: c },
                        )}
                      >
                        {c}
                      </Tooltip>
                      {foreignKey && (
                        // Its own tooltip, so what the column points at is readable without having
                        // to remember the schema. Drawn by the app rather than by `title`, which
                        // would have put it in the system's font rather than the one the grid is
                        // drawn in.
                        <Tooltip
                          text={t("sqlTable.foreignKey", {
                            table: foreignKey.table,
                            column: foreignKey.column,
                          })}
                        >
                          <span className={styles.fkBadge}>FK</span>
                        </Tooltip>
                      )}
                      {/* Always rendered, empty when unsorted: the column is measured for the
                          edit input's width, and an indicator that comes and goes would change
                          that width under it. */}
                      <span className={styles.sortIcon}>
                        {sorted && (sorted.desc ? <ChevronDownIcon /> : <ChevronUpIcon />)}
                      </span>
                    </th>
                  );
                })}
              </tr>
            </thead>
            <tbody>
              {/* The rows outside the window, as the height they would have taken. Two elements
                  instead of hundreds, and the scrollbar is the length it would have been. Always
                  both, even at zero height: a spacer that disappeared at the end of the page would
                  hand `tr:last-child` to a real row and change the table's height at exactly the
                  point a scroll is trying to settle. */}
              {virtual && (
                <tr className={styles.spacer} style={{ height: view.padTop }} aria-hidden="true">
                  <td colSpan={columns.length} />
                </tr>
              )}
              {rows.slice(view.first, view.last).map((row, offset) => {
                // The index into the page, not into what is drawn. Every other part of this
                // component — the selection, the staged edit, the delete keys — is indexed the same
                // way, so windowing the rows changes none of it.
                const i = view.first + offset;
                return (
                  <tr
                    key={i}
                    className={selectedRows.has(i) ? styles.rowSelected : undefined}
                    aria-selected={selectedRows.has(i)}
                  >
                    {/* The columns outside the window, as one cell spanning them. It needs no
                        width of its own: a fixed layout takes every column's from the colgroup
                        above, which holds all of them whether or not a cell was built for them, so
                        the space left here is the space those columns were measured to want. */}
                    {columnsVirtual && colView.first > 0 && (
                      <td colSpan={colView.first} aria-hidden="true" />
                    )}
                    {drawnColumns.map((c, at) => {
                      // Where this cell sits in the whole table rather than in what was drawn —
                      // see `aria-colcount` above. One-based, as the attribute counts.
                      const colIndex = colView.first + at + 1;
                      const isEditing = editingCell?.rowIndex === i && editingCell.col === c;
                      if (isEditing) {
                        const multiline = isMultilineType(columnMeta[c]?.dataType);
                        const cellWidth = columnWidths[c];
                        return (
                          <td
                            key={c}
                            aria-colindex={colIndex}
                            className={styles.cellEditing}
                            style={cellWidth ? { width: cellWidth } : undefined}
                          >
                            {multiline ? (
                              <textarea
                                ref={editInputRef as React.RefObject<HTMLTextAreaElement>}
                                value={editValue}
                                onChange={handleEditChange}
                                onKeyDown={handleEditKeyDown}
                                onBlur={() => handleInputBlur(i, c)}
                                className={styles.cellTextarea}
                                rows={1}
                                autoComplete="off"
                                autoCorrect="off"
                                autoCapitalize="off"
                                spellCheck={false}
                              />
                            ) : (
                              <input
                                ref={editInputRef as React.RefObject<HTMLInputElement>}
                                type="text"
                                value={editValue}
                                onChange={handleEditChange}
                                onKeyDown={handleEditKeyDown}
                                onBlur={() => handleInputBlur(i, c)}
                                className={styles.cellInput}
                                autoComplete="off"
                                autoCorrect="off"
                                autoCapitalize="off"
                                spellCheck={false}
                              />
                            )}
                          </td>
                        );
                      }
                      const raw = row[c];
                      const isNull = raw === null || raw === undefined;
                      // What the cell draws, not the value: see `cellPreview`.
                      const value = cellPreview(displayValue(raw));
                      const isDirty =
                        pendingRowRef.current?.rowIndex === i &&
                        Object.prototype.hasOwnProperty.call(pendingRowRef.current.changes, c);
                      const cellClassName = [isNull && styles.cellNull, isDirty && styles.cellDirty]
                        .filter(Boolean)
                        .join(" ");
                      return (
                        <td
                          key={c}
                          aria-colindex={colIndex}
                          title={value}
                          className={cellClassName || undefined}
                          onMouseDown={(e) => handleCellMouseDown(e, i, c)}
                          // Only on a cell being read. The cell open for editing is the branch
                          // above and deliberately has none: right-clicking inside its input is
                          // the webview's own cut/copy/paste, which nothing here replaces.
                          onContextMenu={(e) => handleCellContextMenu(e, i, c)}
                        >
                          {value}
                        </td>
                      );
                    })}
                    {columnsVirtual && colView.last < columns.length && (
                      <td colSpan={columns.length - colView.last} aria-hidden="true" />
                    )}
                  </tr>
                );
              })}
              {virtual && (
                <tr className={styles.spacer} style={{ height: view.padBottom }} aria-hidden="true">
                  <td colSpan={columns.length} />
                </tr>
              )}
            </tbody>
          </table>
          {!loading && rows.length === 0 && <p className="muted">{t("sqlTable.noRows")}</p>}
        </div>
        {(loading || deleting) && (
          <LoadingOverlay label={deleting ? t("sqlTable.deletingRows") : t("sqlTable.loading")} />
        )}
      </div>
      <div className={styles.footer}>
        <ActionBar
          actions={[
            {
              key: "reload",
              icon: ReloadIcon,
              label: withReloadShortcut(t("sqlTable.reloadRows")),
              disabled: loading || deleting,
              busy: loading,
              onClick: () => void reload(),
            },
            {
              key: "insert",
              icon: PlusIcon,
              label: t("sqlTable.insertRows"),
              disabled: readOnly || loading || deleting || columns.length === 0,
              disabledHint: readOnly ? t("common.readOnlyConnection") : undefined,
              onClick: () => void openInsert("new"),
            },
            {
              key: "clone",
              icon: CopyIcon,
              label: t("sqlTable.cloneRows"),
              disabled: readOnly || loading || deleting || selectedRows.size === 0,
              disabledHint: readOnly ? t("common.readOnlyConnection") : undefined,
              onClick: () => void openInsert("clone"),
            },
            {
              key: "delete",
              icon: TrashIcon,
              label: t("sqlTable.deleteRows"),
              danger: true,
              disabled: readOnly || loading || deleting || selectedRows.size === 0,
              disabledHint: readOnly ? t("common.readOnlyConnection") : undefined,
              onClick: () => void openDeleteConfirm(),
            },
          ]}
        />
        <Pagination
          page={page}
          pageCount={pageCount}
          total={total}
          pageSize={pageSize}
          pageSizeOptions={PAGE_SIZES}
          loading={loading}
          onPageChange={setPage}
          onPageSizeChange={(n) => {
            setPageSize(n);
            setPage(0);
          }}
        />
      </div>
      {headerMenu !== null && (
        <ContextMenu x={headerMenu.x} y={headerMenu.y} onClose={closeHeaderMenu}>
          <button type="button" onClick={() => copyToClipboard(headerMenu.col)}>
            {t("sqlTable.copyColumnName")}
          </button>
        </ContextMenu>
      )}
      {menu !== null && menuRow !== null && (
        <ContextMenu x={menu.x} y={menu.y} onClose={closeMenu}>
          <button
            type="button"
            onClick={() => {
              setExpandedCell({ rowIndex: menu.rowIndex, col: menu.col });
              closeMenu();
            }}
          >
            {t("sqlTable.expandCell")}
          </button>
          <button type="button" onClick={() => copyToClipboard(displayValue(menuRow[menu.col]))}>
            {t("sqlTable.copyCellValue")}
          </button>
          {menuForeignKey !== null && (
            <button type="button" onClick={() => openReferenced(menuForeignKey)}>
              {t("sqlTable.openReferencedRow", { table: menuForeignKey.table })}
            </button>
          )}
          <div className="context-menu-separator" />
          <button
            type="button"
            onClick={() =>
              copyToClipboard(
                insertStatements(selectedTable, insertableColumns, menuRows, null, binaryColumns),
              )
            }
          >
            {menuRows.length === 1
              ? t("sqlTable.copyInsert")
              : t("sqlTable.copyInsertRows", { n: menuRows.length })}
          </button>
          {/* Only a table MySQL numbers itself has a column an insert can sensibly leave out.
              Everywhere else every column is a value somebody has to provide, and an INSERT short
              of one would simply be refused — so the entry is not offered rather than offered and
              broken. */}
          {autoIncrementColumn !== null && (
            <button
              type="button"
              onClick={() =>
                copyToClipboard(
                  insertStatements(
                    selectedTable,
                    insertableColumns,
                    menuRows,
                    autoIncrementColumn,
                    binaryColumns,
                  ),
                )
              }
            >
              {menuRows.length === 1
                ? t("sqlTable.copyInsertWithout", { column: autoIncrementColumn })
                : t("sqlTable.copyInsertRowsWithout", {
                    n: menuRows.length,
                    column: autoIncrementColumn,
                  })}
            </button>
          )}
          <button type="button" onClick={() => copyToClipboard(spreadsheetText(columns, menuRows))}>
            {menuRows.length === 1
              ? t("sqlTable.copyAsTsv")
              : t("sqlTable.copyRowsAsTsv", { n: menuRows.length })}
          </button>
          <button type="button" onClick={() => copyToClipboard(csvText(columns, menuRows))}>
            {menuRows.length === 1
              ? t("sqlTable.copyAsCsv")
              : t("sqlTable.copyRowsAsCsv", { n: menuRows.length })}
          </button>
          <button type="button" onClick={() => copyToClipboard(jsonText(columns, menuRows))}>
            {menuRows.length === 1
              ? t("sqlTable.copyAsJson")
              : t("sqlTable.copyRowsAsJson", { n: menuRows.length })}
          </button>
          <div className="context-menu-separator" />
          <button
            type="button"
            disabled={loading || deleting}
            onClick={() => {
              closeMenu();
              void reload();
            }}
          >
            {withReloadShortcut(t("sqlTable.reloadRows"))}
          </button>
        </ContextMenu>
      )}
      {insertMode !== null && (
        <InsertRowsDialog
          table={selectedTable}
          columns={columns}
          columnMeta={columnMeta}
          seedRows={insertMode === "clone" ? selectedRowsInOrder() : undefined}
          onCancel={() => {
            setInsertMode(null);
            focusGrid();
          }}
          onSubmit={submitInsert}
        />
      )}
      {confirmingDelete && (
        <ConfirmDialog
          title={t("sqlTable.deleteRowsTitle")}
          message={t("sqlTable.deleteRowsMessage", { n: selectedRows.size })}
          confirmLabel={t("common.delete")}
          danger
          onConfirm={() => void confirmDelete()}
          onCancel={() => {
            setConfirmingDelete(false);
            focusGrid();
          }}
        >
          {(canDeleteWholeTable || canResetAutoIncrement) && (
            <div className={styles.deleteOptions}>
              {canDeleteWholeTable && (
                <Checkbox
                  className={styles.deleteOption}
                  label={t("sqlTable.deleteAllRowsOption", { total })}
                  checked={deleteWholeTable}
                  onChange={(e) => {
                    setDeleteWholeTable(e.target.checked);
                    // The reset only exists because of this option on a multi-page table;
                    // taking it back takes its follow-up with it.
                    if (!e.target.checked) setResetAutoIncrement(false);
                  }}
                />
              )}
              {canResetAutoIncrement && (
                <Checkbox
                  className={styles.deleteOption}
                  label={t("sqlTable.resetAutoIncrementOption", { column: autoIncrementColumn ?? "" })}
                  checked={resetAutoIncrement}
                  onChange={(e) => setResetAutoIncrement(e.target.checked)}
                />
              )}
            </div>
          )}
        </ConfirmDialog>
      )}
      {/* The row is looked up at render time rather than held in state, so a value edited or
          reloaded underneath the dialog is the one it shows. A reload that drops the row — a page
          that came back shorter — closes it rather than showing a blank. */}
      {expandedCell !== null && rows[expandedCell.rowIndex] !== undefined && (
        <CellDialog
          column={expandedCell.col}
          // The rows here are one page of a table somebody may have sorted and filtered, so there
          // is no number that means anything outside this screenful.
          rowNumber={null}
          value={rows[expandedCell.rowIndex][expandedCell.col]}
          onClose={() => setExpandedCell(null)}
        />
      )}
    </div>
  );
}

export default SqlTable;
