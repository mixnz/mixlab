import { describe, expect, it } from "vitest";
import {
  cellPreview,
  columnEdges,
  columnWindow,
  PREVIEW_CHARS,
  rowWindow,
  widestValues,
} from "./virtualRows";

/** The grid's own constants, repeated rather than imported: a test that reads the number it is
 *  checking against cannot notice that number changing. */
const OVERSCAN = 8;
const BLOCK = 16;
const BLIND_ROWS = 48;
const COLUMN_OVERSCAN = 2;
const COLUMN_BLOCK = 4;
const BLIND_COLUMNS = 24;

/** A row's height and a pane's height, in the shape the grid actually sees them: 25px rows and a
 *  400px pane is sixteen rows on screen. */
const ROW = 25;
const PANE = 400;

/** The height the two real grids pin their rows to, near enough — 31px in the results grid, 33px in
 *  the data grid. The sweeps below run at this rather than at the round number above, so they are
 *  answering the question at the sizes the app actually uses. */
const ROW_HEIGHT = 31;

/** How the sweeps read a cell out of a row. Both grids hand `widestValues` an accessor, since one
 *  of them holds positional rows and the other holds rows keyed by column name. */
const cell = (row: unknown[], column: number) => row[column];

describe("rowWindow", () => {
  it("opens on a block of rows at the top of the set", () => {
    // Sixteen rows on screen, the overscan below them, rounded out to the next block.
    expect(rowWindow(500, ROW, 0, PANE)).toEqual({ first: 0, last: 32 });
  });

  it("keeps whole blocks either side once it is scrolled into the set", () => {
    // Scrolled to row 100: rows 100–115 are on screen, 92–124 with the overscan, 80–128 as blocks.
    expect(rowWindow(500, ROW, 100 * ROW, PANE)).toEqual({ first: 80, last: 128 });
  });

  it("does not move while the scroll stays inside a block", () => {
    // The point of the rounding: most frames of a scroll find the window they already have, and a
    // window that has not changed is a pane that does not re-render.
    const settled = rowWindow(500, ROW, 100 * ROW, PANE);
    for (const rows of [0.5, 1, 1.5, 2]) {
      expect(rowWindow(500, ROW, (100 + rows) * ROW, PANE)).toEqual(settled);
    }
  });

  it("always covers the rows on screen, wherever it is scrolled to", () => {
    // Swept rather than sampled: a window that misses its viewport by a row is a blank strip at one
    // edge, and it only shows up at some offsets.
    for (let scrollTop = 0; scrollTop <= 500 * ROW - PANE; scrollTop += 7) {
      const { first, last } = rowWindow(500, ROW, scrollTop, PANE);
      expect(first * ROW).toBeLessThanOrEqual(scrollTop);
      expect(last * ROW).toBeGreaterThanOrEqual(scrollTop + PANE);
    }
  });

  it("keeps at least the overscan beyond each edge, except at the ends of the set", () => {
    const { first, last } = rowWindow(500, ROW, 100 * ROW, PANE);
    expect(100 - first).toBeGreaterThanOrEqual(OVERSCAN);
    expect(last - 116).toBeGreaterThanOrEqual(OVERSCAN);
  });

  it("stops at the end of the set rather than past it", () => {
    expect(rowWindow(500, ROW, 500 * ROW - PANE, PANE).last).toBe(500);
  });

  it("starts and ends on block boundaries while there is set on either side", () => {
    const { first, last } = rowWindow(500, ROW, 200 * ROW, PANE);
    expect(first % BLOCK).toBe(0);
    expect(last % BLOCK).toBe(0);
  });

  it("reaches the last row of a very large set", () => {
    // Ten thousand rows, scrolled to the bottom: the window has to end on the last row, or the end
    // of the set sits below a scrollbar that has already stopped.
    expect(rowWindow(10_000, ROW, 10_000 * ROW - PANE, PANE).last).toBe(10_000);
  });

  it("never hands over more rows than the backstop allows", () => {
    // A viewport measured wrong — a box read mid-transition, or before it has been laid out — must
    // cost a few hundred rows rather than every row in the set.
    const { first, last } = rowWindow(10_000, ROW, 0, 1_000_000);
    expect(last - first).toBeLessThanOrEqual(400);
  });

  it("renders a blind pane's worth when the box has no height", () => {
    // A tab nobody is looking at measures zero, and a window worked out from that would be empty —
    // leaving nothing to measure a row's height from when the tab does come back.
    expect(rowWindow(500, ROW, 0, 0)).toEqual({ first: 0, last: BLIND_ROWS });
  });

  it("renders a blind pane's worth when a row has never been measured", () => {
    expect(rowWindow(500, 0, 0, PANE)).toEqual({ first: 0, last: BLIND_ROWS });
  });

  it("never claims more rows than there are", () => {
    expect(rowWindow(20, 0, 0, 0)).toEqual({ first: 0, last: 20 });
    expect(rowWindow(20, ROW, 0, PANE).last).toBe(20);
  });

  it("never ends before it starts", () => {
    // Scrolled past the end — which a box can be, briefly, when a shorter result replaces a longer
    // one under it.
    const { first, last } = rowWindow(20, ROW, 5000, PANE);
    expect(last).toBeGreaterThanOrEqual(first);
  });
});

