import { describe, expect, it } from "vitest";
import { fitWidth } from "./sidebarWidth";

/* The measuring itself needs a DOM and is not tested here; this is the arithmetic it feeds, which
   is what decides whether a name fits or ends in an ellipsis. */
describe("fitWidth", () => {
  it("adds the row's padding and the bar's own to the measured text", () => {
    // 100 of text + 16 of row padding + 0 extra + 4 of sidebar padding.
    expect(fitWidth(100, 16, 0, 60, 500)).toBe(120);
  });

  it("rounds a fractional width up, since rounding it down clips the last letter", () => {
    expect(fitWidth(100.2, 0, 0, 0, 500)).toBe(105);
  });

  it("counts the fixed columns a row draws left of its name", () => {
    // Redis draws a chevron and a type badge there; measuring the name alone misses both.
    expect(fitWidth(100, 16, 66, 60, 500)).toBe(186);
  });

  it("never goes below the default — a sidebar of short names is not a thin sidebar", () => {
    expect(fitWidth(10, 0, 0, 200, 500)).toBe(200);
  });

  it("never goes past the maximum, however long the longest name is", () => {
    expect(fitWidth(9000, 16, 0, 200, 480)).toBe(480);
  });

  it("lets the maximum win over the default when the two disagree", () => {
    // Not reachable from the workspaces, whose defaults are all below their maximums, but the
    // order of the clamp is what says which one a caller that got them the wrong way round gets.
    expect(fitWidth(10, 0, 0, 900, 480)).toBe(480);
  });
});
