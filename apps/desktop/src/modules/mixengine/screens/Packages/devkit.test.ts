import { describe, expect, it } from "vitest";
import type { PackageCatalogue, PackageRelease, RuntimeRelease } from "@mixengine/api";
import { devkitOffer, lacksLabels } from "./devkit";

const ruby = (lacks?: Record<string, string>) =>
  ({ kind: "ruby", version: "3.4.11", channel: "stable", bytes: 1, installed: false, lacks }) as RuntimeRelease;

describe("lacksLabels", () => {
  it("lists each key with its reason, in key order", () => {
    expect(lacksLabels(ruby({ yjit: "b", "native gems": "a" }))).toEqual([
      { key: "native gems", reason: "a" },
      { key: "yjit", reason: "b" },
    ]);
  });

  it("is empty for a release lacking nothing", () => {
    expect(lacksLabels(ruby())).toEqual([]);
  });
});

describe("devkitOffer", () => {
  const release = (version: string, installed: boolean) =>
    ({ package: "msys2", version, channel: "stable", bytes: 9, installed }) as PackageRelease;

  it("offers the newest release by release order and says whether one is installed", () => {
    const catalogue = {
      packages: [release("2026.10.08", false), release("2026.9.30", true)],
      stale: false,
    } as unknown as PackageCatalogue;
    expect(devkitOffer(catalogue)).toEqual({ release: release("2026.10.08", false), installed: true });
  });

  it("offers nothing where the index has no msys2", () => {
    expect(devkitOffer({ packages: [], stale: false } as unknown as PackageCatalogue)).toEqual({
      release: null,
      installed: false,
    });
  });
});
