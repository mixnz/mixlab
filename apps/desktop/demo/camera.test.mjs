import { describe, expect, it } from "vitest";
import { clockCheck, inFrame, normalize, sheetTimes, toVideoTime } from "./camera.mjs";
import { geometryFor, isWholeGeometry } from "./geometry.mjs";

const rect = (x, y, w, h) => ({ x, y, w, h });

describe("geometry", () => {
  it("is the stills' 1440×900 at 2 when nothing is scaled", () => {
    expect(geometryFor()).toEqual({ viewport: { width: 1440, height: 900 }, scale: 2 });
  });

  it("shrinks the viewport and raises the device scale by the same factor", () => {
    expect(geometryFor(1.25)).toEqual({ viewport: { width: 1152, height: 720 }, scale: 2.5 });
  });

  it("accepts only scales from 1 to 2 that give whole pixels", () => {
    expect(isWholeGeometry(1.25)).toBe(true);
    expect(isWholeGeometry(1)).toBe(true);
    expect(isWholeGeometry(1.3)).toBe(false);
    expect(isWholeGeometry(0.8)).toBe(false);
    expect(isWholeGeometry(2.5)).toBe(false);
  });
});

describe("toVideoTime", () => {
  it("measures from the first frame", () => {
    expect(toVideoTime(1000.75, 1000, 12)).toBeCloseTo(0.75);
  });

  it("clamps a sample taken as recording stopped to the end, and one before it to 0", () => {
    expect(toVideoTime(1013, 1000, 12)).toBe(12);
    expect(toVideoTime(999.9, 1000, 12)).toBe(0);
  });
});

describe("clockCheck", () => {
  it("passes delays of a few milliseconds and reports their median", () => {
    const frames = [0.006, 0.013, 0.028].map((d, i) => ({ t: 100 + i, arrival: 100 + i + d }));
    expect(clockCheck(frames)).toEqual({ ok: true, least: 0.006, median: 0.013, worst: 0.028 });
  });

  it("allows a millisecond of disagreement between the two processes' clocks", () => {
    expect(clockCheck([{ t: 100, arrival: 99.999 }, { t: 101, arrival: 101.01 }]).ok).toBe(true);
  });

  it("fails a frame that arrived before it was swapped", () => {
    expect(clockCheck([{ t: 100, arrival: 99.99 }]).ok).toBe(false);
  });

  it("fails a delay of a second or more", () => {
    expect(clockCheck([{ t: 100, arrival: 101 }]).ok).toBe(false);
  });
});

describe("normalize", () => {
  it("divides by the viewport, so both geometries give the same share", () => {
    expect(normalize(rect(144, 90, 720, 450), { width: 1440, height: 900 })).toEqual(rect(0.1, 0.1, 0.5, 0.5));
    expect(normalize(rect(115.2, 72, 576, 360), { width: 1152, height: 720 })).toEqual(rect(0.1, 0.1, 0.5, 0.5));
  });

  it("clamps a region that runs off the frame", () => {
    expect(normalize(rect(-48, 600, 400, 400), { width: 1152, height: 720 })).toEqual(
      rect(0, 0.833, 0.306, 0.167),
    );
  });
});

describe("sheetTimes", () => {
  it("takes one frame a second, never at or past the end", () => {
    expect(sheetTimes(3.5)).toEqual([0, 1, 2, 3]);
    expect(sheetTimes(3)).toEqual([0, 1, 2]);
    expect(sheetTimes(0.4)).toEqual([0]);
  });
});

describe("inFrame", () => {
  const viewport = { width: 1152, height: 720 };

  it("accepts a box wholly inside, to the edge", () => {
    expect(inFrame({ x: 0, y: 0, width: 1152, height: 720 }, viewport)).toBe(true);
  });

  it("refuses one that crosses an edge", () => {
    expect(inFrame({ x: 1100, y: 10, width: 60, height: 20 }, viewport)).toBe(false);
    expect(inFrame({ x: 10, y: -3, width: 60, height: 20 }, viewport)).toBe(false);
  });
});
