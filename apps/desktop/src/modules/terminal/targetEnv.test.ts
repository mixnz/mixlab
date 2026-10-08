import { describe, expect, it } from "vitest";
import { formatEnvLines, parseEnvLines, parsePathLines } from "./targetEnv";

describe("parseEnvLines", () => {
  it("reads KEY=value lines and skips blank ones", () => {
    expect(parseEnvLines("A=1\n\n B = two words \n")).toEqual({ env: { A: "1", B: "two words" } });
  });
  it("names the first line that is not KEY=value", () => {
    expect(parseEnvLines("A=1\nnot a pair")).toEqual({ error: 2 });
  });
  it("keeps everything after the first = as the value", () => {
    expect(parseEnvLines("URL=a=b")).toEqual({ env: { URL: "a=b" } });
  });
});

describe("formatEnvLines", () => {
  it("is the inverse of parsing", () => {
    expect(formatEnvLines({ A: "1", B: "x" })).toBe("A=1\nB=x");
    expect(formatEnvLines(undefined)).toBe("");
  });
});

describe("parsePathLines", () => {
  it("one directory per line, trimmed, blanks dropped", () => {
    expect(parsePathLines(" C:\a b\bin \n\n/usr/x\n")).toEqual(["C:\a b\bin", "/usr/x"]);
  });
});
