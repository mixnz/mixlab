import { describe, expect, it } from "vitest";

import type { UpgradePlan } from "@mixengine/api";

import { oldVersionLine, planLines } from "./upgradePlanLines";

// Returns the key and its parameters, so a test asserts which sentence and with what, not wording.
const t = (key: string, params?: Record<string, string | number>) =>
  `${key}${params ? " " + JSON.stringify(params) : ""}`;

const plan = (overrides: Partial<UpgradePlan> = {}): UpgradePlan => ({
  subject: "php",
  from: "8.4.24",
  to: "8.4.25",
  to_installed: false,
  bytes: 31457280,
  stale: false,
  needs: [],
  entries: [
    { item: { item: "default" }, outcome: { outcome: "planned" } },
    {
      item: { item: "site", site: "blog.test", from: "php-fpm@8.4.24", to: "php-fpm@8.4.25" },
      outcome: { outcome: "planned" },
    },
    {
      item: { item: "extension_pool", pool: "php-fpm@phpmyadmin", moves: false, requires: "^8.5" },
      outcome: { outcome: "planned" },
    },
  ],
  old: { state: "will_be_removed" },
  ...overrides,
});

describe("planLines", () => {
  it("says one sentence per entry with the names it is about", () => {
    const lines = planLines(plan(), t);

    expect(lines[0]).toContain("mixengine.upgrade.default");
    expect(lines[1]).toContain("mixengine.upgrade.site");
    expect(lines[1]).toContain("blog.test");
    expect(lines[2]).toContain("mixengine.upgrade.extensionPoolStays");
    expect(lines[2]).toContain("^8.5");
  });
});

describe("oldVersionLine", () => {
  it("says the old version goes when nothing keeps it", () => {
    expect(oldVersionLine(plan(), t)).toContain("mixengine.upgrade.willBeRemoved");
  });

  it("names why the old version stays", () => {
    const kept = plan({ old: { state: "will_be_kept", because: ["blog pins php 8.4.24"] } });
    const line = oldVersionLine(kept, t);
    expect(line).toContain("mixengine.upgrade.willBeKept");
    expect(line).toContain("blog pins php 8.4.24");
  });
});
