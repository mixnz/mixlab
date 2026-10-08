import { describe, expect, it } from "vitest";
import type { PackageCatalogue, PackageRelease, RuntimeRelease } from "@mixengine/api";
import { anyLacksNativeGems, devkitOffer, lacksLabels, lacksReason } from "./devkit";

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

/* T206, measured by hand: two pills and a devkit button on every Ruby row overflowed the row. One
   mark a row, its reasons on hover; the devkit offered once, above the list. */
describe("one mark a row", () => {
  it("joins every reason into the mark's tooltip, in key order", () => {
    expect(lacksReason(ruby({ yjit: "b", "native gems": "a" }))).toBe("a\nb");
    expect(lacksReason(ruby())).toBe("");
  });

  it("offers the devkit when any Ruby here cannot build native gems", () => {
    expect(anyLacksNativeGems([ruby({ "native gems": "a" }), ruby()])).toBe(true);
    expect(anyLacksNativeGems([ruby({ yjit: "b" })])).toBe(false);
    expect(anyLacksNativeGems([])).toBe(false);
  });
});
