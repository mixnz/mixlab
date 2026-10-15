import { describe, expect, it } from "vitest";
import type { SqlEditing } from "../../sql/dialect";
import type { SqlStructureColumn, SqlTableIndex } from "../../types";
import { indexMethod, isExpressionDefault, isFunctional } from "./shape";

const index = (indexType: string, names: (string | null)[] = ["id"]) =>
  ({ indexType, columns: names.map((name) => ({ name, prefixLength: null })) }) as unknown as SqlTableIndex;
const column = (defaultValue: string | null, defaultIsExpression: boolean) =>
  ({ defaultValue, defaultIsExpression }) as unknown as SqlStructureColumn;
const offers = (markExpressionDefaults: boolean) => ({ markExpressionDefaults }) as unknown as SqlEditing;

describe("indexMethod", () => {
  it("names a method this engine offers, whatever its case", () => {
    expect(indexMethod(index("btree"), ["BTREE", "HASH"])).toBe("BTREE");
  });
  it("says nothing of a kind that is not a method", () => {
    expect(indexMethod(index("FULLTEXT"), ["BTREE", "HASH"])).toBe("");
  });
});

describe("isExpressionDefault", () => {
  it("marks an expression on an engine that draws the distinction", () => {
    expect(isExpressionDefault(column("uuid()", true), offers(true))).toBe(true);
  });
  it("does not on an engine that does not", () => {
    expect(isExpressionDefault(column("uuid()", true), offers(false))).toBe(false);
  });
  it("does not when there is no default", () => {
    expect(isExpressionDefault(column(null, true), offers(true))).toBe(false);
  });
});

describe("isFunctional", () => {
  it("is an index with an expression among its columns", () => {
    expect(isFunctional(index("btree", ["a", null]))).toBe(true);
  });
  it("is not one over plain columns", () => {
    expect(isFunctional(index("btree", ["a"]))).toBe(false);
  });
});
