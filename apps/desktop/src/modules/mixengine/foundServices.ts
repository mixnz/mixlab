import type { ServiceFoundList } from "@mixengine/api";

/**
 * `service.found` as rows to draw — roadmap task T182g.
 *
 * Pure and reading only what the daemon answered: whether a row can be adopted is `opens_with`
 * being there, and why not is the daemon's own sentence. Which version opens which data is decided
 * in the daemon, never here.
 */
export interface FoundRow {
  id: string;
  path: string;
  adoptable: boolean;
  /** The installed version the daemon would run it with, when it can be adopted. */
  opensWith: string | null;
  /** The daemon's reason it cannot be adopted yet, with what to do. */
  whyNot: string | null;
}

export function rowsFrom(list: ServiceFoundList): FoundRow[] {
  return list.found.map((row) => ({
    id: row.service,
    path: row.path,
    adoptable: row.opens_with != null,
    opensWith: row.opens_with ?? null,
    whyNot: row.why_not ?? null,
  }));
}
