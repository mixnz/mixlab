/**
 * Groups packages by function for the sub-tabs — display only; `package.*` has no field saying
 * this (see `PackageSummary`/`PackageRelease`).
 *
 * `"other"` is the catch-all for every unknown name, not an error case: a package MixEngine adds
 * later (a registry newer than the running MixDB) still has to show up somewhere, and must not
 * silently vanish because the lookup table lacks its name.
 */
export type PackageCategory = "web" | "database" | "cache" | "other";

/** The sub-tab order is always fixed, regardless of the order `package.list_available` returns. */
export const PACKAGE_CATEGORY_ORDER: PackageCategory[] = ["web", "database", "cache", "other"];

const WEB_SERVERS = new Set(["caddy", "nginx", "apache", "apache2", "httpd"]);
const DATABASES = new Set(["mariadb", "mysql", "postgres", "postgresql", "mongodb", "mongo", "sqlite"]);
const CACHES = new Set(["redis", "memcached"]);

export function packageCategory(packageName: string): PackageCategory {
  const key = packageName.toLowerCase();
  if (WEB_SERVERS.has(key)) return "web";
  if (DATABASES.has(key)) return "database";
  if (CACHES.has(key)) return "cache";
  return "other";
}
