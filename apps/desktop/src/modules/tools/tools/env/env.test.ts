import { describe, expect, it } from "vitest";
import { parseEnv, parseJsonEnv, toDockerArgs, toEnv, toExport, toJsonEnv } from "./env";

describe("parseEnv", () => {
  it("reads key-value pairs", () => {
    expect(parseEnv("A=1\nB=x")).toEqual([
      { key: "A", value: "1" },
      { key: "B", value: "x" },
    ]);
  });

  it("skips blank lines and comment lines", () => {
    expect(parseEnv("# ghi chú\n\nA=1")).toEqual([{ key: "A", value: "1" }]);
  });

  it("drops the export prefix", () => {
    expect(parseEnv("export A=1")).toEqual([{ key: "A", value: "1" }]);
  });

  it("cuts a comment after an unquoted value", () => {
    expect(parseEnv("A=1 # ghi chú")).toEqual([{ key: "A", value: "1" }]);
  });

  it("keeps a hash inside quotes", () => {
    expect(parseEnv('A="mật#khẩu"')).toEqual([{ key: "A", value: "mật#khẩu" }]);
  });

  it("keeps a single-quoted value verbatim", () => {
    expect(parseEnv("A='dòng\\ncó gạch'")).toEqual([{ key: "A", value: "dòng\\ncó gạch" }]);
  });

  it("expands escapes in double quotes", () => {
    expect(parseEnv('A="dòng\\nsau"')).toEqual([{ key: "A", value: "dòng\nsau" }]);
    expect(parseEnv('A="nói \\"chào\\""')).toEqual([{ key: "A", value: 'nói "chào"' }]);
  });

  it("joins a quoted value spanning several lines", () => {
    expect(parseEnv('KEY="dòng một\ndòng hai"')).toEqual([
      { key: "KEY", value: "dòng một\ndòng hai" },
    ]);
  });

  it("keeps an equals sign inside the value", () => {
    expect(parseEnv("URL=postgres://u:p@h/db?a=b")).toEqual([
      { key: "URL", value: "postgres://u:p@h/db?a=b" },
    ]);
  });
});

describe("parseJsonEnv", () => {
  it("reads a flat object", () => {
    expect(parseJsonEnv('{"A":"1","B":2}')).toEqual([
      { key: "A", value: "1" },
      { key: "B", value: "2" },
    ]);
  });

  it("returns null when it is not an object", () => {
    expect(parseJsonEnv("[1]")).toBeNull();
    expect(parseJsonEnv("hỏng")).toBeNull();
  });
});

describe("writers", () => {
  const pairs = [
    { key: "A", value: "1" },
    { key: "B", value: "có dấu cách" },
  ];

  it("writes .env, quoting when needed", () => {
    expect(toEnv(pairs)).toBe('A=1\nB="có dấu cách"');
  });

  it("writes the export form", () => {
    expect(toExport(pairs)).toBe('export A=1\nexport B="có dấu cách"');
  });

  it("ghi JSON", () => {
    expect(toJsonEnv(pairs)).toBe('{\n  "A": "1",\n  "B": "có dấu cách"\n}');
  });

  it("writes docker arguments quoted by shell rules", () => {
    expect(toDockerArgs(pairs)).toBe("-e A='1' -e B='có dấu cách'");
  });

  it("closes, escapes, then reopens single quotes for docker", () => {
    expect(toDockerArgs([{ key: "A", value: "it's" }])).toBe("-e A='it'\\''s'");
  });

  it("escapes newlines and double quotes when writing .env", () => {
    expect(toEnv([{ key: "A", value: 'hai\ndòng "x"' }])).toBe('A="hai\\ndòng \\"x\\""');
  });
});
