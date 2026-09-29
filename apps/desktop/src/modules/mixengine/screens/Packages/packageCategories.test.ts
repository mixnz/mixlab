import { describe, expect, it } from "vitest";

import { packageCategory } from "./packageCategories";

describe("packageCategory", () => {
  it("puts caddy under web", () => {
    expect(packageCategory("caddy")).toBe("web");
  });

  it("puts mariadb and mysql under database", () => {
    expect(packageCategory("mariadb")).toBe("database");
    expect(packageCategory("mysql")).toBe("database");
  });

  it("puts redis under cache", () => {
    expect(packageCategory("redis")).toBe("cache");
  });

  it("is case-insensitive", () => {
    expect(packageCategory("Caddy")).toBe("web");
  });

  it("falls back to other for an unknown name, instead of losing it", () => {
    expect(packageCategory("something-new")).toBe("other");
  });
});
