import { describe, expect, it } from "vitest";
import type { NextStep, PlanAction, SiteSummary } from "@mixengine/api";
import { addressFor, firstSiteDomain, siteAddresses, siteOfEachStep, stepsBySite } from "./siteGroups";

const site = {
  action: "create_site",
  kind: { kind: "static" },
  doc_root: "",
  https: true,
} as unknown as PlanAction;
const name = (domain: string, primary = true): PlanAction => ({ action: "add_domain", domain, primary });
const cert = { action: "issue_certificate", domains: [] } as unknown as PlanAction;
const other = { action: "set_php_extension", name: "xdebug" } as unknown as PlanAction;

describe("siteOfEachStep", () => {
  it("puts each name and certificate under the site before it", () => {
    expect(
      siteOfEachStep([site, name("shop.test"), name("www.shop.test", false), cert, site, name("docs.shop.test")]),
    ).toEqual(["shop.test", "shop.test", "shop.test", "shop.test", "docs.shop.test", "docs.shop.test"]);
  });

  it("leaves steps outside a site's group unclaimed", () => {
    expect(siteOfEachStep([other, site, name("shop.test"), other])).toEqual([null, "shop.test", "shop.test", null]);
  });
});

describe("firstSiteDomain", () => {
  it("is the first site's primary, and null without a site", () => {
    expect(firstSiteDomain([site, name("shop.test"), site, name("docs.shop.test")])).toBe("shop.test");
    expect(firstSiteDomain([other])).toBeNull();
  });
});

describe("siteAddresses", () => {
  const sites = [
    { domain: "docs.shop.test", https: true },
    { domain: "shop.test", https: false },
  ] as SiteSummary[];

  it("opens the blueprint's first site, not the list's", () => {
    expect(siteAddresses(sites, "shop.test").first).toBe("http://shop.test");
  });

  it("falls back to the list's first site", () => {
    expect(siteAddresses(sites, null).first).toBe("https://docs.shop.test");
    expect(siteAddresses([], null).first).toBeNull();
  });

  it("opens a step's own site, and the first site for a step that names none", () => {
    const addresses = siteAddresses(sites, "shop.test");
    expect(addressFor(addresses, { kind: "open", site: "docs.shop.test" } as NextStep)).toBe("https://docs.shop.test");
    expect(addressFor(addresses, { kind: "open" } as NextStep)).toBe("http://shop.test");
    expect(addressFor(addresses, { kind: "open", site: "gone.test" } as NextStep)).toBeNull();
  });
});

describe("stepsBySite", () => {
  it("is one group when no step names a site", () => {
    const steps = [
      { kind: "once", run: "a" },
      { kind: "serve", run: "b" },
    ] as NextStep[];
    expect(stepsBySite(steps)).toEqual([{ site: null, steps }]);
  });

  it("groups in first-seen order, keeping each group's order", () => {
    const a = { kind: "once", run: "a", site: "x.test" } as NextStep;
    const b = { kind: "serve", run: "b", site: "y.test" } as NextStep;
    const c = { kind: "once", run: "c", site: "x.test" } as NextStep;
    expect(stepsBySite([a, b, c])).toEqual([
      { site: "x.test", steps: [a, c] },
      { site: "y.test", steps: [b] },
    ]);
  });
});
