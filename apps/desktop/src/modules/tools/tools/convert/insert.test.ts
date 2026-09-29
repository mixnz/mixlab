import { describe, expect, it } from "vitest";
import { toInsert } from "./insert";

const mysql = { table: "users", dialect: "mysql" as const, multiRow: false };
const postgres = { table: "users", dialect: "postgres" as const, multiRow: false };

describe("toInsert", () => {
  it("in một câu lệnh cho mỗi dòng", () => {
    expect(toInsert([{ id: 1, name: "An" }], mysql)).toBe(
      "INSERT INTO `users` (`id`, `name`) VALUES (1, 'An');",
    );
  });

  it("gộp nhiều dòng vào một câu lệnh khi được yêu cầu", () => {
    expect(toInsert([{ id: 1 }, { id: 2 }], { ...mysql, multiRow: true })).toBe(
      "INSERT INTO `users` (`id`) VALUES\n  (1),\n  (2);",
    );
  });

  it("bọc định danh theo dialect", () => {
    expect(toInsert([{ id: 1 }], postgres)).toBe('INSERT INTO "users" ("id") VALUES (1);');
  });

  it("nhân đôi dấu nháy đơn ở cả hai dialect", () => {
    expect(toInsert([{ a: "it's" }], mysql)).toContain("'it''s'");
    expect(toInsert([{ a: "it's" }], postgres)).toContain("'it''s'");
  });

  // MySQL treats `\` as an escape character; PostgreSQL does not. Get this wrong and the statement
  // still runs, writing something else into the DB.
  it("chỉ nhân đôi dấu gạch chéo ngược cho MySQL", () => {
    expect(toInsert([{ p: "C:\\new" }], mysql)).toContain("'C:\\\\new'");
    expect(toInsert([{ p: "C:\\new" }], postgres)).toContain("'C:\\new'");
  });

  it("in null, boolean và số không có ngoặc", () => {
    expect(toInsert([{ a: null, b: true, c: 1.5 }], mysql)).toContain("(NULL, TRUE, 1.5)");
  });

  it("in object và mảng thành chuỗi JSON", () => {
    expect(toInsert([{ tags: ["a", "b"] }], postgres)).toContain(`'["a","b"]'`);
  });

  it("lấy hợp các cột và bù NULL cho khoá thiếu", () => {
    expect(toInsert([{ a: 1 }, { b: 2 }], { ...mysql, multiRow: true })).toBe(
      "INSERT INTO `users` (`a`, `b`) VALUES\n  (1, NULL),\n  (NULL, 2);",
    );
  });

  it("bọc được tên bảng có ký tự đặc biệt", () => {
    expect(toInsert([{ a: 1 }], { ...mysql, table: "a`b" })).toContain("INSERT INTO `a``b`");
  });

  it("trả chuỗi rỗng khi không có dòng nào", () => {
    expect(toInsert([], mysql)).toBe("");
  });
});
