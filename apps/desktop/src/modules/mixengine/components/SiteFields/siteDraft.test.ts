import { describe, expect, it } from "vitest";

import type { SiteDetail } from "@mixengine/api";
import {
  emptySiteDraft,
  kindPayload,
  needsRiskyTldConsent,
  routeFromApi,
  routeToApi,
  siteCreateFields,
  siteDraftFromDetail,
  siteUpdateFields,
  type SiteDraft,
} from "./siteDraft";

function draft(change: Partial<SiteDraft>): SiteDraft {
  return { ...emptySiteDraft(), ...change };
}

describe("siteCreateFields", () => {
  it("sends an empty field as null, so the project's mixengine.toml fills it", () => {
    expect(siteCreateFields(emptySiteDraft())).toEqual({
      domains: null,
      doc_root: null,
      kind: { kind: "php-fpm", pool: null },
      services: null,
      routes: null,
      https: false,
      https_redirect: false,
      accept_risky_tld: false,
    });
  });

  it("sends what was filled in", () => {
    const fields = siteCreateFields(
      draft({
        domainsText: "blog.test, www.blog.test",
        docRoot: "public",
        services: new Set(["mariadb@main"]),
        routes: [{ path: "/api", target: "proxy", upstream: "http://127.0.0.1:3000", pool: "", root: "" }],
        https: true,
        httpsRedirect: true,
      }),
    );
    expect(fields.domains).toEqual(["blog.test", "www.blog.test"]);
    expect(fields.doc_root).toBe("public");
    expect(fields.services).toEqual(["mariadb@main"]);
    expect(fields.routes).toEqual([{ path: "/api", target: "proxy", upstream: "http://127.0.0.1:3000" }]);
    expect(fields.https_redirect).toBe(true);
  });

  it("never sends a redirect without HTTPS", () => {
    expect(siteCreateFields(draft({ https: false, httpsRedirect: true })).https_redirect).toBe(false);
  });
});

describe("siteUpdateFields", () => {
  it("sends every field, an empty list as empty rather than absent", () => {
    const fields = siteUpdateFields(draft({ enabled: false }));
    expect(fields.domains).toEqual([]);
    expect(fields.doc_root).toBe("");
    expect(fields.services).toEqual([]);
    expect(fields.routes).toEqual([]);
    expect(fields.state).toBe("disabled");
  });
});

describe("kindPayload", () => {
  it("sends only the field of the chosen kind", () => {
    const base = { pool: "php-fpm@8.4", upstream: "http://x", port: "3000" };
    expect(kindPayload(draft({ ...base, kind: "php-fpm" }))).toEqual({ kind: "php-fpm", pool: "php-fpm@8.4" });
    expect(kindPayload(draft({ ...base, kind: "static" }))).toEqual({ kind: "static" });
    expect(kindPayload(draft({ ...base, kind: "reverse-proxy" }))).toEqual({
      kind: "reverse-proxy",
      upstream: "http://x",
    });
    expect(kindPayload(draft({ ...base, kind: "node-app" }))).toEqual({ kind: "node-app", port: 3000 });
  });
});

describe("routes", () => {
  it("round-trip through the editing row", () => {
    const route = { path: "/", target: "php-fpm" as const, pool: null };
    expect(routeToApi(routeFromApi(route))).toEqual(route);
  });
});

describe("needsRiskyTldConsent", () => {
  it("asks only for a .local name", () => {
    expect(needsRiskyTldConsent(draft({ domainsText: "blog.test" }))).toBe(false);
    expect(needsRiskyTldConsent(draft({ domainsText: "blog.test, blog.local" }))).toBe(true);
  });
});

describe("siteDraftFromDetail", () => {
  it("reads a site from a daemon that sends no redirect and no routes", () => {
    const detail = {
      site: {
        domain: "blog.test",
        doc_root: "public",
        kind: { kind: "node-app", port: 3000 },
        https: true,
        state: "enabled",
      },
      domains: ["blog.test"],
      services: [{ service: "redis@main" }],
    } as unknown as SiteDetail;
    const result = siteDraftFromDetail(detail);
    expect(result.port).toBe("3000");
    expect(result.routes).toEqual([]);
    expect(result.httpsRedirect).toBe(false);
    expect([...result.services]).toEqual(["redis@main"]);
    expect(result.enabled).toBe(true);
  });
});
