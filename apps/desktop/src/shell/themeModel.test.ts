import { describe, expect, it } from "vitest";
import { clearRetiredKeys, resolveTheme, RETIRED_KEYS } from "./themeModel";

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
});
