import { describe, expect, it } from "vitest";
import {
  clearRetiredKeys,
  parseColorTheme,
  parseThemeMode,
  resolvePalette,
  resolveTheme,
  RETIRED_KEYS,
} from "./themeModel";

describe("clearRetiredKeys", () => {
  it("removes the stored glass setting", () => {
    const removed: string[] = [];
    clearRetiredKeys({ removeItem: (key) => void removed.push(key) });
    expect(removed).toEqual([...RETIRED_KEYS]);
  });
});

describe("resolveTheme", () => {
  it("keeps an explicit choice whatever the OS prefers", () => {
    expect(resolveTheme("light", true)).toBe("light");
    expect(resolveTheme("dark", false)).toBe("dark");
  });

  it("settles system against the OS", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
  });

  it("draws a colour theme over dark, whatever the OS prefers", () => {
    expect(resolveTheme("color", false)).toBe("dark");
  });
});

describe("resolvePalette", () => {
  it("names a palette only under Colour", () => {
    expect(resolvePalette("color", "dim")).toBe("dim");
    expect(resolvePalette("dark", "dim")).toBeNull();
    expect(resolvePalette("light", "dim")).toBeNull();
    expect(resolvePalette("system", "dim")).toBeNull();
  });
});

describe("stored values", () => {
  it("reads an unknown or absent mode as system", () => {
    expect(parseThemeMode(null)).toBe("system");
    expect(parseThemeMode("sepia")).toBe("system");
    expect(parseThemeMode("color")).toBe("color");
  });

  it("reads an unknown or absent colour theme as navy", () => {
    expect(parseColorTheme(null)).toBe("navy");
    expect(parseColorTheme("teal")).toBe("navy");
    expect(parseColorTheme("nord")).toBe("nord");
  });
});
