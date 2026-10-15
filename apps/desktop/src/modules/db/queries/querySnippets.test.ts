import { describe, expect, it } from "vitest";
import { snippetFromSync, snippetToSync } from "./querySnippets";

describe("a query snippet in sync", () => {
  it("is known by its name, whatever its case", () => {
    expect(snippetToSync({ name: "Top Users", sql: "select 1" })).toEqual({
      id: "top users",
      data: { name: "Top Users", sql: "select 1" },
    });
  });

  it("comes back as it went", () => {
    expect(snippetFromSync(snippetToSync({ name: "a", sql: "b" }))).toEqual({ name: "a", sql: "b" });
  });

  it("refuses what it cannot read", () => {
    expect(snippetFromSync({ id: "x", data: { name: "x" } })).toBeNull();
  });
});
