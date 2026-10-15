import { useCallback, useEffect, useLayoutEffect, useState, type RefObject } from "react";

/**
 * A table that only ever holds the rows you can see.
 *
 * The machinery is shared because the fault it fixes is shared: a grid that builds every row into
 * the DOM is a grid that lays every row out again on every event that touches it — and a tab behind
 * another tab is `display: none`, which keeps no layout at all, so simply coming back to it pays
 * the whole cost. A few hundred rows is where that stops being instant.
 *
 * Two things have to be true for a window of rows to work, and both are the caller's to arrange
 * (see {@link useVirtualRows} for the first and {@link measureColumns} for the second):
 *
 * - **Every row is exactly `rowHeight` tall.** The rows outside the window are stood in for by a
 *   spacer of `count × rowHeight`, so any difference between that and a drawn row's real height
 *   makes the table's total height depend on where it is scrolled — the end of the set drifts away
 *   as you scroll towards it, by an amount that scales with the set. The height is therefore
 *   *stated* and pinned by the stylesheet, never measured: measuring can only make the error small,
 *   and small times ten thousand is not small.
 * - **Every column has a width of its own.** Only a few dozen rows are in the DOM at a time, and a
 *   table left to size itself sizes itself around those — every scroll would find new widest
 *   values and shuffle the columns sideways under the pointer.
 *
 * There are two ways to give a column a width, and the grids here use both, because they hold
 * different things:
 *
 * - **Measured** ({@link measureColumns}): the text is measured against a canvas and the widths are
 *   pinned with a colgroup and `table-layout: fixed`. Cheap over any number of rows — no elements,
 *   no layout — and right for a grid whose cells are text and nothing else. It has to be told about
 *   anything that is *not* text, which is why it takes `headerExtra`.
 * - **A sizer row**: a row of no height, holding each column's widest value, left in the table for
 *   the browser's own layout to find. Nothing is measured or pinned; the table sizes itself as it
 *   always did, around a row that is always present and always the widest. That is what suits a
 *   grid whose cells hold badges, buttons and several fonts — describing all of that to a canvas is
 *   how it gets it wrong, and the browser already knows.
 *
 * A grid wide enough needs the same treatment sideways ({@link useVirtualColumns}), and for the
 * same arithmetic: what a grid costs is rows times columns, so windowing one of the two leaves the
 * other free to put the cost back. Fifty rows of two hundred columns is ten thousand cells whether
 * or not the rows were windowed down to fifty.
 */

/** Rows kept either side of the visible window, so a flick of the wheel lands on rows that are
 *  already there rather than on a blank strip waiting for the next render. */
const OVERSCAN = 8;

/** The window's edges are rounded out to a multiple of this many rows.
 *
 * Without it every pixel of scrolling produces a different window, and the grid re-renders on every
 * frame of every scroll — which over a tall pane is fifty rows across twenty columns reconciled per
 * frame, and reads as a stutter. Rounded, the window only changes once per block of rows scrolled
 * past: most frames of a scroll now find the window they already have and do nothing at all. */
const BLOCK = 16;

/** How many rows are drawn before anything has been measured, which is the case for the first frame
 *  and for a grid in a tab nobody is looking at. Enough to fill a pane at any height it can be
 *  dragged to, so the rows are already there in the frame the tab comes back. */
const BLIND_ROWS = 48;

/** The most rows the window will ever hold, however tall the box claims to be.
 *
 * A backstop rather than a working limit: nothing in a real pane comes near it (a full-screen grid
 * is some seventy rows). It is here so that a viewport measured wrong — a box read mid-transition,
 * or before it has been laid out — costs a few hundred rows rather than every row in the set. */
const MAX_WINDOW = 400;

/** Which rows a scrolled box is over: the first one in it, and one past the last. */
export interface RowWindow {
  first: number;
  last: number;
}

/**
 * The rows worth rendering, given where the box is scrolled to and how tall it is.
 *
 * Its own function, and exported, because it is the whole of the arithmetic that decides what is on
 * screen — everything else here is DOM. An off-by-one is a strip of blank rows at one end of a
 * scroll, which is the sort of thing worth pinning down in a test rather than by dragging a
 * scrollbar.
 *
 * The overscan is added first and the block rounding second, so the block is slack on top of the
 * guaranteed margin rather than instead of it.
 */
