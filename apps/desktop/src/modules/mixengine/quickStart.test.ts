import { describe, expect, it } from "vitest";
import type { SiteSummary } from "@mixengine/api";

import { canStart, shouldOfferQuickStart } from "./quickStart";

/** A site, with enough fields to count. The content does not matter: the question is *whether*. */
const a_site = { domain: "blog.test" } as unknown as SiteSummary;

describe("shouldOfferQuickStart", () => {
  it("offers on a home with no sites", () => {
    expect(shouldOfferQuickStart([])).toBe(true);
  });

  it("does not offer once a site exists", () => {
    expect(shouldOfferQuickStart([a_site])).toBe(false);
  });

  /* `null` is "not finished reading", not "empty" — inviting on it would flash the card in front of
     someone who already has sites, every time the tab opens. */
  it("does not offer before the listing has arrived", () => {
    expect(shouldOfferQuickStart(null)).toBe(false);
  });
});

describe("canStart", () => {
  it("needs both a name and a folder", () => {
    expect(canStart("blog", "/projects/blog")).toBe(true);
    expect(canStart("", "/projects/blog")).toBe(false);
    expect(canStart("blog", "")).toBe(false);
  });

  /* Whitespace is not a name. The real slug rules still belong to the daemon — this only decides
     whether the button can be pressed. */
  it("does not count whitespace as either", () => {
    expect(canStart("   ", "/projects/blog")).toBe(false);
    expect(canStart("blog", "   ")).toBe(false);
  });
});