/**
 * The property that has to hold for every set, not for the one that was tried.
 *
 * Each of these sizes broke differently while this grid was being built — a hundred rows, a hundred
 * and fifty, five hundred, ten thousand — and each break looked like its own bug. They were one
 * bug: a window that did not quite cover the viewport, or a height that depended on where the box
 * was scrolled. So the check is a sweep rather than an example, and the sizes are a range rather
 * than the one in the bug report.
 */
describe("rowWindow over every size of result", () => {
  const sizes = [60, 61, 100, 150, 499, 500, 501, 1000, 5000, 10_000];
  const panes = [96, 240, 400, 900];

  it("covers the viewport wherever it is scrolled, at every size", () => {
    // Gathered rather than asserted one at a time: this is tens of thousands of positions, and an
    // assertion at each of them costs more than the sweep. A failure reports where it was.
    const uncovered: string[] = [];
    // A prime-ish step so the positions land all over the rows rather than on the same offset
    // within each of them.
    const step = ROW_HEIGHT * 3 + 7;
    for (const total of sizes) {
      for (const pane of panes) {
        const content = total * ROW_HEIGHT;
        const bottom = Math.max(0, content - pane);
        for (let at = 0; at <= bottom + step; at += step) {
          const scrollTop = Math.min(at, bottom);
          const { first, last } = rowWindow(total, ROW_HEIGHT, scrollTop, pane);
          const covered =
            first * ROW_HEIGHT <= scrollTop &&
            last * ROW_HEIGHT >= Math.min(scrollTop + pane, content) &&
            last <= total;
          if (!covered) uncovered.push(`${total} rows, ${pane}px pane, at ${scrollTop}`);
        }
      }
    }
    expect(uncovered).toEqual([]);
  });

  it("ends on the last row when the box is at the bottom, at every size", () => {
    for (const total of sizes) {
      for (const pane of panes) {
        const bottom = Math.max(0, total * ROW_HEIGHT - pane);
        expect(rowWindow(total, ROW_HEIGHT, bottom, pane).last).toBe(total);
      }
    }
  });

  it("accounts for every row exactly once, at every size", () => {
    // The rows above, the rows drawn and the rows below are the whole set — which is what makes the
    // table `total × ROW_HEIGHT` tall at every scroll position rather than at some of them.
    const wrong: string[] = [];
    for (const total of sizes) {
      for (let at = 0; at <= total * ROW_HEIGHT; at += 397) {
        const { first, last } = rowWindow(total, ROW_HEIGHT, at, 400);
        if (first + (last - first) + (total - last) !== total) {
          wrong.push(`${total} rows at ${at}`);
        }
      }
    }
    expect(wrong).toEqual([]);
  });
});

/** A column of the round width the sweeps below reason in, and a pane eight of them wide. */
const COLUMN = 100;
const PANE_WIDTH = 800;

/** Two hundred columns, which is the table this was written for. */
const WIDE = 200;

/** Widths that are all different, inside the band the grid measures columns into (48–320). The
 *  point of columns is that they are *not* evenly spaced — a window that only works over equal
 *  widths is a window that works in the test and not in the app — so everything below that can be
 *  asked of uneven widths is. */
const uneven = (count: number) =>
  columnEdges(Array.from({ length: count }, (_, c) => 48 + ((c * 37) % 273)));

const even = (count: number) => columnEdges(Array.from({ length: count }, () => COLUMN));

describe("columnEdges", () => {
  it("gives each column its start and the set its total", () => {
    expect(columnEdges([100, 48, 320])).toEqual([0, 100, 148, 468]);
  });

  it("copes with no columns at all", () => {
    expect(columnEdges([])).toEqual([0]);
  });
});

