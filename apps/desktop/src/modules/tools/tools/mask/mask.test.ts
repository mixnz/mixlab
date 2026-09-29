import { describe, expect, it } from "vitest";
import {
  detectFieldSpecs,
  maskRows,
  maskValue,
  parseFlatRows,
  type FieldMaskSpec,
} from "./mask";

describe("detectFieldSpecs", () => {
  it("guesses the shape and default kind from the column name", () => {
    const rows = [
      {
        id: 1,
        email: "a@b.com",
        phone: "0912345678",
        full_name: "Nguyễn Văn An",
        credit_card: "4111111111111111",
        cmnd: "079203001234",
        ngay_sinh: "1995-01-01",
        address: "123 Main St",
        note: "vip",
      },
    ];
    const byName = Object.fromEntries(detectFieldSpecs(rows).map((s) => [s.name, s]));
    expect(byName.id).toEqual({ name: "id", shape: "generic", kind: "none" });
    expect(byName.email).toEqual({ name: "email", shape: "email", kind: "partial" });
    expect(byName.phone).toEqual({ name: "phone", shape: "phone", kind: "partial" });
    expect(byName.full_name).toEqual({ name: "full_name", shape: "name", kind: "partial" });
    expect(byName.credit_card).toEqual({ name: "credit_card", shape: "card", kind: "partial" });
    expect(byName.cmnd).toEqual({ name: "cmnd", shape: "idNumber", kind: "partial" });
    expect(byName.ngay_sinh).toEqual({ name: "ngay_sinh", shape: "dob", kind: "redact" });
    expect(byName.address).toEqual({ name: "address", shape: "address", kind: "redact" });
    expect(byName.note).toEqual({ name: "note", shape: "generic", kind: "none" });
  });

  it("lists columns in order of first appearance, merged across rows", () => {
    const rows = [{ b: 1 }, { a: 2, b: 3 }];
    expect(detectFieldSpecs(rows).map((s) => s.name)).toEqual(["b", "a"]);
  });
});

describe("maskValue", () => {
  it("none keeps the value and its original type", () => {
    expect(maskValue(42, "none", "generic")).toBe(42);
    expect(maskValue(null, "none", "generic")).toBeNull();
  });

  it("empty or null values are left unchanged, whatever the kind", () => {
    expect(maskValue(null, "redact", "generic")).toBeNull();
    expect(maskValue(undefined, "hash", "email")).toBeUndefined();
    expect(maskValue("", "partial", "name")).toBe("");
  });

  it("redact always gives ***", () => {
    expect(maskValue("bất kỳ giá trị nào", "redact", "generic")).toBe("***");
  });

  it("partial with the email shape keeps the first letter and the whole domain", () => {
    expect(maskValue("jane.doe@example.com", "partial", "email")).toBe("j*******@example.com");
  });

  it("partial with the phone shape keeps the last 2 digits", () => {
    expect(maskValue("0912345678", "partial", "phone")).toBe("********78");
  });

  it("partial with the card shape keeps the last 4 digits", () => {
    expect(maskValue("4111111111111111", "partial", "card")).toBe("************1111");
  });

  it("partial with the idNumber shape keeps the last 4 digits", () => {
    expect(maskValue("079203001234", "partial", "idNumber")).toBe("********1234");
  });

  it("partial with the name shape abbreviates each word", () => {
    expect(maskValue("Nguyễn Văn An", "partial", "name")).toBe("N*** V*** A***");
  });

  it("partial with the generic shape keeps the first and last letters", () => {
    expect(maskValue("abcdefg", "partial", "generic")).toBe("a*****g");
    expect(maskValue("ab", "partial", "generic")).toBe("**");
  });

  it("hash is deterministic — the same value always gives the same code", () => {
    const first = maskValue("customer-42", "hash", "generic");
    const second = maskValue("customer-42", "hash", "generic");
    expect(first).toBe(second);
    expect(first).toMatch(/^h_[0-9a-f]{8}$/);
  });

  it("hash gives different codes for different values", () => {
    expect(maskValue("a", "hash", "generic")).not.toBe(maskValue("b", "hash", "generic"));
  });
});

describe("maskRows", () => {
  it("only masks fields whose kind is not none, leaving the rest intact", () => {
    const rows = [
      { id: 1, email: "a@b.com" },
      { id: 2, email: "c@d.com" },
    ];
    const specs: FieldMaskSpec[] = [
      { name: "id", shape: "generic", kind: "none" },
      { name: "email", shape: "email", kind: "redact" },
    ];
    expect(maskRows(rows, specs)).toEqual([
      { id: 1, email: "***" },
      { id: 2, email: "***" },
    ]);
  });

  it("hash stays consistent when the same value repeats across rows", () => {
    const rows = [{ key: "same" }, { key: "same" }, { key: "different" }];
    const specs: FieldMaskSpec[] = [{ name: "key", shape: "generic", kind: "hash" }];
    const [first, second, third] = maskRows(rows, specs);
    expect(first!.key).toBe(second!.key);
    expect(first!.key).not.toBe(third!.key);
  });
});

describe("parseFlatRows", () => {
  it("accepts an array of flat objects", () => {
    expect(parseFlatRows([{ a: 1 }, { a: 2 }])).toEqual([{ a: 1 }, { a: 2 }]);
  });

  it("accepts a single object, wrapping it into a one-element array", () => {
    expect(parseFlatRows({ a: 1 })).toEqual([{ a: 1 }]);
  });

  it("refuses nested objects/arrays — it does not silently skip them", () => {
    expect(parseFlatRows([{ a: 1, nested: { x: 1 } }])).toBeNull();
    expect(parseFlatRows([{ a: 1, tags: ["x"] }])).toBeNull();
  });

  it("returns null when it cannot be read", () => {
    expect(parseFlatRows("hello")).toBeNull();
    expect(parseFlatRows([1, 2, 3])).toBeNull();
    expect(parseFlatRows([])).toBeNull();
  });
});