export function rowWindow(
  total: number,
  rowHeight: number,
  scrollTop: number,
  span: number
): RowWindow {
  // A box of no height belongs to a tab nobody is looking at. It can say nothing about a window, so
  // a blind pane's worth is the answer — enough to fill the box whenever it does appear.
  if (rowHeight <= 0 || span <= 0) return { first: 0, last: Math.min(total, BLIND_ROWS) };
  const from = scrollTop / rowHeight - OVERSCAN;
  const to = (scrollTop + span) / rowHeight + OVERSCAN;
  const first = Math.max(0, Math.floor(from / BLOCK) * BLOCK);
  const last = Math.min(total, first + MAX_WINDOW, Math.ceil(to / BLOCK) * BLOCK);
  return { first, last: Math.max(first, last) };
}

/** A window widened to keep one particular row in it — the row being edited, which must not be
 *  unmounted from under the input it is holding. Serves the column window too, which is the same
 *  pair of numbers over the same kind of index and has the same one cell it cannot drop. */
function including(window: RowWindow, row: number | null | undefined, total: number): RowWindow {
  if (row === null || row === undefined || row < 0 || row >= total) return window;
  if (row >= window.first && row < window.last) return window;
  return { first: Math.min(window.first, row), last: Math.max(window.last, row + 1) };
}

export interface VirtualRows {
  /** The slice of rows to render: `rows.slice(first, last)`. */
  first: number;
  last: number;
  /** What the spacer above and below the drawn rows must be tall. Both are always rendered, even at
   *  zero: a spacer that came and went at the end of the set would hand `tr:last-child` to a real
   *  row and take its bottom rule with it, changing the table's height at exactly the point a
   *  scroll is trying to settle. */
  padTop: number;
  padBottom: number;
  /** Put on the scrolling box. */
  onScroll: () => void;
}

export interface VirtualRowsOptions {
  total: number;
  /** The height every row is pinned to by the stylesheet. Stated, never measured — see the note at
   *  the top of this file. */
  rowHeight: number;
  /** False for a grid small enough not to need any of this, which then renders every row and sizes
   *  its own columns exactly as it always did. */
  enabled: boolean;
  /** A row that has to stay in the window whatever the scroll says — the one being edited. */
  pinned?: number | null;
}

/**
 * The window of rows a box is scrolled over, kept up to date.
 *
 * The scroll offset is read off the box rather than kept in state, and the window is only written
 * when it actually changes: React drops a state write that changes nothing, so the frames between
 * one block and the next cost a division and no render at all.
 */
