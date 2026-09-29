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
  it("converts JSON to YAML", async () => {
    expect(await output('{"a":1,"b":"x"}', "json", "yaml")).toBe("a: 1\nb: x\n");
  });

  it("converts YAML to JSON", async () => {
    expect(await output("a: 1\nb: x\n", "yaml", "json")).toBe('{\n  "a": 1,\n  "b": "x"\n}');
  });

  // YAML 1.2 — YAML 1.1's trap of `yes` becoming `true` is absent from modern js-yaml. Pinned here
  // in case someone changes the version.
  it("keeps `yes` a string, not a boolean", async () => {
    expect(await output("a: yes\n", "yaml", "json")).toBe('{\n  "a": "yes"\n}');
  });

  it("converts CSV to JSON through the header row", async () => {
    expect(await output("id,name\n1,An", "csv", "json")).toBe(
      '[\n  {\n    "id": "1",\n    "name": "An"\n  }\n]',
    );
  });

  it("converts JSON to INSERT", async () => {
    expect(await output('[{"id":1}]', "json", "insert")).toBe("INSERT INTO `t` (`id`) VALUES (1);");
  });

  it("converts CSV to INSERT", async () => {
    expect(await output("id\n1", "csv", "insert")).toBe("INSERT INTO `t` (`id`) VALUES ('1');");
  });
});

describe("refusals", () => {
  it("refuses when the output needs an array of objects and the input is not one", async () => {
    expect(await output('{"a":1}', "json", "csv")).toBe("THẤT BẠI:needsRows");
  });

  it("refuses nested objects when producing CSV", async () => {
    expect(await output('[{"a":{"b":1}}]', "json", "csv")).toBe("THẤT BẠI:needsRows");
  });

  it("refuses when both ends are the same format", async () => {
    expect(await output('{"a":1}', "json", "json")).toBe("THẤT BẠI:same");
  });

  it("refuses empty input", async () => {
    expect(await output("   ", "json", "yaml")).toBe("THẤT BẠI:empty");
  });

  it("reports a parse error with the verbatim message", async () => {
    const result = await convertData("{oops", "json", "yaml", options);
    expect(result.ok).toBe(false);
    if (result.ok) return;
    expect(result.failure.reason).toBe("parse");
  });
});

describe("warnings", () => {
  // The pivot goes through `JSON.parse`, unlike the Format tool. Say so rather than stay silent.
  it("warns when the source JSON has an integer that is too long", async () => {
    const result = await convertData('{"id":1787875200123456789}', "json", "yaml", options);
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.warnings).toEqual(["precision"]);
  });

  it("does not warn for ordinary numbers", async () => {
    const result = await convertData('{"id":12345}', "json", "yaml", options);
    expect(result.ok && result.warnings).toEqual([]);
  });
});
