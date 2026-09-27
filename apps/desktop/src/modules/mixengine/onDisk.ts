import type { PackageFoundList, RuntimeFoundList } from "@mixengine/api";

/**
 * `runtime.found` and `package.found` as rows to draw — roadmap task T182i.
 *
 * Pure, and reading only what the daemon answered: the reason beside **Adopt** is the daemon's own
 * phrase, and whether adopting works is for `runtime.adopt` / `package.adopt` to say.
 */
export interface OnDiskRow {
  key: string;
  /** The language or package, as the daemon names it. */
  name: string;
  version: string;
  path: string;
  why: string;
}

export function runtimeRowsFrom(list: RuntimeFoundList): OnDiskRow[] {
  return list.found.map((row) => ({
    key: `${row.kind}@${row.version}`,
    name: row.kind,
    version: row.version,
    path: row.path,
    why: row.why,
  }));
}

/** `shown` is the tab's own filter: each Packages tab draws the packages of its category. */
export function packageRowsFrom(list: PackageFoundList, shown: (name: string) => boolean): OnDiskRow[] {
  return list.found
    .filter((row) => shown(row.package))
    .map((row) => ({
      key: `${row.package}@${row.version}`,
      name: row.package,
      version: row.version,
      path: row.path,
      why: row.why,
    }));
}
