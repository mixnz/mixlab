import { describe, expect, it } from "vitest";
import type { BlueprintPlan, PackageRelease, ProjectPin, RuntimeRelease } from "@mixengine/api";
import { devkitNeed, planDevkitNeed } from "./devkitNeed";

const pin = (kind: string, resolved: string | null): ProjectPin =>
  ({ kind, constraint: "3.4", source: "row", resolved }) as unknown as ProjectPin;
const ruby = (version: string, lacks?: Record<string, string>): RuntimeRelease =>
  ({ kind: "ruby", version, channel: "stable", bytes: 1, installed: true, lacks }) as RuntimeRelease;
const msys2 = { package: "msys2", version: "2026.10.08", channel: "stable", bytes: 9, installed: false } as PackageRelease;

describe("devkitNeed", () => {
  it("is the devkit to offer when the project's Ruby cannot build native gems and none is installed", () => {
    expect(
      devkitNeed([pin("ruby", "3.4.11")], [ruby("3.4.11", { "native gems": "x" })], { release: msys2, installed: false }),
    ).toEqual({ offer: msys2 });
  });

  it("is nothing once a devkit is installed", () => {
    expect(
      devkitNeed([pin("ruby", "3.4.11")], [ruby("3.4.11", { "native gems": "x" })], { release: msys2, installed: true }),
    ).toBeNull();
  });

  it("is nothing for a Ruby that builds them, or a project with no Ruby", () => {
    expect(devkitNeed([pin("ruby", "3.4.11")], [ruby("3.4.11")], { release: msys2, installed: false })).toBeNull();
    expect(devkitNeed([pin("php", "8.4.26")], [ruby("3.4.11", { "native gems": "x" })], { release: msys2, installed: false })).toBeNull();
    expect(devkitNeed([pin("ruby", null)], [ruby("3.4.11", { "native gems": "x" })], { release: msys2, installed: false })).toBeNull();
  });

  it("still says so where no devkit is offered for this machine, with nothing to install", () => {
    expect(
      devkitNeed([pin("ruby", "3.4.11")], [ruby("3.4.11", { "native gems": "x" })], { release: null, installed: false }),
    ).toEqual({ offer: null });
  });
});

/* Asked where the decision is made: the Apply dialog's plan, before anything is installed. */
describe("planDevkitNeed", () => {
  const plan = (pins: Record<string, string> | undefined) =>
    ({
      steps: [{ action: { action: "register_project", name: "shop", root: "/p", pins }, disposition: { disposition: "go" }, elevates: false }],
    }) as unknown as BlueprintPlan;

  it("offers the devkit for a blueprint whose Ruby line cannot build native gems", () => {
    const releases = [ruby("3.4.11", { "native gems": "x" }), ruby("4.0.7", { "native gems": "x" })];
    expect(planDevkitNeed(plan({ ruby: "3.4" }), releases, { release: msys2, installed: false })).toEqual({ offer: msys2 });
  });

  it("is nothing for a blueprint with no Ruby, a Ruby that builds them, or a devkit already here", () => {
    const lacking = [ruby("3.4.11", { "native gems": "x" })];
    expect(planDevkitNeed(plan({ php: "8.4" }), lacking, { release: msys2, installed: false })).toBeNull();
    expect(planDevkitNeed(plan(undefined), lacking, { release: msys2, installed: false })).toBeNull();
    expect(planDevkitNeed(plan({ ruby: "3.4" }), [ruby("3.4.11")], { release: msys2, installed: false })).toBeNull();
    expect(planDevkitNeed(plan({ ruby: "3.4" }), lacking, { release: msys2, installed: true })).toBeNull();
  });

  it("reads the line, not another one", () => {
    const releases = [ruby("3.4.11"), ruby("4.0.7", { "native gems": "x" })];
    expect(planDevkitNeed(plan({ ruby: "3.4" }), releases, { release: msys2, installed: false })).toBeNull();
  });
});
