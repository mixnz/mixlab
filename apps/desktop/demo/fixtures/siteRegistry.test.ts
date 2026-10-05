import type { ProjectSummary, SiteDetail, SiteSummary } from "@mixengine/api";
import { describe, expect, it } from "vitest";
import { createSiteRegistry } from "./siteRegistry";

const project = (name: string) => ({ type: "project" as const, name });
const SITES: SiteSummary[] = [
  {
    domain: "acme-shop.test",
    owner: project("acme-shop"),
    kind: { kind: "static" },
    doc_root: "",
    https: true,
    https_redirect: true,
    state: "enabled",
  },
  {
    domain: "blog.test",
    owner: project("blog"),
    kind: { kind: "php-fpm", pool: "php-fpm@8.3" },
    doc_root: "public",
    https: true,
    https_redirect: true,
    state: "enabled",
  },
];
const PROJECTS: ProjectSummary[] = [
  { name: "acme-shop", root: "/Users/ada/Sites/acme-shop", created_at: "2025-11-03T09:12:00Z", keep_warm: true },
  { name: "blog", root: "/Users/ada/Sites/blog", created_at: "2025-12-18T16:40:00Z", keep_warm: false },
];
const detailOf = (site: SiteSummary): SiteDetail => ({
  site,
  root: "/old",
  doc_root_full: "/old",
  doc_root_exists: true,
  domains: [site.domain],
  services: [],
});
const make = (without?: string[]) =>
  createSiteRegistry({ sites: SITES, projects: PROJECTS, without, defaultPool: "php-fpm@8.3", detailOf });

describe("createSiteRegistry", () => {
  it("lists what it started with, exactly, and details them as before", () => {
    const registry = make();
    expect(registry.list().sites).toEqual(SITES);
    expect(registry.list("").sites).toEqual(SITES);
    expect(registry.detail("blog.test")).toEqual(detailOf(SITES[1]));
  });

  it("filters by project the way the daemon does", () => {
    expect(make().list("blog").sites.map((s) => s.domain)).toEqual(["blog.test"]);
  });

  it("starts without the domains a clip is about to create", () => {
    expect(make(["blog.test"]).list().sites.map((s) => s.domain)).toEqual(["acme-shop.test"]);
  });

  it("creates a php-fpm site with the resolved pool, at the project's root, and lists it after", () => {
    const registry = make(["blog.test"]);
    const creation = registry.create({ project: { name: "blog" }, domains: ["blog.test"], https: true });
    expect(creation.site).toEqual({
      site: {
        domain: "blog.test",
        owner: project("blog"),
        kind: { kind: "php-fpm", pool: "php-fpm@8.3" },
        doc_root: "",
        https: true,
        https_redirect: false,
        state: "enabled",
      },
      root: "/Users/ada/Sites/blog",
      doc_root_full: "/Users/ada/Sites/blog",
      doc_root_exists: true,
      domains: ["blog.test"],
      pool: { declared: "php-fpm@8.3", resolved: "php-fpm@8.3" },
      services: [],
    });
    expect(registry.list().sites.map((s) => s.domain)).toEqual(["acme-shop.test", "blog.test"]);
    expect(registry.detail("blog.test")).toEqual(creation.site);
  });

  it("refuses a domain that is already declared, and drops a redirect without HTTPS", () => {
    expect(() => make().create({ project: { name: "blog" }, domains: ["blog.test"] })).toThrow(
      "blog.test is already declared",
    );
    const fresh = make(["blog.test"]).create({
      project: { name: "blog" },
      domains: ["blog.test"],
      https: false,
      https_redirect: true,
    });
    expect(fresh.site.site.https_redirect).toBe(false);
  });

  it("refuses a project it does not know", () => {
    expect(() => make().create({ project: { name: "nope" }, domains: ["x.test"] })).toThrow("no project named nope");
  });

  it("starts a fresh machine with no project and no site", () => {
    const registry = createSiteRegistry({
      sites: SITES,
      projects: PROJECTS,
      fresh: true,
      defaultPool: "php-fpm@8.3",
      detailOf,
    });
    expect(registry.list().sites).toEqual([]);
    expect(registry.projects().projects).toEqual([]);
  });

  it("creates a site under a project registered after it started", () => {
    const registry = createSiteRegistry({
      sites: SITES,
      projects: PROJECTS,
      fresh: true,
      defaultPool: "php-fpm@8.3",
      detailOf,
    });
    registry.registerProject({
      name: "blog",
      root: "/Users/ada/Sites/blog",
      created_at: "2026-01-15T10:24:00Z",
      keep_warm: false,
    });
    registry.create({
      project: { name: "blog" },
      domains: ["blog.test"],
      https: true,
      doc_root: "public",
      kind: { kind: "php-fpm", pool: "php-fpm@8.4" },
    });
    expect(registry.list("blog").sites.map((s) => [s.domain, s.doc_root])).toEqual([["blog.test", "public"]]);
    expect(registry.projects().projects.map((p) => p.name)).toEqual(["blog"]);
  });

  it("lists the projects it started with", () => {
    expect(make().projects().projects).toEqual(PROJECTS);
  });
});
