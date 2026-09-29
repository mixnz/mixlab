import type { RouteTarget, SiteCreate, SiteDetail, SiteKind, SiteRoute, SiteUpdate } from "@mixengine/api";
import { parseDomains } from "../../siteState";

export type Kind = SiteKind["kind"];
export type Target = RouteTarget["target"];

/**
 * One route while being edited — T135.
 *
 * Flat, not a union: the user switches back and forth between targets, and the field they just
 * typed into has to still be there when they switch back. `routeToApi` is where it narrows to
 * exactly the shape the daemon takes.
 */
export interface RouteRow {
  path: string;
  target: Target;
  upstream: string;
  pool: string;
  root: string;
}

/**
 * Every value a site form holds while it is being filled in — the one shape both `SiteForm` and the
 * "also create a site" section of `ProjectForm` edit through `SiteFields`, so a field added here
 * reaches both at once.
 */
export interface SiteDraft {
  domainsText: string;
  /** Relative to the project's root, as `SiteSummary.doc_root` stores it. */
  docRoot: string;
  kind: Kind;
  pool: string;
  upstream: string;
  port: string;
  routes: RouteRow[];
  services: Set<string>;
  https: boolean;
  /** T98. Only meaningful with `https` on — the daemon refuses `true` beside `https: false`, so the
   *  payload helpers below never send that pair. */
  httpsRedirect: boolean;
  acceptRiskyTld: boolean;
  /** Edit only: a site being created is always enabled. */
  enabled: boolean;
}

export function emptySiteDraft(): SiteDraft {
  return {
    domainsText: "",
    docRoot: "",
    kind: "php-fpm",
    pool: "",
    upstream: "",
    port: "",
    routes: [],
    services: new Set(),
    https: false,
    httpsRedirect: false,
    acceptRiskyTld: false,
    enabled: true,
  };
}

export function siteDraftFromDetail(detail: SiteDetail): SiteDraft {
  const kind = detail.site.kind;
  return {
    domainsText: detail.domains.join(", "),
    docRoot: detail.site.doc_root,
    kind: kind.kind,
    pool: kind.kind === "php-fpm" ? (kind.pool ?? "") : "",
    upstream: kind.kind === "reverse-proxy" ? kind.upstream : "",
    port: kind.kind === "node-app" ? String(kind.port) : "",
    // T135. `?? []` because a daemon built before T135 does not send this field.
    routes: (detail.site.routes ?? []).map(routeFromApi),
    services: new Set(detail.services.map((s) => s.service)),
    https: detail.site.https,
    // T98. `?? false` because a daemon built before T98 does not send this field.
    httpsRedirect: detail.site.https_redirect ?? false,
    acceptRiskyTld: false,
    enabled: detail.site.state === "enabled",
  };
}

export function emptyRoute(): RouteRow {
  return { path: "", target: "proxy", upstream: "", pool: "", root: "" };
}

/** A `SiteRoute` from the daemon, expanded into a row being edited. */
export function routeFromApi(route: SiteRoute): RouteRow {
  return {
    path: route.path,
    target: route.target,
    upstream: route.target === "proxy" ? route.upstream : "",
    pool: route.target === "php-fpm" ? (route.pool ?? "") : "",
    root: route.target === "static" ? route.root : "",
  };
}

/** And back again — only sends the fields of the chosen target. */
export function routeToApi(row: RouteRow): SiteRoute {
  switch (row.target) {
    case "proxy":
      return { path: row.path, target: "proxy", upstream: row.upstream };
    case "php-fpm":
      return { path: row.path, target: "php-fpm", pool: row.pool === "" ? null : row.pool };
    case "static":
      return { path: row.path, target: "static", root: row.root };
  }
}

export function draftDomains(draft: SiteDraft): string[] {
  return parseDomains(draft.domainsText);
}

/** A `.local` name collides with mDNS, and the daemon wants that said out loud before it takes one. */
export function needsRiskyTldConsent(draft: SiteDraft): boolean {
  return draftDomains(draft).some((d) => d.endsWith(".local"));
}

export function kindPayload(draft: SiteDraft): SiteKind {
  switch (draft.kind) {
    case "php-fpm":
      return { kind: "php-fpm", pool: draft.pool === "" ? null : draft.pool };
    case "static":
      return { kind: "static" };
    case "reverse-proxy":
      return { kind: "reverse-proxy", upstream: draft.upstream };
    case "node-app":
      return { kind: "node-app", port: Number(draft.port) };
  }
}

/**
 * Everything of `site.create` but the project. An empty field goes as `null`, not as an empty
 * value, so `site.create` falls back to the project's `mixengine.toml` — the same for `domains`,
 * `doc_root`, `services` and `routes`.
 */
export function siteCreateFields(draft: SiteDraft): Omit<SiteCreate, "project"> {
  const domains = draftDomains(draft);
  return {
    domains: domains.length > 0 ? domains : null,
    doc_root: draft.docRoot === "" ? null : draft.docRoot,
    kind: kindPayload(draft),
    services: draft.services.size > 0 ? [...draft.services] : null,
    routes: draft.routes.length > 0 ? draft.routes.map(routeToApi) : null,
    https: draft.https,
    https_redirect: draft.https && draft.httpsRedirect,
    accept_risky_tld: draft.acceptRiskyTld,
  };
}

/** Everything of `site.update` but the site. Every field is sent: an update replaces, so an empty
 *  list here is "none left", not "leave it". */
export function siteUpdateFields(draft: SiteDraft): Omit<SiteUpdate, "site"> {
  return {
    domains: draftDomains(draft),
    doc_root: draft.docRoot,
    kind: kindPayload(draft),
    services: [...draft.services],
    routes: draft.routes.map(routeToApi),
    https: draft.https,
    https_redirect: draft.https && draft.httpsRedirect,
    state: draft.enabled ? "enabled" : "disabled",
    accept_risky_tld: draft.acceptRiskyTld,
  };
}
