import { describe, expect, it } from "vitest";
import { scrollTopFor } from "./savedListScroll";

/* A scroll box 300 high, a sticky header 40 high, rows 50 high. Round numbers so you can read off
   at once which row is where, not to resemble a real box. */
const view = { scrollTop: 0, height: 300 };
const HEADER = 40;

describe("scrollTopFor", () => {
  it("does not scroll a row already in full view below the header", () => {
    expect(scrollTopFor({ top: 60, height: 50 }, view, HEADER)).toBeNull();
  });

  /* The visible region starts below the sticky header, not at the box's top edge — otherwise what
     gets scrolled to is exactly what is covered. */
  it("counts a row behind the sticky header as out of view", () => {
    expect(scrollTopFor({ top: 120, height: 50 }, { scrollTop: 100, height: 300 }, HEADER)).toBe(80);
  });

  it("brings a row above the viewport down to just under the header", () => {
    expect(scrollTopFor({ top: 500, height: 50 }, { scrollTop: 600, height: 300 }, HEADER)).toBe(460);
  });

  it("brings a row below the viewport up to its bottom edge", () => {
    expect(scrollTopFor({ top: 900, height: 50 }, view, HEADER)).toBe(650);
  });

  /* A row taller than the frame — a long name wrapping, plus a read-only badge — has its top as the
     part worth seeing, so it aligns to the top edge. */
  it("aligns a row taller than the viewport to the top", () => {
    expect(scrollTopFor({ top: 900, height: 400 }, view, HEADER)).toBe(860);
  });

  /* The first row of the list sits partly behind the sticky header and there is nowhere left to
     scroll up to. Scrolling up to a negative number is scrolling nowhere, so do not make the
     browser do it. */
  it("gives up rather than scrolling past the top of the list", () => {
    expect(scrollTopFor({ top: 10, height: 50 }, view, HEADER)).toBeNull();
  });
});
