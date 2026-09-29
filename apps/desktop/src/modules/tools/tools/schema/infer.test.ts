import { describe, expect, it } from "vitest";
import { inferSchema } from "./infer";

describe("inferSchema", () => {
  it("đọc một object đơn", () => {
    expect(inferSchema({ id: 1, name: "An" })).toEqual([
      { name: "id", types: ["integer"], optional: false, isoLike: false },
      { name: "name", types: ["string"], optional: false, isoLike: false },
    ]);
  });

  it("hợp các khoá của mọi phần tử trong mảng mẫu", () => {
    expect(inferSchema([{ a: 1 }, { b: "x" }])).toEqual([
      { name: "a", types: ["integer"], optional: true, isoLike: false },
      { name: "b", types: ["string"], optional: true, isoLike: false },
    ]);
  });

  it("khoá có mặt ở mọi phần tử thì không optional", () => {
    const fields = inferSchema([{ a: 1 }, { a: 2 }]);
    expect(fields?.[0]?.optional).toBe(false);
  });

  // `integer` meeting `number` widens to `number`; both are not kept.
  it("nới integer thành number khi thấy cả hai", () => {
    expect(inferSchema([{ a: 1 }, { a: 1.5 }])?.[0]?.types).toEqual(["number"]);
  });

  it("giữ null bên cạnh kiểu thật thay vì nuốt mất", () => {
    expect(inferSchema([{ a: 1 }, { a: null }])?.[0]?.types).toEqual(["integer", "null"]);
  });

  it("đánh dấu chuỗi trông như ISO 8601", () => {
    const fields = inferSchema([{ at: "2026-08-28T00:00:00Z" }, { at: "2026-08-29T10:30:00Z" }]);
    expect(fields?.[0]?.isoLike).toBe(true);
  });

  it("không đánh dấu khi có một giá trị không phải ISO", () => {
    const fields = inferSchema([{ at: "2026-08-28T00:00:00Z" }, { at: "hôm qua" }]);
    expect(fields?.[0]?.isoLike).toBe(false);
  });

  it("đi xuống object lồng nhau", () => {
    const fields = inferSchema({ user: { id: 1 } });
    expect(fields?.[0]?.types).toEqual(["object"]);
    expect(fields?.[0]?.children).toEqual([
      { name: "id", types: ["integer"], optional: false, isoLike: false },
    ]);
  });

  it("lấy hình dạng phần tử của mảng object", () => {
    const fields = inferSchema({ tags: [{ n: "a" }, { n: "b" }] });
    expect(fields?.[0]?.types).toEqual(["array"]);
    expect(fields?.[0]?.children?.[0]?.name).toBe("n");
  });

  it("để mảng rỗng không có children", () => {
    expect(inferSchema({ tags: [] })?.[0]?.children).toBeUndefined();
  });

  it("trả null khi đầu vào không phải object hay mảng object", () => {
    expect(inferSchema(42)).toBeNull();
    expect(inferSchema([1, 2])).toBeNull();
    expect(inferSchema(null)).toBeNull();
  });
});
