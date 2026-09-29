import { describe, expect, it } from "vitest";
import { generate, inferFields, slugify, type FieldSpec } from "./fake";

/** A deterministic LCG — the same seed always gives the same sequence, so the test does not depend
 *  on `Math.random`. */
function seededRnd(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
    return s / 4294967296;
  };
}

describe("generate", () => {
  it("sinh đúng số dòng, cột theo đúng thứ tự field", () => {
    const fields: FieldSpec[] = [
      { name: "b", kind: "word" },
      { name: "a", kind: "word" },
    ];
    const rows = generate(fields, 3, seededRnd(1));
    expect(rows).toHaveLength(3);
    expect(Object.keys(rows[0])).toEqual(["b", "a"]);
  });

  it("integer nằm trong khoảng min/max", () => {
    const rows = generate([{ name: "age", kind: "integer", min: 18, max: 65 }], 200, seededRnd(7));
    for (const row of rows) {
      expect(row.age).toBeGreaterThanOrEqual(18);
      expect(row.age).toBeLessThanOrEqual(65);
      expect(Number.isInteger(row.age)).toBe(true);
    }
  });

  it("float nằm trong khoảng và làm tròn đúng số chữ số thập phân", () => {
    const rows = generate(
      [{ name: "price", kind: "float", min: 0, max: 100, decimals: 1 }],
      50,
      seededRnd(3),
    );
    for (const row of rows) {
      const value = row.price as number;
      expect(value).toBeGreaterThanOrEqual(0);
      expect(value).toBeLessThanOrEqual(100);
      expect(Number(value.toFixed(1))).toBe(value);
    }
  });

  it("boolean chỉ sinh true hoặc false", () => {
    const rows = generate([{ name: "active", kind: "boolean" }], 50, seededRnd(11));
    for (const row of rows) expect(typeof row.active).toBe("boolean");
  });

  it("constant luôn trả đúng giá trị đã đặt", () => {
    const rows = generate([{ name: "status", kind: "constant", value: "seed" }], 10, seededRnd(2));
    expect(rows.every((row) => row.status === "seed")).toBe(true);
  });

  it("uuid có đúng hình dạng UUID v4", () => {
    const rows = generate([{ name: "id", kind: "uuid" }], 30, seededRnd(5));
    const re = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
    for (const row of rows) expect(row.id as string).toMatch(re);
  });

  it("email có dạng địa chỉ hợp lệ, đúng domain đã định nghĩa", () => {
    const rows = generate([{ name: "email", kind: "email" }], 30, seededRnd(9));
    for (const row of rows) {
      expect(row.email as string).toMatch(/^[a-z0-9.]+@(example\.com|mail\.test|sample\.dev|demo\.io)$/);
    }
  });

  it("phone theo locale vi có 10 chữ số bắt đầu bằng 0", () => {
    const rows = generate([{ name: "phone", kind: "phone", locale: "vi" }], 30, seededRnd(13));
    for (const row of rows) expect(row.phone as string).toMatch(/^0\d{9}$/);
  });

  it("fullName theo locale vi lấy từ vốn tên tiếng Việt", () => {
    const rows = generate([{ name: "name", kind: "fullName", locale: "vi" }], 30, seededRnd(17));
    // Accented VN names — the English name pool has no such characters, so this proves the right
    // pool was used.
    expect(rows.some((row) => /[ăâđêôơư]/i.test(row.name as string))).toBe(true);
  });

  it("fullName mặc định không có tên đệm — luôn đúng hai từ", () => {
    const rows = generate([{ name: "name", kind: "fullName", locale: "vi" }], 30, seededRnd(23));
    for (const row of rows) expect((row.name as string).split(" ")).toHaveLength(2);
  });

  it("fullName kèm includeMiddle thì chèn tên đệm ở giữa", () => {
    const rowsVi = generate(
      [{ name: "name", kind: "fullName", locale: "vi", includeMiddle: true }],
      30,
      seededRnd(29),
    );
    for (const row of rowsVi) expect((row.name as string).split(" ")).toHaveLength(3);

    const rowsEn = generate(
      [{ name: "name", kind: "fullName", locale: "en", includeMiddle: true }],
      30,
      seededRnd(31),
    );
    for (const row of rowsEn) expect((row.name as string).split(" ")).toHaveLength(3);
  });

  it("middleName đứng riêng lấy đúng vốn theo locale", () => {
    const rows = generate([{ name: "middle", kind: "middleName", locale: "vi" }], 30, seededRnd(37));
    // The Vietnamese middle-name pool does not mix with the given-name pool — "Long"/"Nam" are not
    // in it.
    expect(rows.every((row) => !["Long", "Nam"].includes(row.middle as string))).toBe(true);
  });

  it("fullName cùng dòng với firstName/lastName thì là cùng một người", () => {
    const fields: FieldSpec[] = [
      { name: "full", kind: "fullName", locale: "vi" },
      { name: "first", kind: "firstName", locale: "vi" },
      { name: "last", kind: "lastName", locale: "vi" },
    ];
    const rows = generate(fields, 30, seededRnd(41));
    for (const row of rows) expect(row.full).toBe(`${row.last} ${row.first}`);
  });

  it("kết quả giống nhau bất kể firstName hay fullName đứng trước trong danh sách field", () => {
    const withFullFirst = generate(
      [
        { name: "full", kind: "fullName", locale: "vi" },
        { name: "first", kind: "firstName", locale: "vi" },
      ],
      10,
      seededRnd(41),
    );
    const withFirstFirst = generate(
      [
        { name: "first", kind: "firstName", locale: "vi" },
        { name: "full", kind: "fullName", locale: "vi" },
      ],
      10,
      seededRnd(41),
    );
    expect(withFullFirst.map((row) => row.first)).toEqual(withFirstFirst.map((row) => row.first));
  });

  it("fullName + firstName + middleName + lastName cùng dòng khớp nhau hoàn toàn", () => {
    const fields: FieldSpec[] = [
      { name: "full", kind: "fullName", locale: "vi", includeMiddle: true },
      { name: "first", kind: "firstName", locale: "vi" },
      { name: "middle", kind: "middleName", locale: "vi" },
      { name: "last", kind: "lastName", locale: "vi" },
    ];
    const rows = generate(fields, 30, seededRnd(47));
    for (const row of rows) expect(row.full).toBe(`${row.last} ${row.middle} ${row.first}`);
  });

  it("email đi theo tên khi cùng dòng có field tên", () => {
    const fields: FieldSpec[] = [
      { name: "first", kind: "firstName", locale: "vi" },
      { name: "last", kind: "lastName", locale: "vi" },
      { name: "email", kind: "email", locale: "vi" },
    ];
    const rows = generate(fields, 30, seededRnd(53));
    for (const row of rows) {
      const email = row.email as string;
      const local = email.slice(0, email.indexOf("@"));
      const expectedPrefix = `${slugify(row.first as string)}.${slugify(row.last as string)}`;
      expect(local.startsWith(expectedPrefix)).toBe(true);
    }
  });

  // A fixed bug: email has no Locale picker of its own on the Panel, so it used to always default
  // to "vi", even when the name field in the same list was "en" — the email and the name then
  // belonged to two different people.
  it("email theo locale en khi field tên là en, dù bản thân field email không đặt locale", () => {
    const fields: FieldSpec[] = [
      { name: "first", kind: "firstName", locale: "en" },
      { name: "last", kind: "lastName", locale: "en" },
      { name: "email", kind: "email" },
    ];
    const rows = generate(fields, 30, seededRnd(59));
    for (const row of rows) {
      const email = row.email as string;
      const local = email.slice(0, email.indexOf("@"));
      const expectedPrefix = `${slugify(row.first as string)}.${slugify(row.last as string)}`;
      expect(local.startsWith(expectedPrefix)).toBe(true);
    }
  });

  it("email vẫn mặc định vi khi không có field tên nào trong danh sách", () => {
    const rows = generate([{ name: "email", kind: "email" }], 20, seededRnd(61));
    for (const row of rows) {
      expect(row.email as string).toMatch(/^[a-z0-9.]+@(example\.com|mail\.test|sample\.dev|demo\.io)$/);
    }
  });

  it("date nằm trong khoảng from/to, in ra ISO hợp lệ", () => {
    const fields: FieldSpec[] = [
      { name: "created_at", kind: "date", from: "2026-01-01T00:00:00Z", to: "2026-01-31T00:00:00Z" },
    ];
    const rows = generate(fields, 30, seededRnd(21));
    const start = Date.parse("2026-01-01T00:00:00Z");
    const end = Date.parse("2026-01-31T00:00:00Z");
    for (const row of rows) {
      const t = Date.parse(row.created_at as string);
      expect(t).toBeGreaterThanOrEqual(start);
      expect(t).toBeLessThanOrEqual(end);
    }
  });
});

