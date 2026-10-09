import { describe, expect, it } from "vitest";
import { isEmulated } from "./emulated";

describe("isEmulated", () => {
  it("marks a release the daemon says is emulated", () => {
    expect(isEmulated({ execution: "emulated" })).toBe(true);
  });

  it("leaves a native release alone", () => {
    expect(isEmulated({ execution: "native" })).toBe(false);
  });

  // ADR 0019: an older daemon omits the member, and silence is not a claim of emulation.
  it("reads a daemon that reports nothing as making no claim", () => {
    expect(isEmulated({})).toBe(false);
    expect(isEmulated({ execution: null })).toBe(false);
  });
});
