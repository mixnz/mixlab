import { describe, expect, it } from "vitest";
import { toInsert } from "./insert";

const mysql = { table: "users", dialect: "mysql" as const, multiRow: false };
const postgres = { table: "users", dialect: "postgres" as const, multiRow: false };

describe("toInsert", () => {
  it("prints one statement per row", () => {
    expect(toInsert([{ id: 1, name: "An" }], mysql)).toBe(
      "INSERT INTO `users` (`id`, `name`) VALUES (1, 'An');",
    );
  });

  it("merges several rows into one statement when asked", () => {
    expect(toInsert([{ id: 1 }, { id: 2 }], { ...mysql, multiRow: true })).toBe(
      "INSERT INTO `users` (`id`) VALUES\n  (1),\n  (2);",
    );
  });

  it("quotes identifiers per dialect", () => {
    expect(toInsert([{ id: 1 }], postgres)).toBe('INSERT INTO "users" ("id") VALUES (1);');
  });

  it("doubles single quotes in both dialects", () => {
    expect(toInsert([{ a: "it's" }], mysql)).toContain("'it''s'");
    expect(toInsert([{ a: "it's" }], postgres)).toContain("'it''s'");
  });

  // MySQL treats `\` as an escape character; PostgreSQL does not. Get this wrong and the statement
  // still runs, writing something else into the DB.
  it("only doubles backslashes for MySQL", () => {
    expect(toInsert([{ p: "C:\\new" }], mysql)).toContain("'C:\\\\new'");
    expect(toInsert([{ p: "C:\\new" }], postgres)).toContain("'C:\\new'");
  });

  it("prints null, booleans and numbers unquoted", () => {
    expect(toInsert([{ a: null, b: true, c: 1.5 }], mysql)).toContain("(NULL, TRUE, 1.5)");
  });

  it("prints objects and arrays as JSON strings", () => {
    expect(toInsert([{ tags: ["a", "b"] }], postgres)).toContain(`'["a","b"]'`);
  });

  it("takes the union of columns and fills NULL for missing keys", () => {
    expect(toInsert([{ a: 1 }, { b: 2 }], { ...mysql, multiRow: true })).toBe(
      "INSERT INTO `users` (`a`, `b`) VALUES\n  (1, NULL),\n  (NULL, 2);",
    );
  });

  it("quotes table names with special characters", () => {
    expect(toInsert([{ a: 1 }], { ...mysql, table: "a`b" })).toContain("INSERT INTO `a``b`");
  });

  it("returns an empty string when there are no rows", () => {
    expect(toInsert([], mysql)).toBe("");
  });
});
