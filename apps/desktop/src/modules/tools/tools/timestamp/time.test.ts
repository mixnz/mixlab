import { describe, expect, it } from "vitest";
import { detectUnit, toInstant, toOutputs } from "./time";

describe("detectUnit", () => {
  it("guesses by the number of digits", () => {
    expect(detectUnit("1756339200")).toBe("seconds");
    expect(detectUnit("1756339200000")).toBe("millis");
    expect(detectUnit("1756339200000000")).toBe("micros");
  });

  it("returns null for anything that is not a run of digits", () => {
    expect(detectUnit("2026-08-28")).toBeNull();
    expect(detectUnit("")).toBeNull();
    expect(detectUnit("12.5")).toBeNull();
  });
});

describe("toInstant", () => {
  it("converts every unit to milliseconds", () => {
    expect(toInstant("1756339200")).toBe(1756339200000);
    expect(toInstant("1756339200000")).toBe(1756339200000);
    expect(toInstant("1756339200000000")).toBe(1756339200000);
  });

  it("accepts ISO 8601 strings too", () => {
    expect(toInstant("2026-08-28T00:00:00Z")).toBe(Date.parse("2026-08-28T00:00:00Z"));
  });

  it("returns null for anything unreadable", () => {
    expect(toInstant("không phải giờ")).toBeNull();
  });
});

describe("toOutputs", () => {
  const ms = Date.parse("2026-08-28T00:00:00Z");

  it("prints ISO UTC and unix in both units", () => {
    const out = toOutputs(ms, "UTC", ms);
    expect(out.isoUtc).toBe("2026-08-28T00:00:00.000Z");
    expect(out.unixSeconds).toBe("1787875200");
    expect(out.unixMillis).toBe("1787875200000");
  });

  it("states the relative distance from the `now` passed in", () => {
    expect(toOutputs(ms, "UTC", ms + 3 * 86_400_000).relative).toBe("3 days ago");
    expect(toOutputs(ms, "UTC", ms - 2 * 3_600_000).relative).toBe("in 2 hours");
    expect(toOutputs(ms, "UTC", ms).relative).toBe("now");
  });
});
