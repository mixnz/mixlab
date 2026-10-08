import type { DeclaredSite } from "@mixengine/api";

/** A `DeclaredSite` as a row of the project panel. Whether Add is offered is read off the state
 *  the daemon computed, never worked out here (T204). */
export interface DeclaredSiteRow {
  domain: string;
  aliases: string[];
  status: "here" | "missing" | "elsewhere";
  owner?: string;
  canAdd: boolean;
}

export function declaredSiteRows(sites: DeclaredSite[] | undefined): DeclaredSiteRow[] {
  return (sites ?? []).map((site) => ({
    domain: site.domain,
    aliases: site.aliases,
    status: site.state.is,
    owner: site.state.is === "elsewhere" ? site.state.owner : undefined,
    canAdd: site.state.is === "missing",
  }));
}
