import { describe, expect, it } from "vitest";
import { overflowState, scrollStep } from "./overflow";

describe("overflowState", () => {
  it("says nothing is hidden when every tab fits", () => {
    expect(overflowState({ scrollLeft: 0, scrollWidth: 400, clientWidth: 400 }, true)).toEqual({
      overflowing: false,
      atStart: true,
      atEnd: true,
    });
  });

  it("marks the far left as the start, with tabs hidden to the right", () => {
    expect(overflowState({ scrollLeft: 0, scrollWidth: 900, clientWidth: 400 }, true)).toEqual({
      overflowing: true,
      atStart: true,
      atEnd: false,
    });
  });

  it("marks the far right as the end, with tabs hidden to the left", () => {
    expect(overflowState({ scrollLeft: 500, scrollWidth: 900, clientWidth: 400 }, true)).toEqual({
      overflowing: true,
      atStart: false,
      atEnd: true,
    });
  });

  it("sees tabs hidden both ways in the middle", () => {
    expect(overflowState({ scrollLeft: 250, scrollWidth: 900, clientWidth: 400 }, true)).toEqual({
      overflowing: true,
      atStart: false,
      atEnd: false,
    });
  });

  /* `scrollWidth` and `clientWidth` are rounded integers while `scrollLeft` is not, so a tab strip
     scrolled all the way often stands a fraction of a pixel short of the end. Calling that fraction
     "tabs still hidden" would light up an arrow that goes nowhere when clicked. */
  it("does not call a fraction of a pixel a hidden tab", () => {
    expect(overflowState({ scrollLeft: 499.6, scrollWidth: 900, clientWidth: 400 }, true).atEnd).toBe(true);
    expect(overflowState({ scrollLeft: 0.4, scrollWidth: 900, clientWidth: 400 }, true).atStart).toBe(true);
    expect(overflowState({ scrollLeft: 0, scrollWidth: 401, clientWidth: 400 }, true).overflowing).toBe(false);
  });

  /* A strip with no tabs holds only what trails them, `[+]`. Calling that overflow takes `[+]` out
     of the box to make room for the arrows, which empties the box, which then fits, which puts
     `[+]` back: a new state on every render, until React gives up. The arrows scroll tabs, and
     there are none to scroll. */
  it("never calls a strip with no tabs overflowing, however narrow its box", () => {
    expect(overflowState({ scrollLeft: 0, scrollWidth: 34, clientWidth: 13 }, false)).toEqual({
      overflowing: false,
      atStart: true,
      atEnd: true,
    });
  });
});

describe("scrollStep", () => {
  /* Nearly one frame, keeping a little back so the eye can still catch the place just left —
     jumping a whole frame per click leaves you not knowing where you are. */
  it("moves most of a screenful", () => {
    expect(scrollStep(400)).toBe(320);
  });

  it("still moves a strip narrower than a single tab", () => {
    expect(scrollStep(0)).toBe(1);
  });
});