describe("columnWindow", () => {
  it("opens on a block of columns at the left of the table", () => {
    // Eight columns on screen, the overscan beyond them, rounded out to the next block.
    expect(columnWindow(even(WIDE), 0, PANE_WIDTH)).toEqual({ first: 0, last: 12 });
  });

  it("keeps whole blocks either side once it is scrolled across", () => {
    // At column 100: 100–108 are on screen, 98–111 with the overscan, 96–112 as blocks.
    expect(columnWindow(even(WIDE), 100 * COLUMN, PANE_WIDTH)).toEqual({ first: 96, last: 112 });
  });

  it("does not move while the scroll stays inside a block", () => {
    // The point of the rounding: most frames of a sideways drag find the window they already have,
    // and a window that has not changed is fifty rows that do not re-render.
    const edges = even(WIDE);
    const settled = columnWindow(edges, 100 * COLUMN, PANE_WIDTH);
    for (const px of [1, 20, 50, 99]) {
      expect(columnWindow(edges, 100 * COLUMN + px, PANE_WIDTH)).toEqual(settled);
    }
  });

  it("starts and ends on block boundaries while there is table on either side", () => {
    const { first, last } = columnWindow(uneven(WIDE), 4000, PANE_WIDTH);
    expect(first % COLUMN_BLOCK).toBe(0);
    expect(last % COLUMN_BLOCK).toBe(0);
  });

  it("keeps at least the overscan beyond each edge, over widths that are all different", () => {
    const edges = uneven(WIDE);
    const at = 4000;
    const { first, last } = columnWindow(edges, at, PANE_WIDTH);
    // Which columns are actually on screen, found without the arithmetic under test.
    const onScreen = [...Array(WIDE).keys()].filter(
      (c) => edges[c + 1] > at && edges[c] < at + PANE_WIDTH
    );
    expect(onScreen[0] - first).toBeGreaterThanOrEqual(COLUMN_OVERSCAN);
    expect(last - (onScreen[onScreen.length - 1] + 1)).toBeGreaterThanOrEqual(COLUMN_OVERSCAN);
  });

  it("reaches the last column at the right-hand end", () => {
    const edges = uneven(WIDE);
    const total = edges[WIDE];
    expect(columnWindow(edges, total - PANE_WIDTH, PANE_WIDTH).last).toBe(WIDE);
  });

  it("renders a blind pane's worth when the box has no width", () => {
    // A tab nobody is looking at measures zero, and a window worked out from that would be empty.
    expect(columnWindow(even(WIDE), 0, 0)).toEqual({ first: 0, last: BLIND_COLUMNS });
  });

  it("never hands over more columns than the backstop allows", () => {
    const { first, last } = columnWindow(even(500), 0, 1_000_000);
    expect(last - first).toBeLessThanOrEqual(120);
  });

  it("never claims more columns than there are", () => {
    expect(columnWindow(even(6), 0, 0)).toEqual({ first: 0, last: 6 });
    expect(columnWindow(even(6), 0, PANE_WIDTH).last).toBe(6);
    expect(columnWindow(columnEdges([]), 0, PANE_WIDTH)).toEqual({ first: 0, last: 0 });
  });

  it("never ends before it starts", () => {
    // Scrolled past the right-hand end, which a box can be for a frame when a narrower table
    // replaces a wider one under it.
    const { first, last } = columnWindow(even(6), 50_000, PANE_WIDTH);
    expect(last).toBeGreaterThanOrEqual(first);
  });
});

/**
 * The properties that have to hold at every position, not at the one that was tried.
 *
 * Uneven widths are what make this worth sweeping rather than sampling: the column a given offset
 * falls in is a search rather than a division, and a search that is one out shows up at some offsets
 * and not at others.
 */
