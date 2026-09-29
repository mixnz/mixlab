import { describe, expect, it } from "vitest";
import { convertData, type ConvertOptions, type ReadFormat, type WriteFormat } from "./pivot";

const options: ConvertOptions = {
  delimiter: ",",
  header: true,
  table: "t",
  dialect: "mysql",
  multiRow: false,
};

const output = async (text: string, from: ReadFormat, to: WriteFormat): Promise<string> => {
  const result = await convertData(text, from, to, options);
  return result.ok ? result.output : `THẤT BẠI:${result.failure.reason}`;
};

describe("convertData", () => {
  it("đổi JSON sang YAML", async () => {
    expect(await output('{"a":1,"b":"x"}', "json", "yaml")).toBe("a: 1\nb: x\n");
  });

  it("đổi YAML sang JSON", async () => {
    expect(await output("a: 1\nb: x\n", "yaml", "json")).toBe('{\n  "a": 1,\n  "b": "x"\n}');
  });

  // YAML 1.2 — YAML 1.1's trap of `yes` becoming `true` is absent from modern js-yaml. Pinned here
  // in case someone changes the version.
  it("để `yes` là chuỗi, không thành boolean", async () => {
    expect(await output("a: yes\n", "yaml", "json")).toBe('{\n  "a": "yes"\n}');
  });

  it("đổi CSV sang JSON qua dòng tiêu đề", async () => {
    expect(await output("id,name\n1,An", "csv", "json")).toBe(
      '[\n  {\n    "id": "1",\n    "name": "An"\n  }\n]',
    );
  });

  it("đổi JSON sang INSERT", async () => {
    expect(await output('[{"id":1}]', "json", "insert")).toBe("INSERT INTO `t` (`id`) VALUES (1);");
  });

  it("đổi CSV sang INSERT", async () => {
    expect(await output("id\n1", "csv", "insert")).toBe("INSERT INTO `t` (`id`) VALUES ('1');");
  });
});

describe("từ chối", () => {
  it("từ chối khi đầu ra cần mảng object mà đầu vào không phải", async () => {
    expect(await output('{"a":1}', "json", "csv")).toBe("THẤT BẠI:needsRows");
  });

  it("từ chối object lồng nhau khi ra CSV", async () => {
    expect(await output('[{"a":{"b":1}}]', "json", "csv")).toBe("THẤT BẠI:needsRows");
  });

  it("từ chối khi hai đầu cùng định dạng", async () => {
    expect(await output('{"a":1}', "json", "json")).toBe("THẤT BẠI:same");
  });

  it("từ chối đầu vào rỗng", async () => {
    expect(await output("   ", "json", "yaml")).toBe("THẤT BẠI:empty");
  });

  it("báo lỗi parse kèm nguyên văn thông báo", async () => {
    const result = await convertData("{oops", "json", "yaml", options);
    expect(result.ok).toBe(false);
    if (result.ok) return;
    expect(result.failure.reason).toBe("parse");
  });
});

describe("cảnh báo", () => {
  // The pivot goes through `JSON.parse`, unlike the Format tool. Say so rather than stay silent.
  it("cảnh báo khi JSON nguồn có số nguyên quá dài", async () => {
    const result = await convertData('{"id":1787875200123456789}', "json", "yaml", options);
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.warnings).toEqual(["precision"]);
  });

  it("không cảnh báo với số bình thường", async () => {
    const result = await convertData('{"id":12345}', "json", "yaml", options);
    expect(result.ok && result.warnings).toEqual([]);
  });
});
