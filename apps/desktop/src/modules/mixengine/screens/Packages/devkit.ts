import type { PackageCatalogue, PackageRelease, PackageSummary, RuntimeRelease } from "@mixengine/api";

/** The package that gives a Windows Ruby a compiler — roadmap task T206a. */
export const DEVKIT_PACKAGE = "msys2";

/** What a release cannot do on this machine's cell, in the publisher's words — T206, D2. */
export function lacksLabels(release: RuntimeRelease): { key: string; reason: string }[] {
  return Object.entries(release.lacks ?? {})
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([key, reason]) => ({ key, reason }));
}

/** Every reason a release lacks something, one a line, for the tooltip of its single mark. */
export function lacksReason(release: RuntimeRelease): string {
  return lacksLabels(release)
    .map(({ reason }) => reason)
    .join("\n");
}

/** Whether any Ruby here cannot build gems with C extensions — the devkit is offered once, above
 *  the list, rather than on every row: one install serves every Ruby. */
export function anyLacksNativeGems(releases: RuntimeRelease[]): boolean {
  return releases.some((release) => release.lacks?.["native gems"] !== undefined);
}

/** Release order on dotted numbers: `2026.10.08` comes after `2026.9.30`. */
function compareReleases(a: string, b: string): number {
  const left = a.split(".").map(Number);
  const right = b.split(".").map(Number);
  for (let i = 0; i < Math.max(left.length, right.length); i++) {
    const difference = (left[i] ?? 0) - (right[i] ?? 0);
    if (difference !== 0) return difference;
  }
  return 0;
}

/** The devkit this machine can install, and whether any version of it already is — T206a. */
export function devkitOffer(catalogue: PackageCatalogue): {
  release: PackageRelease | null;
  installed: boolean;
} {
  const releases = catalogue.packages.filter((release) => release.package === DEVKIT_PACKAGE);
  const release = releases.reduce<PackageRelease | null>(
    (best, next) => (best === null || compareReleases(next.version, best.version) > 0 ? next : best),
    null,
  );
  return { release, installed: releases.some((candidate) => candidate.installed) };
}

/** Whether a devkit is here, recorded with its folder gone, or neither — T206d. */
export type DevkitState =
  | { state: "absent" }
  | { state: "missing"; version: string }
  | { state: "present"; version: string };

/** Read from `package.list`, not the catalogue: a catalogue reads a missing devkit as not installed
 *  and would offer the newest release, leaving the missing row behind — T206d. */
export function devkitState(packages: PackageSummary[]): DevkitState {
  const newest = (rows: PackageSummary[]) =>
    rows.reduce<PackageSummary | null>(
      (best, next) => (best === null || compareReleases(next.version, best.version) > 0 ? next : best),
      null,
    );
  const recorded = packages.filter((row) => row.package === DEVKIT_PACKAGE);
  const present = newest(recorded.filter((row) => row.missing !== true));
  if (present !== null) return { state: "present", version: present.version };
  const missing = newest(recorded);
  return missing === null ? { state: "absent" } : { state: "missing", version: missing.version };
}
