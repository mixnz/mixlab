import type { HomePrevious } from "@mixengine/api";

/**
 * `home.previous` as the Dashboard's offer — roadmap task T182h.
 *
 * Pure, and reading only what the daemon answered: it counted what the copy holds and said whether
 * a newer build wrote it. `null` is no offer at all.
 */
export interface RestoreOffer {
  projects: number;
  sites: number;
  services: number;
  /** False for a copy a newer MixEngine wrote, which this build does not restore. */
  restorable: boolean;
}

export function offerFrom(previous: HomePrevious): RestoreOffer | null {
  const copy = previous.copy;
  if (copy == null) return null;

  return {
    projects: Number(copy.projects),
    sites: Number(copy.sites),
    services: Number(copy.services),
    restorable: !copy.newer,
  };
}
