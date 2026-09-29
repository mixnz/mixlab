import { describe, expect, it } from "vitest";
import { toCreateTable, toGoStruct, toTypeScript } from "./emit";
import { inferSchema } from "./infer";

const fields = (value: unknown) => inferSchema(value)!;

describe("toCreateTable", () => {
  it("ánh xạ kiểu theo dialect MySQL", () => {
    const sql = toCreateTable(fields({ id: 1, ratio: 1.5, ok: true, meta: { a: 1 } }), {
      table: "users",
      dialect: "mysql",
    });
    expect(sql).toContain("`id` BIGINT NOT NULL");
    expect(sql).toContain("`ratio` DOUBLE NOT NULL");
    expect(sql).toContain("`ok` TINYINT(1) NOT NULL");
    expect(sql).toContain("`meta` JSON NOT NULL");
  });

  it("ánh xạ kiểu theo dialect PostgreSQL", () => {
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
  it("dùng BIGINT chứ không INT", () => {
    expect(toCreateTable(fields({ n: 1 }), { table: "t", dialect: "mysql" })).toContain(
      "`n` BIGINT",
    );
    expect(toCreateTable(fields({ n: 1 }), { table: "t", dialect: "mysql" })).not.toContain(
      "`n` INT",
    );
  });

  it("đổi tên cột sang snake_case", () => {
    expect(toCreateTable(fields({ createdAt: "x" }), { table: "t", dialect: "mysql" })).toContain(
      "`created_at`",
    );
  });

  it("nhận ra cột thời gian", () => {
    const sql = toCreateTable(fields({ at: "2026-08-28T00:00:00Z" }), {
      table: "t",
      dialect: "postgres",
    });
    expect(sql).toContain('"at" TIMESTAMPTZ');
  });

  it("bỏ NOT NULL cho khoá optional hoặc từng thấy null", () => {
    const sql = toCreateTable(fields([{ a: 1 }, { a: null, b: 2 }]), {
      table: "t",
      dialect: "mysql",
    });
    // `a` has been seen as null, `b` is absent from the first element — both are nullable.
    expect(sql).not.toContain("NOT NULL");
  });

  it("giữ NOT NULL cho khoá luôn có mặt và không bao giờ null", () => {
    expect(toCreateTable(fields([{ a: 1 }, { a: 2 }]), { table: "t", dialect: "mysql" })).toContain(
      "`a` BIGINT NOT NULL",
    );
  });

  it("in TEXT khi chỉ thấy null", () => {
    expect(toCreateTable(fields({ a: null }), { table: "t", dialect: "mysql" })).toContain(
      "`a` TEXT",
    );
  });

  // Flattening is a data modelling decision; the tool does not have enough information to make it
  // for the user.
  it("để object lồng nhau thành một cột JSON chứ không trải phẳng", () => {
    const sql = toCreateTable(fields({ user: { id: 1 } }), { table: "t", dialect: "mysql" });
    expect(sql).toContain("`user` JSON");
    expect(sql).not.toContain("user_id");
  });
});

describe("toTypeScript", () => {
  it("in interface với optional và null", () => {
    expect(toTypeScript(fields([{ a: 1 }, { a: null, b: "x" }]), "Row")).toBe(
      "export interface Row {\n  a: number | null;\n  b?: string;\n}",
    );
  });

  it("in interface lồng cho object con", () => {
    const code = toTypeScript(fields({ user: { id: 1 } }), "Row");
    expect(code).toContain("user: RowUser;");
    expect(code).toContain("export interface RowUser {");
  });

  it("in mảng object thành T[]", () => {
    expect(toTypeScript(fields({ tags: [{ n: "a" }] }), "Row")).toContain("tags: RowTags[];");
  });

  it("bọc ngoặc kép khoá không phải định danh hợp lệ", () => {
    expect(toTypeScript(fields({ "a-b": 1 }), "Row")).toContain('"a-b": number;');
  });

  it("in unknown khi chỉ thấy null hoặc mảng rỗng", () => {
    expect(toTypeScript(fields({ a: null, b: [] }), "Row")).toContain("a: unknown");
  });
});

describe("toGoStruct", () => {
  it("in field PascalCase kèm tag json giữ khoá gốc", () => {
    expect(toGoStruct(fields({ created_at: "x" }), "Row")).toBe(
      'type Row struct {\n\tCreatedAt string `json:"created_at"`\n}',
    );
  });

  it("dùng con trỏ cho khoá optional hoặc nullable", () => {
    expect(toGoStruct(fields([{ a: 1 }, { a: null }]), "Row")).toContain("A *int64");
  });

  it("in struct lồng và slice", () => {
    const code = toGoStruct(fields({ user: { id: 1 }, tags: [{ n: "a" }] }), "Row");
    // `user` is required and never null, so it is a value rather than a pointer.
    expect(code).toContain("User RowUser");
    expect(code).toContain("Tags []RowTags");
    expect(code).toContain("type RowUser struct {");
  });

  it("dùng con trỏ cho struct con optional", () => {
    const code = toGoStruct(fields([{ user: { id: 1 } }, {}]), "Row");
    expect(code).toContain("User *RowUser");
  });
});
