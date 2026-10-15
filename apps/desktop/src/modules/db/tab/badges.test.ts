import { describe, expect, it } from "vitest";
import { dbBadgeMarks } from "./badges";

describe("dbBadgeMarks", () => {
  it("marks nothing while the tab is not connected", () => {
    expect(dbBadgeMarks(undefined, false)).toEqual([]);
  });

  // `kind` is undefined exactly while there is no connection, and the lock belongs to a connection
  // — so a read-only saved row on a tab still showing the form marks nothing either.
  it("marks nothing when there is no connection even if read-only", () => {
    expect(dbBadgeMarks(undefined, true)).toEqual([]);
  });

  it("marks the engine once connected", () => {
    expect(dbBadgeMarks("mysql", false)).toEqual([{ type: "kind", kind: "mysql" }]);
  });

  it("puts the lock after the engine, never before it", () => {
    expect(dbBadgeMarks("postgres", true)).toEqual([
      { type: "kind", kind: "postgres" },
      { type: "readOnly" },
    ]);
  });
});