describe("inferFields", () => {
  it("đoán kiểu từ mảng object mẫu", () => {
    const sample = [
      {
        id: "550e8400-e29b-41d4-a716-446655440000",
        email: "a@b.com",
        name: "An",
        age: 20,
        active: true,
        created_at: "2026-01-01T00:00:00Z",
      },
    ];
    expect(inferFields(sample)).toEqual([
      { name: "id", kind: "uuid" },
      { name: "email", kind: "email" },
      { name: "name", kind: "fullName" },
      { name: "age", kind: "integer" },
      { name: "active", kind: "boolean" },
      { name: "created_at", kind: "date" },
    ]);
  });

  it("đoán tên đệm từ tên cột middle_name/ten_dem", () => {
    expect(inferFields({ middle_name: "Văn" })).toEqual([{ name: "middle_name", kind: "middleName" }]);
    expect(inferFields({ ten_dem: "Văn" })).toEqual([{ name: "ten_dem", kind: "middleName" }]);
  });

  it("đoán được từ một object đơn, không cần mảng", () => {
    expect(inferFields({ phone: "0912345678" })).toEqual([{ name: "phone", kind: "phone" }]);
  });

  it("bỏ qua field lồng object hoặc mảng", () => {
    expect(inferFields([{ id: 1, meta: { a: 1 }, tags: ["x"] }])).toEqual([
      { name: "id", kind: "integer" },
    ]);
  });

  it("không đọc được thì trả null", () => {
    expect(inferFields("hello")).toBeNull();
    expect(inferFields([1, 2, 3])).toBeNull();
  });
});
