import { useCallback, useEffect, useState } from "react";
import type { SiteSummary } from "@mixengine/api";
import * as api from "../../api";

/** Whether this home has a site yet — what the Quick Start card is drawn on (T117). */
export function useSites(active: boolean) {
  /** `site.list`, or `null` when not finished reading — the condition for drawing the Quick Start
   *  card (T117). */
  const [sites, setSites] = useState<SiteSummary[] | null>(null);

  /**
   * Whether this home has any site yet — the condition for drawing the Quick Start card (T117).
   *
   * Read separately from `reload()` rather than folded into its `Promise.all`: `reload()` runs
   * again on every return to the tab and every time the stream says something changed, while this
   * question only changes when a site is created or deleted. On failure the old value stays and no
   * error banner is raised: a Dashboard turned red because it could not ask "is there a site yet"
   * is a Dashboard turned red over a decorative question.
   */
  const readSites = useCallback(async () => {
    try {
      const listed = await api.sites();
      setSites(listed.sites);
    } catch {
      // Leave it: `null` is still "unknown", and `shouldOfferQuickStart` does not invite on `null`.
    }
  }, []);

  // The same beat, a separate call: see `readSites`.
  useEffect(() => {
    if (active) void readSites();
  }, [active, readSites]);

  return { sites, readSites };
}
