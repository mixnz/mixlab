import type { ServiceSummary, SiteSummary } from "@mixengine/api";

/** A service that cannot start, and the daemon's sentence saying why — T200b, D6. */
export interface Failure {
  service: string;
  detail: string;
}

/** A failed service's note, or `null` — T200b, D6. Only while `failed`, which the daemon already
 *  guarantees; checked here too so a stale row cannot show a failure the state no longer claims. */
export function serviceFailure(id: string | null | undefined, services: ServiceSummary[]): Failure | null {
  if (!id) return null;
  const found = services.find((service) => service.id === id);
  if (!found || found.state !== "failed" || !found.last_failure) return null;
  return { service: found.id, detail: found.last_failure.detail };
}

/** Why a site cannot be served, from its pool — T200b, D6. A pool is what a request wakes, so it is
 *  what the starting page is about; the services a site is linked to are in `site.show`. */
export function siteFailure(site: SiteSummary, services: ServiceSummary[]): Failure | null {
  return site.kind.kind === "php-fpm" ? serviceFailure(site.kind.pool, services) : null;
}
