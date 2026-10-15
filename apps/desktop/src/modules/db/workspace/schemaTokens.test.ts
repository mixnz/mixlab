import { describe, expect, it } from "vitest";
import { cacheKey, databasePrefix } from "./schemaTokens";

/* The hook itself needs a renderer this repository does not set up. What is checked here is the
   pair of functions the hook and the panes both build keys with — and above all that they agree,
   because the failure when they do not is silent: `forgetDatabase` walks the caches, matches
   nothing, and leaves every stale page of rows exactly where it was. */
describe("cacheKey", () => {
  it("files an item under its database", () => {
    expect(cacheKey("shop", "orders")).toBe("shop :: orders");
  });

  it("produces a key the database's own prefix matches", () => {
    expect(cacheKey("shop", "orders").startsWith(databasePrefix("shop"))).toBe(true);
  });

  it("does not match a database whose name this one merely starts with", () => {
    // `shop` and `shopping` are two databases; emptying one must not empty the other.
    expect(cacheKey("shopping", "orders").startsWith(databasePrefix("shop"))).toBe(false);
  });

  it("keeps two databases' items apart under the same item name", () => {
    expect(cacheKey("shop", "orders")).not.toBe(cacheKey("archive", "orders"));
  });
});
