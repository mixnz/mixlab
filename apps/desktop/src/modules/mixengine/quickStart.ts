import type { SiteSummary } from "@mixengine/api";

/**
 * Whether the Dashboard invites the user to build their first site — T117.
 *
 * **It is the home's state, not a saved flag.** A "skipped" flag is something to store, to migrate,
 * and someone will edit it by hand wrongly; reading from `site.list`, a machine that has just
 * deleted every site sees the invitation card come back — correctly, and with nothing to reset.
 *
 * `null` means **the list has not arrived**, not an empty home: inviting on `null` would flash the
 * card even in front of someone with twenty sites, every time the tab opens.
 *
 * **Projects do not count.** Someone with three projects and no site still has no website — that
 * is exactly the complaint T117 was written to answer.
 */
export function shouldOfferQuickStart(sites: SiteSummary[] | null): boolean {
  return sites !== null && sites.length === 0;
}

/**
 * Whether the typed project name is usable yet.
 *
 * Not a copy of the daemon's slug rules — the daemon is still where refusals happen, and this card
 * must not guess on its behalf. This is only the condition for **enabling the button**: empty
 * means there is nothing to send.
 */
export function canStart(project: string, root: string): boolean {
  return project.trim() !== "" && root.trim() !== "";
}