describe("columnWindow across the whole table", () => {
  const counts = [40, 41, 120, 199, 200, 500];
  const panes = [320, 800, 1600];

  it("covers the columns on screen wherever it is scrolled", () => {
    const uncovered: string[] = [];
    for (const count of counts) {
      const edges = uneven(count);
      const width = edges[count];
      for (const pane of panes) {
        const rightmost = Math.max(0, width - pane);
        // A prime-ish step, so the positions land all over the columns rather than on the same
        // offset within each of them.
        for (let at = 0; at <= rightmost + 137; at += 137) {
          const scrollLeft = Math.min(at, rightmost);
          const { first, last } = columnWindow(edges, scrollLeft, pane);
          const covered =
            edges[first] <= scrollLeft &&
            edges[last] >= Math.min(scrollLeft + pane, width) &&
            last <= count;
          if (!covered) uncovered.push(`${count} columns, ${pane}px pane, at ${scrollLeft}`);
        }
      }
    }
    expect(uncovered).toEqual([]);
  });

  it("accounts for every column exactly once", () => {
    // What makes the two spanning cells and the drawn ones add up to the table: the columns before
    // the window, the columns in it and the columns after it are the whole set. Miscount and the
    // rows are a cell short of the colgroup, which a fixed layout answers by shifting every column
    // sideways.
    const wrong: string[] = [];
    for (const count of counts) {
      const edges = uneven(count);
      for (let at = 0; at <= edges[count]; at += 311) {
        const { first, last } = columnWindow(edges, at, 800);
        if (first + (last - first) + (count - last) !== count) wrong.push(`${count} at ${at}`);
      }
    }
    expect(wrong).toEqual([]);
  });
});

describe("widestValues", () => {
  /** The longest value the shortlist offers for one column — what its width ends up measured from. */
  const longest = (picks: string[][], column: number) => picks[column][0];

  it("finds the longest value wherever in the set it is", () => {
    // The case that sent this back for a second try: ten thousand rows of an id that only reaches
    // five digits at the very end. A column sized from anything less than all of them is a column
    // one digit too narrow for its last nine thousand rows.
    const rows = Array.from({ length: 10_000 }, (_, i) => [i + 1]);
    expect(longest(widestValues(rows, 1, cell), 0)).toBe("10000");
  });

  it("keeps a shortlist rather than a single winner", () => {
    // Length is only an approximation of width, so the widest of the longest few is what wins —
    // which means there have to be a few.
    const rows = [["aaaa"], ["bbb"], ["cc"], ["d"]];
    expect(widestValues(rows, 1, cell)[0]).toEqual(["aaaa", "bbb", "cc"]);
  });

  it("keeps each value once", () => {
    // A sizer row keys its cells by value, so a repeat is two React children with one key — and a
    // repeat says nothing about width the first one did not. A wide table's Type column is the
    // case: `text`, `text`, `text`.
    const rows = [["text"], ["text"], ["integer"], ["text"]];
    expect(widestValues(rows, 1, cell)[0]).toEqual(["integer", "text"]);
  });

  it("measures each column against its own values", () => {
    const rows = [
      ["a", "xxxxx"],
      ["bbbb", "y"],
    ];
    const picks = widestValues(rows, 2, cell);
    expect(longest(picks, 0)).toBe("bbbb");
    expect(longest(picks, 1)).toBe("xxxxx");
  });

  it("renders a value the way the cell will", () => {
    const rows = [[null, { a: 1 }, 12345]];
    const picks = widestValues(rows, 3, cell);
    expect(longest(picks, 0)).toBe("NULL");
    expect(longest(picks, 1)).toBe('{"a":1}');
    expect(longest(picks, 2)).toBe("12345");
  });

  it("stops looking at a column that has already hit the ceiling", () => {
    // The long column is settled by its first row and skipped from then on — which is what keeps a
    // JSON column from being serialised once per row — while the short one beside it goes on being
    // measured to the last row, because that is where its longest value is.
    const long = "x".repeat(200);
    const rows = [
      [long, "1"],
      [`${long}${long}`, "22"],
      [`${long}${long}${long}`, "333"],
    ];
    const picks = widestValues(rows, 2, cell);
    expect(longest(picks, 0)).toBe(long);
    expect(longest(picks, 1)).toBe("333");
  });

  it("copes with a result of no rows at all", () => {
    expect(widestValues([], 2, cell)).toEqual([[], []]);
  });
});

describe("cellPreview", () => {
  it("draws text past a lead of spaces nowrap does not fold", () => {
    // What an HTML editor leaves behind: non-breaking and ideographic spaces, a zero-width one.
    expect(cellPreview("  　​\n\t<p>Hello</p>")).toBe("<p>Hello</p>");
  });

  it("puts a multi-line value on one line", () => {
    expect(cellPreview("line one\r\n  line two")).toBe("line one line two");
  });

  it("draws no more of a long value than a cell can show", () => {
    const preview = cellPreview("x".repeat(500_000));
    expect(preview).toHaveLength(PREVIEW_CHARS);
  });

  it("leaves a short value and NULL as they are", () => {
    expect(cellPreview("42")).toBe("42");
    expect(cellPreview("NULL")).toBe("NULL");
    expect(cellPreview("")).toBe("");
  });
});
