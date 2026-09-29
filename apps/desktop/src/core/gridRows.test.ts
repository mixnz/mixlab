import { describe, expect, it } from "vitest";

/**
 * Grid cells must not have vertical padding.
 *
 * The row height is a number declared in TypeScript (`ROW_HEIGHT`), and the spacer standing in for
 * the rows outside the frame is exactly `count × ROW_HEIGHT` tall — see `virtualRows.ts`. CSS takes
 * that number back through `--row-h` and sets `height`, so the two sides cannot drift apart
 * *unless* a rule adds height that `height` does not swallow: vertical padding, or a vertical
 * border.
 *
 * Someone has already hit exactly that while trying to "tighten the rows" with padding: the rows
 * swelled from 33px to 38.8px and the number of visible rows **dropped**. No test went red, and the
 * only symptom was the bottom of the page drifting away when scrolling close to it.
 *
 * Vitest here runs in a node environment with no DOM, so the real height cannot be measured. What
 * can be measured is the stylesheet, and this is the only condition on the stylesheet that could
 * break that invariant.
 *
 * `?raw` only returns real content because `vite.config.ts` turns on `test.css` — see the note in
 * `shell/fonts.test.ts`. The first case below guards that.
 */

/** Every innermost `selector { … }` block. */
function blocks(css: string) {
  return [...css.matchAll(/([^{}]*)\{([^{}]*)\}/g)].map(([, selector, body]) => {
    const lines = selector.trim().split("\n");
    return { selector: lines[lines.length - 1].trim(), body };
  });
}

const sheets = Object.entries(
  import.meta.glob("../modules/db/**/*.css", {
    query: "?raw",
    import: "default",
    eager: true,
  }) as Record<string, string>,
).map(([path, css]) => ({ path, css }));

describe("grid rows", () => {
  it("reads the stylesheet, not an empty blob", () => {
    expect(sheets.length).toBeGreaterThan(10);
    expect(sheets.every(({ css }) => css.length > 0)).toBe(true);
  });

  it("no rule gives grid cells vertical padding", () => {
    const offenders = sheets.flatMap(({ path, css }) =>
      blocks(css)
        .filter(({ selector }) => /\.gridRows\b[^{]*\btd\b/.test(selector))
        .filter(({ body }) => /padding(-top|-bottom)?:\s*(?![0;\s])/.test(body))
        .map(({ selector }) => `${path}: ${selector}`),
    );
    expect(offenders).toEqual([]);
  });

  it("every row-pinning rule takes its height from --row-h, not a written number", () => {
    const offenders = sheets.flatMap(({ path, css }) =>
      blocks(css)
        .filter(({ selector }) => /\.gridRows\b[^{]*\btd\b/.test(selector))
        .filter(({ body }) => /height:/.test(body) && !/height:\s*var\(--row-h\)/.test(body))
        .map(({ selector }) => `${path}: ${selector}`),
    );
    expect(offenders).toEqual([]);
  });
});
