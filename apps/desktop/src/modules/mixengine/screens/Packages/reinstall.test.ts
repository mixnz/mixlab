import { describe, expect, it } from "vitest";
import { reinstallState, releaseToRestore } from "./reinstall";

describe("releaseToRestore", () => {
  const releases = [
    { kind: "php", version: "8.3.33" },
    { kind: "php", version: "8.4.1" },
    { kind: "ruby", version: "8.3.33" },
  ];

  it("finds the same version of the same kind", () => {
    expect(releaseToRestore(releases, (r) => r.kind === "php", "8.3.33")).toEqual({
      kind: "php",
      version: "8.3.33",
    });
  });

  it("is null when the catalogue no longer offers it", () => {
    expect(releaseToRestore(releases, (r) => r.kind === "php", "8.2.0")).toBeNull();
  });
});

describe("reinstallState", () => {
  it("is running while its job runs, so a second click starts nothing", () => {
    expect(reinstallState({ version: "8.3.33" }, true)).toBe("running");
  });

  it("is unavailable when the catalogue no longer offers the version", () => {
    expect(reinstallState(null, false)).toBe("unavailable");
  });

  it("is ready otherwise", () => {
    expect(reinstallState({ version: "8.3.33" }, false)).toBe("ready");
  });
});
