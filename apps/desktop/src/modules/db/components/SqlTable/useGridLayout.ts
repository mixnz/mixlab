import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { RefObject } from "react";
import {
  columnEdges,
  measureColumns,
  useVirtualColumns,
  useVirtualRows,
  widestValues,
} from "../../../../core/virtualRows";
import type { SqlColumnMeta } from "../../types";
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
export const ROW_HEIGHT = 29;

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
export const EDIT_COLUMN = 240;

interface Options {
  scrollRef: RefObject<HTMLDivElement | null>;
  scrollPosRef: RefObject<{ top: number; left: number }>;
  rows: Record<string, unknown>[];
  columns: string[];
  columnMeta: Record<string, SqlColumnMeta>;
  editingCell: { rowIndex: number; col: string } | null;
  active: boolean;
  layoutWidth: number | undefined;
  selectedDb: string;
  selectedTable: string;
}

/** How the grid is laid out: the widths it measures and pins, the windows of rows and columns it
 *  draws, and the scroll position it keeps. */
export default function useGridLayout({
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
}: Options) {
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
  }, [selectedDb, selectedTable, active, rows, scrollRef, scrollPosRef]);

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

  return {
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
  };
}
