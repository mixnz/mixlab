import { describe, expect, it } from "vitest";

import { rowsFrom } from "./foundServices";

describe("rowsFrom", () => {
  it("offers adopt for a row the daemon says opens, and names the version", () => {
    const rows = rowsFrom({
      found: [{ service: "mariadb@main", path: "C:/data/mariadb/main", opens_with: "11.4.5" }],
    });

    expect(rows).toEqual([
      { id: "mariadb@main", path: "C:/data/mariadb/main", adoptable: true, opensWith: "11.4.5", whyNot: null },
    ]);
  });

  it("keeps the daemon's reason for a row that cannot be adopted, and offers nothing", () => {
    const rows = rowsFrom({
      found: [{ service: "mariadb@half", path: "C:/data/mariadb/half", why_not: "its first run never finished" }],
    });

    expect(rows[0]).toMatchObject({ adoptable: false, opensWith: null, whyNot: "its first run never finished" });
  });

  it("is empty for an empty answer", () => {
    expect(rowsFrom({ found: [] })).toEqual([]);
  });
});
