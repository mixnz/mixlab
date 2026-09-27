import { describe, expect, it } from "vitest";

import { packageRowsFrom, runtimeRowsFrom } from "./onDisk";

describe("runtimeRowsFrom", () => {
  it("keeps the daemon's reason and names the version", () => {
    const rows = runtimeRowsFrom({
      found: [{ kind: "php", version: "8.3.33", path: "C:/rt/php/8.3.33", why: "its marker names another install" }],
    });

    expect(rows).toEqual([
      {
        key: "php@8.3.33",
        name: "php",
        version: "8.3.33",
        path: "C:/rt/php/8.3.33",
        why: "its marker names another install",
      },
    ]);
  });
});

describe("packageRowsFrom", () => {
  const list = {
    found: [
      { package: "mariadb", version: "11.4.12", path: "C:/pk/mariadb/11.4.12", why: "no marker" },
      { package: "caddy", version: "2.8.4", path: "C:/pk/caddy/2.8.4", why: "no marker" },
    ],
  };

  it("keeps only the packages the tab shows", () => {
    const rows = packageRowsFrom(list, (name) => name === "mariadb");

    expect(rows.map((row) => row.key)).toEqual(["mariadb@11.4.12"]);
  });

  it("is empty when nothing is on disk", () => {
    expect(packageRowsFrom({ found: [] }, () => true)).toEqual([]);
  });
});
