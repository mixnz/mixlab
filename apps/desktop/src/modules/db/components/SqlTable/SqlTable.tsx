import { useCallback, useEffect, useEffectEvent, useLayoutEffect, useMemo, useRef, useState } from "react";
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
import { filterRowFor, initialFilterRows, toQueryFilters, type FilterRow } from "../../filters";
import {
  FILTER_OPERATORS,
  operatorArity,
  type FilterOperator,
  type SqlFilter,
} from "../../sql/filters";
import {
  cellPreview,
  columnEdges,
  gridStyle,
  measureColumns,
  useVirtualColumns,
  useVirtualRows,
  widestValues,
} from "../../../../core/virtualRows";
import {
  fileTable,
  rememberedTable,
  sameRequest,
  type Sort,
  type TableCache,
  type TableRequest,
} from "./request";
import { csvText, insertStatements, jsonText, spreadsheetText } from "./rowText";
import type { SqlColumnMeta } from "../../types";
import { cacheKey as tableCacheKey } from "../../schemaTokens";
import styles from "./SqlTable.module.css";

/** Where handing the browser the whole page stops being the cheap thing to do. Below this the rows
 *  are rendered as they always were, and the table sizes its own columns. */
const VIRTUAL_FROM = 60;

/** The same question asked sideways: how many columns a table has to have before the drawn rows are
 *  cut down to the ones on screen as well.
 *
 * Higher than the row threshold, and deliberately: a column is only ever windowed once the widths
 * have been measured, so the grid has already paid for the whole page by the time this can apply —
 * what it saves is every render after that, and every cell in every row it saves them on. Forty is
 * where a page of them stops being a few hundred cells and starts being a few thousand. */
const VIRTUAL_COLUMNS_FROM = 40;

/** What {@link useVirtualColumns} is handed before there are any widths to speak of: one edge and no
 *  columns. A constant rather than a fresh `[0]`, since the hook searches it on every scroll and a
 *  new array each render would be a new search each render. */
const NO_EDGES: readonly number[] = [0];

/** How tall a row of this grid is — stated, never measured; see `virtualRows.ts` for why that
 *  distinction is what makes a window of rows sound. 29px is what a row here comes to: a 20px
 *  line, 4px of padding above and below, and the 1px rule underneath. The stylesheet pins the rows
 *  to it, so changing the grid's font or padding means changing this with them.
 *
 *  Was 33px, back when the line was 24px. `gridRows.test.ts` guards the half of that promise a
 *  stylesheet can break — a cell that takes vertical padding again would make a drawn row taller
 *  than this says it is, and the end of the page would drift as you scrolled towards it. */
const ROW_HEIGHT = 29;

/** What a cell spends on itself before any text goes in it: 8px of padding either side and the 1px
 *  rule down its right edge. */
const CELL_CHROME = 17;

/** The band a measured column is kept inside, chrome included. The ceiling is the width at which a
 *  cell stops being worth widening — past it the value is cut off with an ellipsis either way. */
const MIN_COLUMN = 48;
const MAX_COLUMN = 320;

/** How wide the column being edited opens to, when it is narrower than this.
 *
 * A cell wide enough to read a value in is not wide enough to edit one in, which is why the input
 * has always had a floor of its own. It used to reach that floor by pushing its column open — the
 * table sized itself around whatever was in it — and a pinned column cannot be pushed: the input
 * simply overhung its cell and read as something floating loose over the row. So the column itself
 * opens for as long as the edit lasts, which is what the pushing looked like from the outside.
 *
 * Kept the same as the `min-width` the stylesheet gives the input, which is still what does the job
 * in a grid too small to be windowed. */
const EDIT_COLUMN = 240;

interface EditingCell {
  rowIndex: number;
  col: string;
}

/** Which cell the right-click menu was opened on, and where the pointer was when it was. Null
 *  while the menu is closed. */
interface CellMenu {
  rowIndex: number;
  col: string;
  x: number;
  y: number;
}

/** The header click cycle: unsorted → descending → ascending → unsorted. */
function nextSort(current: Sort | null, column: string): Sort | null {
  if (current?.column !== column) return { column, desc: true };
  return current.desc ? { column, desc: false } : null;
}

function normalizeCellValue(raw: unknown): string | null {
  if (raw === null || raw === undefined) return null;
  return typeof raw === "object" ? JSON.stringify(raw) : String(raw);
}

function displayValue(raw: unknown): string {
  return normalizeCellValue(raw) ?? "NULL";
}

function isMultilineType(type: string | undefined): boolean {
  if (!type) return false;
  const t = type.toLowerCase();
  return t.includes("text") || t.includes("json") || t.includes("blob");
}

/** One table's filter bar as it was left behind: the rows still being edited, and the conditions
 * that were actually running against the grid. */
export interface RememberedFilters {
  rows: FilterRow<FilterOperator>[];
  applied: SqlFilter[];
  /** Which shape of the database the conditions were written against — see {@link Props.schemaToken}. */
  schemaToken: number;
}

