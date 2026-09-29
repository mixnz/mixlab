import { describe, expect, it } from "vitest";
import type { PathReport } from "@mixengine/api";

import { pathOutcome, shouldOfferPathInstall } from "./pathState";

/** A report, with just enough fields to answer the question being asked. */
function report(on_path: boolean, changed: boolean[] = []): PathReport {
  return {
    directory: "C:\\Users\\me\\MixEngine\\bin",
    on_path,
    places: changed.map((flag, index) => ({ name: `place-${index}`, present: on_path, changed: flag })),
    commands: [],
  };
}

describe("shouldOfferPathInstall", () => {
  it("offers while the directory is not on the PATH", () => {
    expect(shouldOfferPathInstall(report(false))).toBe(true);
  });

  it("does not offer once it is", () => {
    expect(shouldOfferPathInstall(report(true))).toBe(false);
  });

  /* `null` is "not finished reading" or "read failed", not "not installed": inviting on it would
     flash the card in front of someone who installed long ago, every time the tab opens. */
  it("does not offer before the report has arrived", () => {
    expect(shouldOfferPathInstall(null)).toBe(false);
  });
});

describe("pathOutcome", () => {
  it("says a new terminal is needed when this call wrote somewhere", () => {
    expect(pathOutcome(report(true, [false, true]))).toBe("changed");
  });

  /* With no place `changed`, the daemon wrote nothing at all — saying "open a new terminal" then
     asks for something that changes nothing. */
  it("says nothing changed when every place already agreed", () => {
    expect(pathOutcome(report(true, [false, false]))).toBe("unchanged");
  });
});
