import { describe, expect, it } from "vitest";
import { toCreateTable, toGoStruct, toTypeScript } from "./emit";
import { inferSchema } from "./infer";

const fields = (value: unknown) => inferSchema(value)!;

describe("toCreateTable", () => {
  it("maps types for the MySQL dialect", () => {
    const sql = toCreateTable(fields({ id: 1, ratio: 1.5, ok: true, meta: { a: 1 } }), {
      table: "users",
      dialect: "mysql",
    });
    expect(sql).toContain("`id` BIGINT NOT NULL");
    expect(sql).toContain("`ratio` DOUBLE NOT NULL");
    expect(sql).toContain("`ok` TINYINT(1) NOT NULL");
    expect(sql).toContain("`meta` JSON NOT NULL");
  });

  it("maps types for the PostgreSQL dialect", () => {
    const sql = toCreateTable(fields({ ok: true, meta: { a: 1 }, ratio: 1.5 }), {
      table: "users",
      dialect: "postgres",
    });
    expect(sql).toContain('"ok" BOOLEAN');
    expect(sql).toContain('"meta" JSONB');
    expect(sql).toContain('"ratio" DOUBLE PRECISION');
  });

  // A sample is only a sample; an INT column overflowing at the two-billionth record is something
  // to fix in production.
  it("uses BIGINT rather than INT", () => {
    expect(toCreateTable(fields({ n: 1 }), { table: "t", dialect: "mysql" })).toContain(
      "`n` BIGINT",
    );
    expect(toCreateTable(fields({ n: 1 }), { table: "t", dialect: "mysql" })).not.toContain(
      "`n` INT",
    );
  });

  it("renames columns to snake_case", () => {
    expect(toCreateTable(fields({ createdAt: "x" }), { table: "t", dialect: "mysql" })).toContain(
      "`created_at`",
    );
  });

  it("recognises time columns", () => {
    const sql = toCreateTable(fields({ at: "2026-08-28T00:00:00Z" }), {
      table: "t",
      dialect: "postgres",
    });
    expect(sql).toContain('"at" TIMESTAMPTZ');
  });

  it("drops NOT NULL for keys that are optional or have been seen null", () => {
    const sql = toCreateTable(fields([{ a: 1 }, { a: null, b: 2 }]), {
      table: "t",
      dialect: "mysql",
    });
    // `a` has been seen as null, `b` is absent from the first element — both are nullable.
    expect(sql).not.toContain("NOT NULL");
  });

  it("keeps NOT NULL for keys always present and never null", () => {
    expect(toCreateTable(fields([{ a: 1 }, { a: 2 }]), { table: "t", dialect: "mysql" })).toContain(
      "`a` BIGINT NOT NULL",
    );
  });

  it("prints TEXT when only null has been seen", () => {
    expect(toCreateTable(fields({ a: null }), { table: "t", dialect: "mysql" })).toContain(
      "`a` TEXT",
    );
  });

  // Flattening is a data modelling decision; the tool does not have enough information to make it
  // for the user.
  it("makes a nested object one JSON column rather than flattening it", () => {
    const sql = toCreateTable(fields({ user: { id: 1 } }), { table: "t", dialect: "mysql" });
    expect(sql).toContain("`user` JSON");
    expect(sql).not.toContain("user_id");
  });
});

describe("toTypeScript", () => {
  it("prints an interface with optional and null", () => {
    expect(toTypeScript(fields([{ a: 1 }, { a: null, b: "x" }]), "Row")).toBe(
      "export interface Row {\n  a: number | null;\n  b?: string;\n}",
    );
  });

  it("prints nested interfaces for child objects", () => {
    const code = toTypeScript(fields({ user: { id: 1 } }), "Row");
    expect(code).toContain("user: RowUser;");
    expect(code).toContain("export interface RowUser {");
  });

  it("prints an array of objects as T[]", () => {
    expect(toTypeScript(fields({ tags: [{ n: "a" }] }), "Row")).toContain("tags: RowTags[];");
  });

  it("quotes keys that are not valid identifiers", () => {
    expect(toTypeScript(fields({ "a-b": 1 }), "Row")).toContain('"a-b": number;');
  });

  it("prints unknown when only null or empty arrays have been seen", () => {
    expect(toTypeScript(fields({ a: null, b: [] }), "Row")).toContain("a: unknown");
  });
});

describe("toGoStruct", () => {
  it("prints PascalCase fields with a json tag keeping the original key", () => {
    expect(toGoStruct(fields({ created_at: "x" }), "Row")).toBe(
      'type Row struct {\n\tCreatedAt string `json:"created_at"`\n}',
    );
  });

  it("uses pointers for optional or nullable keys", () => {
    expect(toGoStruct(fields([{ a: 1 }, { a: null }]), "Row")).toContain("A *int64");
  });

  it("prints nested structs and slices", () => {
    const code = toGoStruct(fields({ user: { id: 1 }, tags: [{ n: "a" }] }), "Row");
    // `user` is required and never null, so it is a value rather than a pointer.
    expect(code).toContain("User RowUser");
    expect(code).toContain("Tags []RowTags");
    expect(code).toContain("type RowUser struct {");
  });

  it("uses a pointer for an optional child struct", () => {
    const code = toGoStruct(fields([{ user: { id: 1 } }, {}]), "Row");
    expect(code).toContain("User *RowUser");
  });
});
