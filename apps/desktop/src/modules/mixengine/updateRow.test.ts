import { describe, expect, it } from "vitest";

import { updateRowState } from "./updateRow";

const update = { from: "8.4.24", to: "8.4.25" };
const job = { id: 7, kind: "runtime.upgrade", percent: 40, message: "unpacking" };

describe("updateRowState", () => {
  it("draws nothing for a version with no update and no job", () => {
    expect(updateRowState(undefined, undefined)).toEqual({ kind: "none" });
  });

  it("offers the update when one is known", () => {
    expect(updateRowState(update, undefined)).toEqual({ kind: "offer", update });
  });

  it("shows the running job instead of the button", () => {
    expect(updateRowState(update, job)).toEqual({
      kind: "running",
      percent: 40,
      message: "unpacking",
    });
  });

  // After the job finished the list is read again; until then the job still speaks for the row.
  it("keeps showing a running job even once the update has left the list", () => {
    expect(updateRowState(undefined, job).kind).toBe("running");
  });
});
