import type { NextStep, PlanAction, SiteSummary } from "@mixengine/api";
import { siteUrl } from "./siteState";

/**
 * The primary domain of the site each plan step belongs to, or `null` for a step outside every
 * site's group — T204a, D4.
 *
 * **Read off the order, as the daemon's executor reads it**: a site's names are the `add_domain`
 * steps straight after its `create_site`, and its certificate follows them. Nothing is decided here
 * that the plan did not already say.
 */
export function siteOfEachStep(actions: PlanAction[]): (string | null)[] {
  let current: string | null = null;
  return actions.map((action, i) => {
    if (action.action === "create_site") {
      const next = actions[i + 1];
      current = next?.action === "add_domain" ? next.domain : null;
    } else if (action.action !== "add_domain" && action.action !== "issue_certificate") {
      current = null;
    }
    return current;
  });
}

/** The first site a blueprint describes: the name after its first `create_site`. */
export function firstSiteDomain(actions: PlanAction[]): string | null {
  const at = actions.findIndex((action) => action.action === "create_site");
  const next = at < 0 ? undefined : actions[at + 1];
  return next?.action === "add_domain" ? next.domain : null;
}

/** Where a project's steps open: by the site a step names, else the first site — T204a, D5. */
export interface SiteAddresses {
  first: string | null;
  byDomain: Record<string, string>;
}

/**
 * Every site's address, and which one is "the" site.
 *
 * `first` is the blueprint's first site when the caller knows it (`firstSiteDomain`), because
 * `site.list` is ordered by the daemon and not by the blueprint; otherwise the list's first.
 */
export function siteAddresses(sites: SiteSummary[], first: string | null): SiteAddresses {
  const byDomain: Record<string, string> = {};
  for (const site of sites) byDomain[site.domain] = siteUrl(site);
  const chosen = first !== null ? byDomain[first] : undefined;
  return { first: chosen ?? (sites[0] ? siteUrl(sites[0]) : null), byDomain };
}

/** The address a step opens: its own site's when it names one, the first site's otherwise. */
export function addressFor(addresses: SiteAddresses, step: NextStep): string | null {
  if (step.site) return addresses.byDomain[step.site] ?? null;
  return addresses.first;
}

/** Steps under their site, in first-seen order; one group with `site: null` when none names one. */
export function stepsBySite(steps: NextStep[]): { site: string | null; steps: NextStep[] }[] {
  const groups: { site: string | null; steps: NextStep[] }[] = [];
  for (const step of steps) {
    const site = step.site ?? null;
    const group = groups.find((one) => one.site === site);
    if (group) group.steps.push(step);
    else groups.push({ site, steps: [step] });
  }
  return groups;
}