/** Every table's bar, by the table it belongs to. Held by the workspace rather than here: the grid
 * is unmounted whenever the header leaves the Data tab, and a cache living inside it would go with
 * it — the conditions have to outlive a trip to Structure or Query, not just a trip to another
 * table. */
export type FilterCache = Map<string, RememberedFilters>;

/**
 * The bar remembered for a table, or nothing when there is nothing worth speaking for.
 *
 * Conditions written before the app last changed this table are nothing: they name columns that may
 * since have been renamed or dropped, and the name they are filed under may since have been dropped
 * and given to a different table altogether — put back onto that table, they answer the next read
 * with `Unknown column` rather than with rows. Deleting the entry at the moment of the change is not
 * enough on its own: the grid is still holding the bar in state and files it straight back on the way
 * out, so the check has to be here, where the cache is read.
 */
function rememberedFilters(
  cache: FilterCache,
  key: string,
  schemaToken: number,
): RememberedFilters | undefined {
  const entry = cache.get(key);
  return entry?.schemaToken === schemaToken ? entry : undefined;
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
const DEFAULT_PAGE_SIZE = 1000;

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
  const tableKey = tableCacheKey(selectedDb, selectedTable);
  // Everything below opens on what this table was last left showing, when it has been here before.
  // A grid mounted afresh — the connection reopened, another database picked and this one come back
  // to — is then the grid that was left, rather than a first read of the table all over again.
  const restored = rememberedTable(tableCache, tableKey, schemaToken);
  const [page, setPage] = useState(restored?.request.page ?? 0);
  const [pageSize, setPageSize] = useState(restored?.request.pageSize ?? DEFAULT_PAGE_SIZE);
  const [rows, setRows] = useState<Record<string, unknown>[]>(restored?.rows ?? []);
  const [columns, setColumns] = useState<string[]>(restored?.columns ?? []);
  const [columnMeta, setColumnMeta] = useState<Record<string, SqlColumnMeta>>(
    restored?.columnMeta ?? {},
  );
  const [primaryKey, setPrimaryKey] = useState<string[]>(restored?.primaryKey ?? []);
  const [autoIncrementColumn, setAutoIncrementColumn] = useState<string | null>(
    restored?.autoIncrementColumn ?? null,
  );
  const [total, setTotal] = useState(restored?.total ?? 0);
  const [sort, setSort] = useState<Sort | null>(restored?.request.sort ?? null);
  // The filter bar edits `filterRows` freely; only Apply copies them into `appliedFilters`, which
  // is what the fetch below reads. Keeping the two apart is what stops a half-typed condition
  // from reloading the grid on every keystroke.
  //
  // Both start from whatever this table's bar was left carrying, so a grid mounted afresh — the
  // connection reopened, or a table picked after none was — opens on the conditions it closed on.
  // A trip to Structure or Query no longer comes through here at all: the grid stays mounted
  // behind those tabs, bar and all.
  const [filterRows, setFilterRows] = useState<FilterRow<FilterOperator>[]>(
    () => rememberedFilters(filterCache, tableKey, schemaToken)?.rows ?? []
  );
  const [appliedFilters, setAppliedFilters] = useState<SqlFilter[]>(
    () => rememberedFilters(filterCache, tableKey, schemaToken)?.applied ?? []
  );
  // The table whose columns the bar was last seeded from. The seed needs the column list, which
  // is only known once the first fetch lands (or from the cache, when there is one).
  const filtersSeededForRef = useRef<string | null>(null);

  const scrollRef = useRef<HTMLDivElement>(null);
  /** The filter bar above, so `Ctrl+F` can put the caret in it — see {@link FilterBarHandle}. */
  const filterBarRef = useRef<FilterBarHandle>(null);

  /** Where the grid is scrolled to, kept up to date as it moves. This, and not the box itself, is
   *  what the position is read from — the box cannot be asked at either of the two moments it
   *  matters. On the way out React has already detached the ref, and a pane hidden behind another
   *  tab has no layout box at all; both would answer "the top", and filing that would lose a grid
   *  left halfway down. */
  const scrollPosRef = useRef({ top: restored?.scrollTop ?? 0, left: restored?.scrollLeft ?? 0 });

  // The request the rows on screen came from. Held from one render to the next so that coming back
  // to the Data tab can tell "nothing has changed since these rows were read" — which is free — from
  // "the table, the page, the order or the conditions moved while the tab was hidden", which is a
  // read owed. It is also what says which conditions the rows answer, so that what is filed away for
  // the next visit is a page of rows and the very filters that produced it.
  const requestRef = useRef<TableRequest | null>(null);

  /**
   * Files the grid away as it stands, under the table it belongs to, for the next visit to open on.
   *
   * Only what a read actually produced is filed. Until the first one lands there is nothing here
   * worth keeping — an entry written then would be restored later as a grid that has already been
   * read, every row of it empty, and the fetch that should have filled it is the very thing the
   * entry says is not owed. And the request filed is the one the rows came from, never the one the
   * form is asking for next: conditions applied while the read was still out belong to the rows
   * that answer them, not to the rows already on screen.
   */
  function rememberTable(key: string) {
    const loaded = requestRef.current;
    if (columns.length === 0 || loaded === null) return;
    fileTable(tableCache, key, {
      columns,
      columnMeta,
      primaryKey,
      autoIncrementColumn,
      rows,
      total,
      request: loaded,
      scrollTop: scrollPosRef.current.top,
      scrollLeft: scrollPosRef.current.left,
    });
  }

  /** The bar as it was last committed, for the two writes that cannot read the state themselves: the
   * render that first sees another table — where `filterRows` still belongs to the outgoing one, but
   * `schemaToken` is already counted for the incoming one — and a cleanup, which runs long after the
   * last render. Declared above both so either can reach it. */
  const filterStateRef = useRef({
    key: tableKey,
    rows: filterRows,
    applied: appliedFilters,
    schemaToken,
  });
  useEffect(() => {
    filterStateRef.current = {
      key: viewTableKey,
      rows: filterRows,
      applied: appliedFilters,
      schemaToken,
    };
  });

  /**
   * Files the bar away under the table it belongs to and the shape of the database it was written
   * against. That shape is what a later visit judges it by: conditions the workspace has let go of
   * are filed straight back from here, and the token is the only thing that tells them apart from
   * conditions that still mean something.
   *
   * Only a bar that has had its opening row is worth remembering. Before the columns land there is
   * nothing in it, and filing that away would read on the way back in as a bar deliberately
   * emptied — leaving the table with no `id =` row to start from, for good.
   */
  // A callback rather than an effect event: the table swap below calls it during render.
  const rememberFilters = useCallback(() => {
    const { key, rows, applied, schemaToken: token } = filterStateRef.current;
    if (filtersSeededForRef.current !== key) return;
    filterCache.set(key, { rows, applied, schemaToken: token });
  }, [filterCache]);

  // What the fetch below reads — the page, the order and the conditions — is about one table, so
  // it is swapped over here, during the render that first sees a new table, rather than from an
  // effect. An effect would be too late: it and the fetch's own effect run after the same commit,
  // so the request would already have gone out naming the new table with the previous one's page,
  // sort column and filters — and a condition on a column the new table hasn't got comes back an
  // error rather than an empty result.
  const [viewTableKey, setViewTableKey] = useState(tableKey);
  if (viewTableKey !== tableKey) {
    // Put the outgoing table's bar away before its state is replaced. This is where it has to
    // happen: by the time any effect runs, `filterRows` already belongs to the new table.
    rememberFilters();
    rememberTable(viewTableKey);
    setViewTableKey(tableKey);
    // A table that has been here before opens where it was left rather than at its first page in
    // its own order; `pageSize` falls back to the one in hand rather than to the default, so a
    // size chosen for the last table is still carried onto a table never seen before.
    setPage(restored?.request.page ?? 0);
    setPageSize(restored?.request.pageSize ?? pageSize);
    setSort(restored?.request.sort ?? null);
    // The rows of the bar are put back alongside the columns they name, in the effect below.
    setAppliedFilters(rememberedFilters(filterCache, tableKey, schemaToken)?.applied ?? []);
  }

  // The same swap, for the app having changed this table under the grid rather than the user
  // having moved to another one. The conditions that were running were written against columns
  // that may since have been renamed or dropped, and sending them again would answer the table
  // with `Unknown column` rather than with rows; the workspace has already let go of the bar they
  // came from, so this picks up nothing and the grid opens unfiltered. Here rather than in the
  // effect below for the reason the swap above is: by then the fetch has already gone out.
  const [viewSchemaToken, setViewSchemaToken] = useState(schemaToken);
  if (viewSchemaToken !== schemaToken) {
    setViewSchemaToken(schemaToken);
    setAppliedFilters(rememberedFilters(filterCache, tableKey, schemaToken)?.applied ?? []);
  }

  // The bar is put away on the way out as well as on the way to another table: the grid is
  // unmounted when the connection tab closes, and what it was carrying should be there again if
  // the same table is opened later.
  useEffect(() => {
    return () => {
      rememberFilters();
    };
  }, [rememberFilters]);

  /** The grid as it stands, for the write on the way out — the same reason `filterStateRef` exists:
   *  a cleanup runs long after the last render and cannot read the state itself. */
  const tableStateRef = useRef<(key: string) => void>(rememberTable);
  useEffect(() => {
    tableStateRef.current = rememberTable;
  });

  // Filed away on the way out as well as on the way to another table: this is unmounted when the
  // sidebar loses its selection — picking another database does it — and what was on screen should
  // be there again when the table is opened later.
  useEffect(() => {
    return () => {
      tableStateRef.current(filterStateRef.current.key);
    };
  }, []);

  const [loading, setLoading] = useState(false);
  // Bumped by the reload action, by an insert and by a delete, to re-run the fetch below with the
  // request otherwise unchanged.
  const [reloadToken, setReloadToken] = useState(0);

  const [editingCell, setEditingCellState] = useState<EditingCell | null>(null);
  const [editValue, setEditValue] = useState("");
  const editingCellRef = useRef<EditingCell | null>(null);
  const pendingRowRef = useRef<{
    rowIndex: number;
    changes: Record<string, string | null>;
    original: Record<string, unknown>;
  } | null>(null);
  const editInputRef = useRef<HTMLInputElement | HTMLTextAreaElement | null>(null);
  const currentTableRef = useRef({ db: selectedDb, table: selectedTable });
  const thRefs = useRef<Map<string, HTMLTableCellElement>>(new Map());
  const [columnWidths, setColumnWidths] = useState<Record<string, number>>({});
  const tableRef = useRef<HTMLTableElement>(null);
  /** The widths every column is pinned to once the page is big enough to be windowed, and what they
   *  add up to — a fixed layout only obeys its columns when the table has a width of its own to
   *  divide between them. Null while the table is sizing itself, which is a small page or the first
   *  frame of a large one. */
  const [gridColumns, setGridColumns] = useState<{ widths: number[]; total: number } | null>(null);

  // Back to where the grid was left. `scrollPosRef` is the only record of that: hiding a pane
  // behind another tab is `display: none`, which takes its layout box away and the browser's
  // memory of the position with it, so coming back from Structure needs putting back just as
  // coming back from another table does. Re-applying a position the box is already at costs
  // nothing, which is what makes running this on every change of rows harmless.
  //
  // `rows` is in the deps because that is what says the DOM is showing this table: the rows put
  // back for a table come from an effect, so a commit earlier than theirs has nothing of the right
  // height to scroll within and the position would only be clamped away. A layout effect, not an
  // effect, so the scrollbar moves before the browser paints rather than after — and declared
  // above `useVirtualRows` so that the window of rows is chosen for where the grid has been put
  // back to, rather than for the top and corrected a frame later.
  useLayoutEffect(() => {
    const node = scrollRef.current;
    if (!node || !active) return;
    node.scrollLeft = scrollPosRef.current.left;
    node.scrollTop = scrollPosRef.current.top;
  }, [selectedDb, selectedTable, active, rows]);

  // Only the rows on screen are built into the DOM past a certain size. Everything the grid does —
  // selecting, editing, sorting, deleting — is indexed against `rows` rather than against what is
  // rendered, so all of it goes on meaning the same thing; the row being edited is held in the
  // window by name, since unmounting the input under the caret would lose the edit in progress.
  const virtual = rows.length >= VIRTUAL_FROM;
  // Named `view` rather than `window`, which this file needs for the browser's own.
  const view = useVirtualRows(scrollRef, {
    total: rows.length,
    rowHeight: ROW_HEIGHT,
    enabled: virtual,
    pinned: editingCell?.rowIndex ?? null,
  });

  /**
   * Where every column starts, in the very pixels the colgroup lays the table out in.
   *
   * The column being edited is widened there ({@link EDIT_COLUMN}) and so is widened here: worked
   * out from the unopened widths, every column to its right would be looked up one column short of
   * where it actually is for as long as an edit was open.
   *
   * Null until the widths are both known and this table's. `gridColumns` holds the outgoing table's
   * until the measuring effect runs on the incoming one, and a window read off another table's
   * widths is a window over columns that are not there.
   */
  const edges = useMemo(() => {
    if (!gridColumns || gridColumns.widths.length !== columns.length) return null;
    return columnEdges(
      gridColumns.widths.map((width, c) =>
        columns[c] === editingCell?.col ? Math.max(width, EDIT_COLUMN) : width
      )
    );
  }, [gridColumns, columns, editingCell?.col]);

  // The other half of the window, and the half a table of two hundred columns actually needs: a
  // grid windowed by rows alone still builds every column of every drawn row, which is where the
  // cells are. Fifty rows of two hundred columns is ten thousand of them whatever the rows did.
  //
  // Only ever on once the widths are measured, and for the same reason the row window insists on
  // them: the columns outside the window are stood in for by a cell spanning them, and a cell can
  // only stand in for what a fixed layout has already given a width to.
  const columnsVirtual = edges !== null && columns.length >= VIRTUAL_COLUMNS_FROM;
  const colView = useVirtualColumns(scrollRef, {
    edges: edges ?? NO_EDGES,
    enabled: columnsVirtual,
    // Held by index, where the row is held by position: both come to the same thing, which is that
    // the cell under the caret is never unmounted from under it.
    pinned: editingCell ? columns.indexOf(editingCell.col) : null,
  });

  /** Every move of the scrollbar, noted so that the position survives the grid being hidden,
   *  swapped for another table or unmounted — see {@link scrollPosRef}. A box with no layout is
   *  not believed: it reports the top, and that is the one answer that must not be filed. */
  function handleScroll() {
    const node = scrollRef.current;
    if (node && node.clientHeight > 0) {
      scrollPosRef.current = { top: node.scrollTop, left: node.scrollLeft };
    }
    if (virtual) view.onScroll();
    if (columnsVirtual) colView.onScroll();
  }

  // Selected rows are held as indices into the current page: selection is a
  // property of what is on screen, and every path that replaces the rows
  // (paging, reloading, switching table) clears it.
  const [selectedRows, setSelectedRows] = useState<Set<number>>(new Set());
  // Where a shift-click range starts. A ref rather than state because it is
  // only ever read from inside an event handler.
  const anchorRowRef = useRef<number | null>(null);
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

  function clearSelection() {
    setSelectedRows(new Set());
    anchorRowRef.current = null;
  }

  /** Hands the keyboard back to the grid. A dialog opened from here takes focus and leaves it on
   * the body when it closes, which would silence the grid's own shortcuts until the next click. */
  function focusGrid() {
    scrollRef.current?.focus({ preventScroll: true });
  }

  function setEditingCell(next: EditingCell | null) {
    editingCellRef.current = next;
    setEditingCellState(next);
  }

  const measureColumnWidths = useCallback(() => {
    // A tab that is up but not on show has no layout at all, so every header comes back 0 wide.
    // Recording that would leave the edit input pinned to nothing; what was measured while the tab
    // was last looked at stays until it can be measured again.
    if (!tableRef.current?.offsetWidth) return;
    const widths: Record<string, number> = {};
    for (const c of columns) {
      const el = thRefs.current.get(c);
      if (el) widths[c] = el.offsetWidth;
    }
    setColumnWidths(widths);
  }, [columns]);

  useEffect(() => {
    currentTableRef.current = { db: selectedDb, table: selectedTable };
  }, [selectedDb, selectedTable]);

  useEffect(() => {
    const el = editInputRef.current;
    if (!editingCell || !el) return;
    el.focus();
    el.select();
  }, [editingCell]);

  useEffect(() => {
    const el = editInputRef.current;
    if (!editingCell || !(el instanceof HTMLTextAreaElement)) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, [editValue, editingCell]);

  /**
   * What a header carries besides its own name: the sort chevron every column has, and the FK badge
   * a foreign key column has as well.
   *
   * Measured from the DOM rather than worked out from the stylesheet, because the stylesheet says
   * it in `em` of a font this file does not know the size of, and a badge's width is its text plus
   * its padding plus its border. Reading it costs one pass over the header row, which is a few
   * dozen elements — and it is stable however narrow the column gets, since both of these are
   * inline boxes with widths of their own rather than things that shrink to fit.
   */
  const headerExtras = useCallback((): number[] => {
    return columns.map((c) => {
      const th = thRefs.current.get(c);
      if (!th) return 0;
      let width = 0;
      const parts = th.querySelectorAll<HTMLElement>(`.${styles.sortIcon}, .${styles.fkBadge}`);
      for (const part of parts) {
        const style = getComputedStyle(part);
        width +=
          part.offsetWidth +
          (parseFloat(style.marginLeft) || 0) +
          (parseFloat(style.marginRight) || 0);
      }
      return Math.ceil(width);
    });
  }, [columns]);

  // What every column is pinned to while the page is windowed, measured over the whole page against
  // a canvas rather than by laying the rows out — see `virtualRows.ts`. Before paint, so the first
  // frame shown is already the pinned layout rather than a frame of self-sized columns followed by
  // one of pinned ones.
  useLayoutEffect(() => {
    const table = tableRef.current;
    if (!virtual || !table || columns.length === 0) {
      setGridColumns(null);
      return;
    }
    // Not through a hidden tab: the header extras below are read off the DOM and come back 0 there,
    // which is a header measured without the chevron and badge it has to hold. `active` is in the
    // deps so this runs again the moment the tab is looked at.
    if (!table.offsetWidth) return;
    const widths = measureColumns(
      table,
      columns,
      widestValues(rows, columns.length, (row, c) => row[columns[c]]),
      { chrome: CELL_CHROME, min: MIN_COLUMN, max: MAX_COLUMN },
      headerExtras()
    );
    setGridColumns(widths && { widths, total: widths.reduce((sum, w) => sum + w, 0) });
  }, [virtual, columns, rows, columnMeta, active, headerExtras]);

  // Re-measure each column's rendered width whenever the data or layout that
  // could affect it changes, so the edit input/textarea can be pinned to it
  // (auto table-layout would otherwise resize the column around the input's
  // own intrinsic size, causing a visible jump when entering edit mode).
  useLayoutEffect(() => {
    measureColumnWidths();
  }, [columns, rows, layoutWidth, gridColumns, active, measureColumnWidths]);

  useEffect(() => {
    function onResize() {
      measureColumnWidths();
    }
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, [measureColumnWidths]);

  /** Marks the rows a remembered table came back with as loaded — or not — for the fetch below.
   *
   *  What those rows were read with, so the fetch can tell that nothing is owed — but only if the
   *  bar is asking the same question the rows answer. The two are kept apart (the bar in
   *  `filterCache`, the rows here), and conditions applied while a read was still out are filed
   *  with no rows to match, so the pair has to be checked rather than assumed. By value: the arrays
   *  come from different places, and the fetch's own identity check is the one that has to be
   *  satisfied afterwards, which is why this render's array is what gets marked as loaded.
   *
   *  An effect event because it reads the applied filters and the reload token as they stand, and
   *  must not be what makes the table swap below run: a condition applied is a refetch, not a
   *  table put back from memory. */
  const markRememberedRows = useEffectEvent((cached: TableRequest) => {
    requestRef.current =
      JSON.stringify(cached.filters) === JSON.stringify(appliedFilters)
        ? { ...cached, filters: appliedFilters, reloadToken }
        : null;
  });

  // Runs when the selected table changes, and when the database's shape does (not on page/pageSize
  // changes). A table that has been here before is put back as it was left — its rows included, so
  // the trip costs nothing and shows what it showed; otherwise everything is cleared so the grid
  // stays hidden until the fetch below resolves.
  useEffect(() => {
    setEditingCell(null);
    pendingRowRef.current = null;
    // The page, the size, the sort and the applied filters have already been put back above, during
    // the render that saw the table change — see the note there.
    const key = tableCacheKey(selectedDb, selectedTable);
    const cached = rememberedTable(tableCache, key, schemaToken);
    // Where the grid goes back to once its rows are in the DOM — see the layout effect below. Set
    // here rather than left to the box itself, which is at the top whatever the last table did.
    scrollPosRef.current = { top: cached?.scrollTop ?? 0, left: cached?.scrollLeft ?? 0 };
    // A table that has been here before gets its own bar back; only a first visit is seeded with
    // the opening `id =` row.
    const remembered = rememberedFilters(filterCache, key, schemaToken);
    if (cached) {
      setColumns(cached.columns);
      setColumnMeta(cached.columnMeta);
      setPrimaryKey(cached.primaryKey);
      setAutoIncrementColumn(cached.autoIncrementColumn);
      setRows(cached.rows);
      setTotal(cached.total);
      setFilterRows(remembered?.rows ?? initialFilterRows(cached.columns, "eq"));
      filtersSeededForRef.current = key;
      markRememberedRows(cached.request);
    } else {
      setRows([]);
      setTotal(0);
      setColumns([]);
      setColumnMeta({});
      setPrimaryKey([]);
      setAutoIncrementColumn(null);
      setFilterRows(remembered?.rows ?? []);
      // Nothing is owed a seed once the bar has been restored, or the fetch would overwrite it.
      filtersSeededForRef.current = remembered ? key : null;
      // Nothing read for this table yet, so the fetch below is owed one — whatever the previous
      // table left marked here.
      requestRef.current = null;
    }
    // `schemaToken` is in here as well as the table: a change the app made to this database leaves
    // what is on screen describing a shape the server no longer has, and running this again is what
    // empties the grid and marks a read owed. The two caches are the workspace's, one each for its
    // life, so they never run it.
  }, [selectedDb, selectedTable, schemaToken, tableCache, filterCache]);

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
  }, [selectedDb, selectedTable, page, pageSize, sort, appliedFilters, reloadToken]);

  useEffect(() => {
    // Nothing is read for a tab nobody is looking at: the sidebar walked while the Structure tab is
    // up would otherwise send a page of rows and a count per table passed over.
    if (!active) return;
    // Built here rather than during render: it is this read's, and every field of it is already
    // something this effect runs again for.
    const request: TableRequest = {
      connectionId,
      db: selectedDb,
      table: selectedTable,
      page,
      pageSize,
      sort,
      filters: appliedFilters,
      reloadToken,
      schemaToken,
    };
    if (sameRequest(requestRef.current, request)) return;

    const db = selectedDb;
    const table = selectedTable;
    let cancelled = false;
    setLoading(true);
    api.tableData(connectionId, db, table, {
      page,
      pageSize,
      sortColumn: sort?.column ?? null,
      sortDesc: sort?.desc ?? false,
      filters: appliedFilters,
    })
      .then((result) => {
        if (cancelled) return;
        const key = tableCacheKey(db, table);
        // What was just read is what the next visit to this table opens on. Filed here rather than
        // only on the way out: a read that landed is worth keeping even if the app is closed on
        // this very table, and the way out has nothing to add beyond where the grid is scrolled to.
        fileTable(tableCache, key, {
          columns: result.columns,
          columnMeta: result.columnMeta,
          primaryKey: result.primaryKey,
          autoIncrementColumn: result.autoIncrementColumn,
          rows: result.rows,
          total: result.total,
          request,
          // Where the grid is scrolled to right now, which for a first read of a table and for a
          // reload is the top, and otherwise is wherever the user had got to within the page.
          scrollTop: scrollPosRef.current.top,
          scrollLeft: scrollPosRef.current.left,
        });
        // First look at this table's columns — the bar has been waiting for them to put its
        // opening `id` row together.
        if (filtersSeededForRef.current !== key) {
          setFilterRows(initialFilterRows(result.columns, "eq"));
          filtersSeededForRef.current = key;
        }
        setRows(result.rows);
        setColumns(result.columns);
        setColumnMeta(result.columnMeta);
        setPrimaryKey(result.primaryKey);
        setAutoIncrementColumn(result.autoIncrementColumn);
        setTotal(result.total);
        // Only a read that landed counts: a request that failed leaves nothing marked, so coming
        // back to the tab tries again rather than settling on an empty grid.
        requestRef.current = request;
      })
      .catch((e) => {
        // The table on screen is not the one this read was for. Its failure belongs to a table the
        // user has already left, and reported here it reads as this one's.
        if (cancelled) return;
        onError(errorMessage(t, e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [
    connectionId,
    selectedDb,
    selectedTable,
    page,
    pageSize,
    sort,
    appliedFilters,
    reloadToken,
    schemaToken,
    active,
    // One each for the life of the workspace — the engine's api, its caches, the setter its error
    // banner reads — so listing them costs no read.
    api,
    tableCache,
    onError,
    t,
  ]);

  function commitEditingCell() {
    const cell = editingCellRef.current;
    if (!cell) return;
    const { rowIndex, col } = cell;
    const row = rows[rowIndex];
    if (!row) return;
    const oldNorm = normalizeCellValue(row[col]);
    const newVal = editValue === "NULL" ? null : editValue;
    if (newVal === oldNorm) return;
    const prev = pendingRowRef.current;
    const sameRow = prev && prev.rowIndex === rowIndex;
    const changes = { ...(sameRow ? prev.changes : {}), [col]: newVal };
    const original = { ...(sameRow ? prev.original : {}) };
    if (!(col in original)) original[col] = row[col];
    pendingRowRef.current = { rowIndex, changes, original };
    // Optimistically reflect the edit in read mode immediately — the actual
    // UPDATE is still batched/deferred via pendingRowRef.
    setRows((prevRows) => prevRows.map((r, i) => (i === rowIndex ? { ...r, [col]: newVal } : r)));
  }

  async function flushPendingRow() {
    const pending = pendingRowRef.current;
    pendingRowRef.current = null;
    if (!pending || Object.keys(pending.changes).length === 0) return;
    const row = rows[pending.rowIndex];
    if (!row) return;
    const dbAtFlush = selectedDb;
    const tableAtFlush = selectedTable;
    const keyCols = primaryKey.length > 0 ? primaryKey : columns;
    const key: Record<string, string | null> = {};
    for (const c of keyCols) {
      // Key columns already optimistically edited must use their pre-edit
      // value — `row` reflects the optimistic update, not what's in MySQL yet.
      key[c] = c in pending.original ? normalizeCellValue(pending.original[c]) : normalizeCellValue(row[c]);
    }
    try {
      await api.updateRow(connectionId, dbAtFlush, tableAtFlush, pending.changes, key);
    } catch (e) {
      onError(errorMessage(t, e));
      // The value MySQL refused is on screen and may already be filed away: the grid is put away as
      // it stands the moment the user leaves the table, which is exactly what they do while a write
      // is still out. Nothing here can be rolled back in the cache — the entry was written from a
      // render that had the edit in it — so it goes, and the next visit reads the row as the server
      // has it. Free while the table is still up: the rows are held in state, and the entry is
      // written again from them on the way out.
      tableCache.delete(tableCacheKey(dbAtFlush, tableAtFlush));
      // On screen, only if the grid is still showing the table the write was for. Otherwise there is
      // nothing of this row rendered to put back, and the rows in hand belong to another table.
      if (currentTableRef.current.db !== dbAtFlush || currentTableRef.current.table !== tableAtFlush) {
        return;
      }
      setRows((prevRows) =>
        prevRows.map((r, i) => (i === pending.rowIndex ? { ...r, ...pending.original } : r)),
      );
    }
  }

  async function commitAndExit() {
    commitEditingCell();
    setEditingCell(null);
    await flushPendingRow();
  }

  /**
   * Backs out of the open editor and puts the row back the way the server has it — the text being
   * typed goes, and so do the cells of the same row already staged but not yet written.
   *
   * This is Escape, and it is also what `Ctrl+F` does on its way out of the grid. Leaving a cell is
   * normally taken as agreeing to what is in it, because the user pointed at somewhere else in the
   * table; a shortcut aimed at the filter bar says nothing about the edit, and writing it to the
   * server on the strength of that is a change nobody asked for.
   */
  function cancelEdit() {
    const cell = editingCellRef.current;
    if (!cell) return;
    const pending = pendingRowRef.current;
    if (pending && pending.rowIndex === cell.rowIndex) {
      pendingRowRef.current = null;
      setRows((prevRows) =>
        prevRows.map((r, i) => (i === pending.rowIndex ? { ...r, ...pending.original } : r)),
      );
    }
    setEditingCell(null);
  }

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

  /** The selected rows themselves, in the order they sit on screen — `selectedRows` holds their
   * indices, and a Set keeps no order of its own. */
  function selectedRowsInOrder(): Record<string, unknown>[] {
    return [...selectedRows]
      .sort((a, b) => a - b)
      .map((i) => rows[i])
      .filter((row): row is Record<string, unknown> => row !== undefined);
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

  async function moveEditTo(rowIndex: number, col: string) {
    // The one door into edit mode — a double-click and a Tab both come through here — so this is
    // the one place a read-only connection has to be turned away. Nothing is said about it: a cell
    // that simply does not open reads as a grid for looking at, and the reason is on the buttons
    // below, where someone who wants to change something is already looking.
    if (readOnly) return;
    const leavingRow = editingCellRef.current?.rowIndex;
    commitEditingCell();
    if (leavingRow !== undefined && leavingRow !== rowIndex) {
      await flushPendingRow();
    }
    setEditingCell({ rowIndex, col });
    setEditValue(displayValue(rows[rowIndex]?.[col]));
  }

  /** Extends, toggles or replaces the selection the way a list does: plain click selects the one
   * row, the platform's own modifier toggles it, shift takes everything between the anchor and it. */
  function selectRow(rowIndex: number, e: React.MouseEvent) {
    const toggle = hasPrimaryModifier(e);
    const anchor = anchorRowRef.current;
    if (e.shiftKey && anchor !== null) {
      const from = Math.min(anchor, rowIndex);
      const to = Math.max(anchor, rowIndex);
      setSelectedRows((prev) => {
        const next = toggle ? new Set(prev) : new Set<number>();
        for (let i = from; i <= to; i++) next.add(i);
        return next;
      });
      return;
    }
    anchorRowRef.current = rowIndex;
    if (toggle) {
      setSelectedRows((prev) => {
        const next = new Set(prev);
        if (!next.delete(rowIndex)) next.add(rowIndex);
        return next;
      });
      return;
    }
    setSelectedRows(new Set([rowIndex]));
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
    if (!selectedRows.has(rowIndex)) {
      setSelectedRows(new Set([rowIndex]));
      anchorRowRef.current = rowIndex;
    }
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
      setSelectedRows(new Set(rows.map((_, i) => i)));
      anchorRowRef.current = 0;
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

  function handleInputBlur(rowIndex: number, col: string) {
    const current = editingCellRef.current;
    if (!current || current.rowIndex !== rowIndex || current.col !== col) return;
    void commitAndExit();
  }

  function handleEditChange(e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) {
    setEditValue(e.target.value);
  }

  function handleEditKeyDown(e: React.KeyboardEvent<HTMLInputElement | HTMLTextAreaElement>) {
    const cell = editingCellRef.current;
    if (!cell) return;
    if (e.key === "Escape") {
      e.preventDefault();
      cancelEdit();
      return;
    }
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void commitAndExit();
      return;
    }
    if (e.key === "Tab") {
      e.preventDefault();
      const colIdx = columns.indexOf(cell.col);
      const nextColIdx = colIdx + (e.shiftKey ? -1 : 1);
      if (nextColIdx < 0 || nextColIdx >= columns.length) return;
      void moveEditTo(cell.rowIndex, columns[nextColIdx]);
      return;
    }
    if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      e.preventDefault();
      const nextRow = cell.rowIndex + (e.key === "ArrowUp" ? -1 : 1);
      if (nextRow < 0 || nextRow >= rows.length) return;
      void moveEditTo(nextRow, cell.col);
      return;
    }
    if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      const target = e.currentTarget;
      const atStart = target.selectionStart === 0 && target.selectionEnd === 0;
      const atEnd = target.selectionStart === target.value.length && target.selectionEnd === target.value.length;
      if (e.key === "ArrowLeft" && !atStart) return;
      if (e.key === "ArrowRight" && !atEnd) return;
      const colIdx = columns.indexOf(cell.col);
      const nextColIdx = colIdx + (e.key === "ArrowLeft" ? -1 : 1);
      if (nextColIdx < 0 || nextColIdx >= columns.length) return;
      e.preventDefault();
      void moveEditTo(cell.rowIndex, columns[nextColIdx]);
    }
  }

  /** How much wider the table is while a narrow column is open for editing — see
   *  {@link EDIT_COLUMN}. Zero the rest of the time, and zero for a column already wide enough. */
  const editingColumnExtra = (() => {
    if (!gridColumns || !editingCell) return 0;
    const index = columns.indexOf(editingCell.col);
    if (index < 0) return 0;
    return Math.max(0, EDIT_COLUMN - gridColumns.widths[index]);
  })();

  /** The columns each drawn row actually builds cells for. The whole set until they are worth
   *  windowing, and the header keeps the whole set either way — see the note over `<thead>`. */
  const drawnColumns = columnsVirtual ? columns.slice(colView.first, colView.last) : columns;

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