export function useVirtualRows(
  scroll: RefObject<HTMLElement | null>,
  { total, rowHeight, enabled, pinned = null }: VirtualRowsOptions
): VirtualRows {
  /** How tall the scrolling box is. The one thing here still asked of the DOM, and the one thing
   *  that can be wrong without doing harm: it decides how many rows are drawn and nothing about how
   *  tall the table is. Zero until measured, and zero again whenever the tab is put away. */
  const [viewport, setViewport] = useState(0);
  const [view, setView] = useState<RowWindow>({ first: 0, last: BLIND_ROWS });

  const syncWindow = useCallback(() => {
    const el = scroll.current;
    if (!el) return;
    const span = viewport > 0 ? viewport : rowHeight * BLIND_ROWS;
    const next = rowWindow(total, rowHeight, el.scrollTop, span);
    setView((current) =>
      current.first === next.first && current.last === next.last ? current : next
    );
  }, [scroll, viewport, rowHeight, total]);

  const remeasure = useCallback(() => {
    const el = scroll.current;
    if (!el) return;
    setViewport(el.clientHeight);
    syncWindow();
  }, [scroll, syncWindow]);

  // Before paint, so a pane that has just grown is filled in the frame it grew rather than a frame
  // later, and so the first frame of a new set is the right rows rather than the last set's.
  useLayoutEffect(() => {
    syncWindow();
  }, [syncWindow]);

  // An observer rather than a one-off, because the box's height changes without the rows changing:
  // a divider is dragged, a pane is lifted over the window, the window itself is resized — and,
  // most of all, the tab comes back from behind another one, which is when its height goes from
  // zero to real.
  useEffect(() => {
    const el = scroll.current;
    if (!el) return;
    const observer = new ResizeObserver(remeasure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [scroll, remeasure]);

  if (!enabled) {
    return { first: 0, last: total, padTop: 0, padBottom: 0, onScroll: syncWindow };
  }
  const held = including({ first: view.first, last: Math.min(view.last, total) }, pinned, total);
  return {
    first: held.first,
    last: held.last,
    padTop: held.first * rowHeight,
    padBottom: (total - held.last) * rowHeight,
    onScroll: syncWindow,
  };
}

/** Columns kept either side of the visible window. Fewer than the rows' {@link OVERSCAN} because a
 *  column costs far more than a row: one extra column is one extra cell in every drawn row, so the
 *  same generosity here would cost fifty times as much. Two is enough for what horizontal scrolling
 *  is — a deliberate drag, not a flick of the wheel. */
const COLUMN_OVERSCAN = 2;

/** The column window's edges are rounded out to a multiple of this many columns, for the reason the
 *  rows are rounded to {@link BLOCK}: without it every pixel of a sideways drag produces a different
 *  window, and every drawn row is reconciled again on every frame of it. */
const COLUMN_BLOCK = 4;

/** How many columns are drawn before the box has been measured — the first frame, and a grid in a
 *  tab nobody is looking at. Enough to fill a pane at any width it can be dragged to. */
const BLIND_COLUMNS = 24;

/** The most columns the window will ever hold, however wide the box claims to be. The sideways
 *  {@link MAX_WINDOW}: a backstop against a box measured mid-transition, not a working limit. */
const MAX_COLUMN_WINDOW = 120;

/** Which columns a sideways-scrolled box is over: the first one in it, and one past the last. */
export interface ColumnWindow {
  first: number;
  last: number;
}

/**
 * Where every column starts, and where the last one ends: `edges[c]` is the left edge of column `c`,
 * and `edges[count]` is what all of them add up to.
 *
 * This is the one thing columns cannot borrow from rows. A row's position is `index × rowHeight` and
 * so is arithmetic; a column's is the sum of every width before it, and so is a table. It is built
 * once per set of widths rather than per scroll, which is what lets {@link columnWindow} answer with
 * a search instead of a walk.
 */
export function columnEdges(widths: readonly number[]): number[] {
  const edges = new Array<number>(widths.length + 1);
  edges[0] = 0;
  for (let c = 0; c < widths.length; c++) edges[c + 1] = edges[c] + widths[c];
  return edges;
}

/** The column an offset falls in: the last one starting at or before it. A binary search because
 *  this is asked twice per scroll event, and a grid wide enough to be windowed is exactly the grid
 *  where a walk over the columns would be long. */
function columnAt(edges: readonly number[], x: number): number {
  let low = 0;
  let high = edges.length - 2;
  while (low < high) {
    const mid = (low + high + 1) >> 1;
    if (edges[mid] <= x) low = mid;
    else high = mid - 1;
  }
  return low;
}

/**
 * The columns worth rendering, given where the box is scrolled sideways to and how wide it is.
 *
 * Its own function and exported for the reason {@link rowWindow} is: it is the whole of the
 * arithmetic that decides what is built, and an off-by-one here is a strip of blank cells down one
 * edge of the grid.
 *
 * What it does *not* return is the pair of paddings its row equivalent does. The columns outside the
 * window are stood in for by a cell spanning them, and a spanning cell in a fixed layout takes its
 * width from the colgroup — which already holds every column, drawn or not. So the space is
 * accounted for by the same widths the window was worked out from, rather than by a second number
 * that could disagree with them.
 */
export function columnWindow(
  edges: readonly number[],
  scrollLeft: number,
  span: number
): ColumnWindow {
  const count = edges.length - 1;
  // A box of no width belongs to a tab nobody is looking at, exactly as a box of no height does.
  if (count <= 0 || span <= 0) {
    return { first: 0, last: Math.max(0, Math.min(count, BLIND_COLUMNS)) };
  }
  const from = columnAt(edges, scrollLeft) - COLUMN_OVERSCAN;
  // One past the column the right-hand edge lands in, since `last` is exclusive.
  const to = columnAt(edges, scrollLeft + span) + 1 + COLUMN_OVERSCAN;
  const first = Math.max(0, Math.floor(from / COLUMN_BLOCK) * COLUMN_BLOCK);
  const last = Math.min(
    count,
    first + MAX_COLUMN_WINDOW,
    Math.ceil(to / COLUMN_BLOCK) * COLUMN_BLOCK
  );
  return { first, last: Math.max(first, last) };
}

export interface VirtualColumns extends ColumnWindow {
  /** Put on the scrolling box, beside the one {@link useVirtualRows} hands back. */
  onScroll: () => void;
}

export interface VirtualColumnsOptions {
  /** Where every column starts — {@link columnEdges}. Held steady between renders by the caller,
   *  since it is what the window is worked out from and what every scroll is searched against. */
  edges: readonly number[];
  /** False for a grid narrow enough not to need any of this, which then renders every column. */
  enabled: boolean;
  /** A column that has to stay in the window whatever the scroll says — the one being edited. */
  pinned?: number | null;
}

/**
 * The window of columns a box is scrolled over, kept up to date.
 *
 * The twin of {@link useVirtualRows}, down to reading the offset off the box rather than holding it
 * in state and to writing the window only when it changes. It observes the same element and so
 * installs a second `ResizeObserver` on it: two observers on one box is a callback the browser was
 * already going to make, and the alternative — one hook returning both windows — would tie a grid
 * that only wants rows windowed to the machinery for columns.
 */
export function useVirtualColumns(
  scroll: RefObject<HTMLElement | null>,
  { edges, enabled, pinned = null }: VirtualColumnsOptions
): VirtualColumns {
  /** How wide the scrolling box is. Zero until measured, and zero again whenever the tab is put
   *  away — which is why a zero span is answered with a blind window rather than an empty one. */
  const [viewport, setViewport] = useState(0);
  const [view, setView] = useState<ColumnWindow>({ first: 0, last: BLIND_COLUMNS });

  const syncWindow = useCallback(() => {
    const el = scroll.current;
    if (!el) return;
    // A box of no width belongs to a tab nobody is looking at. It cannot say how much is on screen,
    // but it can still say where it is scrolled to — so a blind pane's worth is measured out from
    // there rather than from the left edge. Anchoring at zero instead would leave a grid put back
    // halfway across it drawing the columns at the far left, which is to say a frame of nothing but
    // the filler cell. A mean column is the width to guess with: the real ones are not yet known to
    // be worth asking about, since without a viewport there is nothing to compare them to.
    const count = edges.length - 1;
    const span =
      viewport > 0 ? viewport : (edges[Math.max(0, count)] / Math.max(1, count)) * BLIND_COLUMNS;
    const next = columnWindow(edges, el.scrollLeft, span);
    setView((current) =>
      current.first === next.first && current.last === next.last ? current : next
    );
  }, [scroll, edges, viewport]);

  const remeasure = useCallback(() => {
    const el = scroll.current;
    if (!el) return;
    setViewport(el.clientWidth);
    syncWindow();
  }, [scroll, syncWindow]);

  // Before paint, so the first frame of a set of widths is the right columns rather than the last
  // set's — a table swapped for a wider one would otherwise show a frame missing its right-hand end.
  useLayoutEffect(() => {
    syncWindow();
  }, [syncWindow]);

  useEffect(() => {
    const el = scroll.current;
    if (!el) return;
    const observer = new ResizeObserver(remeasure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [scroll, remeasure]);

  const count = Math.max(0, edges.length - 1);
  if (!enabled) return { first: 0, last: count, onScroll: syncWindow };
  const held = including({ first: view.first, last: Math.min(view.last, count) }, pinned, count);
  return { first: held.first, last: held.last, onScroll: syncWindow };
}

/** How many of a column's values are actually measured. The longest string is not always the widest
 *  one — a proportional font makes `WWW` wider than `iiiiii` — so the few longest by length are all
 *  measured and the widest of them wins. */
const WIDTH_SAMPLES = 3;

/** The length at which a column is certainly going to hit its ceiling whatever else is in it, and
 *  so has nothing left to learn from the rows below.
 *
 * This is what makes measuring every row affordable. A column of long text — a JSON document, a
 * description — is the expensive one to scan, and it is also the one that reaches its ceiling in
 * the first few rows; past that it is skipped entirely. Short columns are the cheap ones to scan
 * and are the reason to scan at all, since an `id` that gains a digit in row nine thousand has to
 * be measured in row nine thousand. */
const SATURATED_CHARS = 96;

/** A hair of slack on every measured column, for the difference between what a canvas says a string
 *  is worth and what the text renderer finally lays out — hinting, ligatures and letter-spacing all
 *  land on the layout side of that line. */
const WIDTH_SLACK = 4;

/** How a value reads in a grid: an absent one is spelled out as NULL, a structured one as the JSON
 *  it came from. Both grids draw their cells this way, so both measure them this way. */
export function displayValue(raw: unknown): string {
  if (raw === null || raw === undefined) return "NULL";
  return typeof raw === "object" ? JSON.stringify(raw) : String(raw);
}

/** The most characters a cell ever draws. A cell is a few hundred pixels at most, so anything past
 *  this is laid out for nobody to see. */
export const PREVIEW_CHARS = 500;

/** Whitespace of every kind, the zero-width ones included — `\s` alone misses those. */
const ANY_SPACE = /[\s​-‍]+/g;

/**
 * What a cell draws of `text` (a {@link displayValue}): one line, starting at the first visible
 * character, and no longer than {@link PREVIEW_CHARS}.
 *
 * Only the drawing — copying, editing and exporting read the value itself. A cell is `nowrap`,
 * which folds ordinary spaces and newlines but not a run of `&nbsp;` or ideographic spaces: text
 * pasted in from an HTML editor could lead with enough of them to fill the cell, leaving it blank
 * but for its ellipsis. And a long text column handed over whole — hundreds of kilobytes on one
 * line — is more than the webview will paint in a cell, on every row of the page.
 */
export function cellPreview(text: string): string {
  // Cut before folding, so a document-sized value is never scanned end to end. Twice the limit
  // leaves room for the runs the fold is about to shrink.
  const head = text.trimStart().slice(0, PREVIEW_CHARS * 2);
  const line = head.replace(ANY_SPACE, " ").trimStart();
  return line.length > PREVIEW_CHARS ? line.slice(0, PREVIEW_CHARS) : line;
}

/**
 * The few longest values in every column, taken from every row.
 *
 * Every row, and that is the point: a column sized from a sample is a column that fits until the
 * sample runs out — an `id` measured over the first two thousand rows of ten thousand is measured
 * one digit short, and every row past the sample shows it.
 *
 * What makes that affordable is what the scan refuses to do. It goes down the rows once for all the
 * columns rather than once per column; it never builds a string for a value that is already one,
 * which is most of a result set; and it drops a column the moment that column's answer is settled,
 * which is what keeps a document column from being serialised ten thousand times to learn something
 * the third row had already said.
 */
export function widestValues<Row>(
  rows: readonly Row[],
  columnCount: number,
  cell: (row: Row, column: number) => unknown
): string[][] {
  const picks: string[][] = Array.from({ length: columnCount }, () => []);
  /** Columns with nothing left to learn, and how many are still worth looking at. */
  const settled = new Array<boolean>(columnCount).fill(false);
  let open = columnCount;

  for (const row of rows) {
    if (open === 0) break;
    for (let c = 0; c < columnCount; c++) {
      if (settled[c]) continue;
      const raw = cell(row, c);
      // A string is its own rendering — and strings are what most cells hold, so this is the branch
      // that keeps the scan cheap. Everything else is built the way the cell will be drawn.
      const text = typeof raw === "string" ? raw : displayValue(raw);
      const list = picks[c];
      // Once is enough: a repeat measures nothing new, and the sizer rows key their cells by value.
      if (list.includes(text)) continue;
      if (list.length < WIDTH_SAMPLES) {
        list.push(text);
        list.sort((a, b) => b.length - a.length);
      } else if (text.length > list[WIDTH_SAMPLES - 1].length) {
        list[WIDTH_SAMPLES - 1] = text;
        list.sort((a, b) => b.length - a.length);
      }
      if (list[0].length >= SATURATED_CHARS) {
        settled[c] = true;
        open--;
      }
    }
  }
  return picks;
}

export interface ColumnSizing {
  /** What a cell spends on itself before any text goes in it: its padding and its rules. A width
   *  handed to a `<col>` is the whole column, chrome included, while what is measured below is the
   *  text alone — so every measured width has to carry this or the column comes out exactly this
   *  much too narrow. */
  chrome: number;
  /** The band a column is kept inside, chrome and all. The ceiling is where a cell stops being
   *  worth widening: past it the text is cut off with an ellipsis either way. */
  min: number;
  max: number;
}

/** The one canvas the app measures text with. Kept between calls: creating one per grid would be
 *  the expensive half of the measuring. */
let pen: CanvasRenderingContext2D | null = null;

function ruler(): CanvasRenderingContext2D | null {
  if (pen === null) pen = document.createElement("canvas").getContext("2d");
  return pen;
}

/**
 * An element drawn in the font the grid's values are: a data cell, not the grid.
 *
 * The two stopped agreeing when the app's text moved to a sans face and the cells kept a mono one —
 * a stylesheet sets `tbody td` apart from the table around it. Measured in the grid's own font, a
 * value came out narrower than it is drawn, and a column of timestamps ended every one of them in
 * an ellipsis. When no row is on screen yet a cell is put in for the reading and taken straight out
 * again, in the same task, so nothing is ever laid out or painted with it.
 */
function bodyFont(grid: HTMLElement): FontParts {
  const cell = grid.querySelector("tbody td");
  if (cell) return fontParts(getComputedStyle(cell));
  const probe = document.createElement("tbody");
  const td = probe.appendChild(document.createElement("tr")).appendChild(document.createElement("td"));
  grid.appendChild(probe);
  // Copied out while the probe is attached: a computed style is live, and empty once it is removed.
  const parts = fontParts(getComputedStyle(td));
  probe.remove();
  return parts;
}

type FontParts = Pick<CSSStyleDeclaration, "fontStyle" | "fontWeight" | "fontSize" | "fontFamily">;

function fontParts(style: FontParts): FontParts {
  const { fontStyle, fontWeight, fontSize, fontFamily } = style;
  return { fontStyle, fontWeight, fontSize, fontFamily };
}

/**
 * How wide each column has to be, worked out without laying a single row out.
 *
 * The text is measured against a canvas in the grid's own font rather than by the layout engine,
 * which is what makes it affordable over the whole set: no elements, no layout, one pass over the
 * shortlists. The header is measured in the weight the header is drawn in, which is not the body's.
 *
 * `null` when there is no canvas to measure with, which leaves the caller to size its own columns —
 * the grid is then wrong in the old way rather than in a new one.
 */
export function measureColumns(
  grid: HTMLElement,
  headers: readonly string[],
  samples: readonly string[][],
  { chrome, min, max }: ColumnSizing,
  /** What each header carries besides its own text — a sort chevron, a key badge. Added to the
   *  header rather than to the column, since it is the header that has to hold it: a column whose
   *  values are wider than its name has room for both already. Without it a short name beside a
   *  long one is a header cut off with an ellipsis while the values under it sit in clear space. */
  headerExtra?: readonly number[]
): number[] | null {
  const ink = ruler();
  if (!ink) return null;
  /** Assembled by hand rather than read from `style.font`: the shorthand comes back empty in some
   *  engines, and every part of it is available separately. */
  const font = (style: FontParts) =>
    `${style.fontStyle} ${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;

  const body = font(bodyFont(grid));
  // Taken from a header cell rather than assumed to be the body in bold. A `th` is bold by default
  // and a stylesheet may make it something else again, and a name measured a weight lighter than it
  // is drawn is a name with an ellipsis in a column wide enough for it.
  const headCell = grid.querySelector("thead th");
  const head = headCell ? font(fontParts(getComputedStyle(headCell))) : body;

  return headers.map((name, c) => {
    ink.font = head;
    let width = ink.measureText(name).width + (headerExtra?.[c] ?? 0);
    ink.font = body;
    for (const text of samples[c] ?? []) {
      width = Math.max(width, ink.measureText(text).width);
    }
    return Math.min(Math.max(Math.ceil(width) + WIDTH_SLACK + chrome, min), max);
  });
}

/** Everything a pinned-height, pinned-width grid needs handed to its stylesheet, since neither
 *  number can be written there: the row height is what this file's arithmetic depends on, and the
 *  table's own width is what a fixed column layout divides between the columns. */
export function gridStyle(rowHeight: number, width: number | null): React.CSSProperties {
  return {
    ...(width === null ? {} : { width }),
    "--row-h": `${rowHeight}px`,
  } as React.CSSProperties;
}
