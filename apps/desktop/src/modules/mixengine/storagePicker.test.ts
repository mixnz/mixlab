import { describe, expect, it } from "vitest";

import type { StorageReport } from "@mixengine/api";

import {
  chosenFrom,
  explanationOf,
  isFree,
  oneFolderFor,
  pick,
  rowsFrom,
} from "./storagePicker";

/** The `--storage` answer of a home nobody has touched yet. */
function aFreeHome(): StorageReport {
  return {
    root: "/home/me/MixEngine",
    paths: {
      runtimes: { path: "/home/me/MixEngine/runtimes", relocated: false },
      packages: { path: "/home/me/MixEngine/packages", relocated: false },
      data: { path: "/home/me/MixEngine/data", relocated: false },
      logs: { path: "/home/me/MixEngine/logs", relocated: false },
    },
    changeable: { changeable: "free" },
  };
}

describe("rowsFrom", () => {
  it("draws four rows in the order the configuration file lists them", () => {
    expect(rowsFrom(aFreeHome()).map((row) => row.key)).toEqual([
      "runtimes",
      "packages",
      "data",
      "logs",
    ]);
  });

  it("carries what the daemon said and nothing the client worked out", () => {
    const report = aFreeHome();
    report.paths.data = { path: "/bulk/data", relocated: true };

    const rows = rowsFrom(report);
    const data = rows.find((row) => row.key === "data");

    expect(data).toEqual({
      key: "data",
      current: "/bulk/data",
      relocated: true,
      picked: null,
    });
  });
});

describe("chosenFrom", () => {
  it("sends nothing when nobody picked anything", () => {
    expect(chosenFrom(rowsFrom(aFreeHome()))).toBeUndefined();
  });

  it("sends only the key that was picked", () => {
    const rows = pick(rowsFrom(aFreeHome()), "data", "/bulk/data");

    expect(chosenFrom(rows)).toEqual({ data: "/bulk/data" });
  });

  // The daemon treats an equal value as a silent no-op, but we do not rely on that: what gets sent
  // should be what changed, or `config.toml` gets rewritten for a click that changed nothing.
  it("does not send a key picked at the place it already is", () => {
    const rows = pick(rowsFrom(aFreeHome()), "data", "/home/me/MixEngine/data");

    expect(chosenFrom(rows)).toBeUndefined();
  });

  it("is undefined and never an empty object, because the two read differently", () => {
    expect(chosenFrom([])).toBeUndefined();
  });
});

describe("oneFolderFor", () => {
  it("fills all four from one folder, each under its own name", () => {
    const rows = oneFolderFor(rowsFrom(aFreeHome()), "/Volumes/SSD/mixengine-bulk");

    expect(chosenFrom(rows)).toEqual({
      runtimes: "/Volumes/SSD/mixengine-bulk/runtimes",
      packages: "/Volumes/SSD/mixengine-bulk/packages",
      data: "/Volumes/SSD/mixengine-bulk/data",
      logs: "/Volumes/SSD/mixengine-bulk/logs",
    });
  });

  it("does not double the separator when the folder ends in one", () => {
    for (const ending of ["/Volumes/SSD/bulk/", "/Volumes/SSD/bulk//", "C:\\bulk\\"]) {
      const rows = oneFolderFor(rowsFrom(aFreeHome()), ending);

      for (const row of rows) {
        expect(row.picked).not.toContain("//");
        expect(row.picked).not.toContain("\\\\");
      }
    }
  });

  // The same directory, both buttons must send the same spelling: the daemon writes `config.toml`
  // through `toml_edit`, which escapes backslashes itself, so there is no longer any reason to
  // force forward slashes.
  it("joins in Windows spelling on Windows, whichever separator the folder came with", () => {
    for (const folder of ["D:\\bulk", "D:/bulk"]) {
      const rows = oneFolderFor(rowsFrom(aFreeHome()), folder, "windows");
      expect(rows[0]?.picked).toBe("D:\\bulk\\runtimes");
    }
  });

  it("joins with a forward slash elsewhere", () => {
    const rows = oneFolderFor(rowsFrom(aFreeHome()), "/Volumes/SSD/bulk", "posix");
    expect(rows[0]?.picked).toBe("/Volumes/SSD/bulk/runtimes");
  });
});

describe("what the daemon decided", () => {
  it("reads free from the answer rather than from the paths", () => {
    expect(isFree(aFreeHome())).toBe(true);
    expect(explanationOf(aFreeHome())).toBeNull();
  });

  it("hands back the daemon's own sentence once something is installed", () => {
    const report = aFreeHome();
    report.changeable = {
      changeable: "taken",
      runtimes: 3,
      packages: 0,
      services: 1,
      explanation: "3 runtimes and 1 service are installed",
    };

    expect(isFree(report)).toBe(false);
    expect(explanationOf(report)).toBe("3 runtimes and 1 service are installed");
  });
});
