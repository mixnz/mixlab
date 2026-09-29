import { describe, expect, it } from "vitest";

import { matchesAvailable } from "./availableFilter";

describe("matchesAvailable", () => {
  it("keeps every row while the box is empty", () => {
    expect(matchesAvailable(["php", "8.3.14", "stable"], "")).toBe(true);
    expect(matchesAvailable(["php", "8.3.14", "stable"], "   ")).toBe(true);
  });

  it("matches a fragment of any single field", () => {
    expect(matchesAvailable(["php", "8.3.14", "stable"], "8.3")).toBe(true);
    expect(matchesAvailable(["php", "8.3.14", "stable"], "sta")).toBe(true);
  });

  it("is case-insensitive", () => {
    expect(matchesAvailable(["MariaDB", "11.4.2", "stable"], "mariadb")).toBe(true);
  });

  it("matches words spread across fields, in any order", () => {
    expect(matchesAvailable(["php", "8.3.14", "stable"], "php 8.3")).toBe(true);
    expect(matchesAvailable(["php", "8.3.14", "stable"], "8.3 php")).toBe(true);
  });

  it("drops a row when one word matches nothing", () => {
    expect(matchesAvailable(["php", "8.3.14", "stable"], "php 9")).toBe(false);
    expect(matchesAvailable(["php", "8.3.14", "stable"], "node")).toBe(false);
  });
});
