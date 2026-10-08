import { describe, expect, it } from "vitest";

import { declaredSiteRows } from "./declaredSites";
import type { DeclaredSite } from "@mixengine/api";

describe("declaredSiteRows", () => {
  it("offers Add only for a site this home does not have", () => {
    const sites: DeclaredSite[] = [
      { domain: "web.test", aliases: [], state: { is: "here" } },
      { domain: "api.test", aliases: ["www.api.test"], state: { is: "missing" } },
      { domain: "taken.test", aliases: [], state: { is: "elsewhere", owner: "blog" } },
    ];

    expect(declaredSiteRows(sites)).toEqual([
      { domain: "web.test", aliases: [], status: "here", owner: undefined, canAdd: false },
      {
        domain: "api.test",
        aliases: ["www.api.test"],
        status: "missing",
        owner: undefined,
        canAdd: true,
      },
      { domain: "taken.test", aliases: [], status: "elsewhere", owner: "blog", canAdd: false },
    ]);
  });

  it("is empty when the daemon sends no list", () => {
    expect(declaredSiteRows(undefined)).toEqual([]);
  });
});
