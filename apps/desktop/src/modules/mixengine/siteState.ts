import type { SiteOwner } from "@mixengine/api";
import type { SiteSummary } from "@mixengine/api";

import { joinPath, nativePath, PATH_STYLE, type PathStyle } from "../../core/paths";

export type SiteRow = SiteSummary;

/**
 * Only a site belonging to a project can be edited.
 *
 * An extension's site can only be viewed/listed here — a `site.update` sent straight at it is
 * still refused by the daemon even if the UI allowed it, but leaving a button that always fails
 * promises an action that cannot be kept.
 */
export function canEditSite(owner: SiteOwner): boolean {
  return owner.type === "project";
}

/**
 * Applies `site_sharing_changed` to the site table.
 *
 * The same rule the Dashboard follows for `service_state_changed`: events are best-effort, but when
 * one arrives it is the real source, not a guess. An unknown `type` or a broken payload is ignored,
 * not thrown — a variant born in a later version has to reach an older MixDB as an ignorable
 * object.
 */
export function applySharingChange(rows: SiteRow[], raw: string): SiteRow[] {
  let event: unknown;
  try {
    event = JSON.parse(raw);
  } catch {
    return rows;
  }
  if (
    typeof event !== "object" ||
    event === null ||
    (event as { type?: unknown }).type !== "site_sharing_changed"
  ) {
    return rows;
  }
  const { domain, sharing } = event as { domain: string; sharing: SiteRow["sharing"] };
  return rows.map((row) => (row.domain === domain ? { ...row, sharing } : row));
}

/** The display names of the typed domains, separated by commas or newlines — the first in the list
 *  is the primary. Shared by `SiteForm` and the "quick site" block in `ProjectForm`. */
export function parseDomains(raw: string): string[] {
  return raw
    .split(/[,\n]/)
    .map((d) => d.trim())
    .filter((d) => d !== "");
}

/**
 * The remainder of an absolute path after removing the project root — used right after the
 * directory-picker dialog (which always returns an absolute path) returns, so the Doc root field
 * holds only the part the daemon really stores (`SiteSummary.doc_root`), written with the operating
 * system's separator as the daemon sends it back (T191), so the field holds the same string whether
 * it came from Browse or from the daemon. Not under the root keeps the absolute path —
 * `SiteCreate.doc_root` accepts both, and this is a rare case not worth blocking.
 */
export function relativeToRoot(
  root: string,
  absolute: string,
  style: PathStyle = PATH_STYLE,
): string {
  const normalizedRoot = root.replace(/[\\/]+$/, "");
  if (absolute === normalizedRoot) return "";
  for (const separator of ["/", "\\"]) {
    const prefix = `${normalizedRoot}${separator}`;
    if (absolute.startsWith(prefix)) return nativePath(absolute.slice(prefix.length), style);
  }
  return absolute;
}

/** Joins the root with the remainder for display, using the operating system's separator (T191) —
 *  for reading only, not the value sent to the daemon (that is still the remainder on its own).
 *  `""` is the root itself, exactly as `SiteSummary.doc_root` describes. */
export function joinDocRoot(root: string, relative: string, style: PathStyle = PATH_STYLE): string {
  if (relative === "") return root;
  return joinPath(root, relative, style);
}

/** `mm:ss`, or `hh:mm:ss` once more than an hour remains. Past due clamps to 0, never negative. */
export function formatRemaining(untilMs: number, nowMs: number = Date.now()): string {
  const totalSeconds = Math.max(0, Math.round((untilMs - nowMs) / 1000));
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return hours > 0 ? `${pad(hours)}:${pad(minutes)}:${pad(seconds)}` : `${pad(minutes)}:${pad(seconds)}`;
}

/**
 * The openable address of a site — T117.
 *
 * `SiteSummary` carries the domain and an `https` flag, not a URL: the daemon returns *what the
 * site is*, and turning it into an address is a display matter. In exactly one place because two
 * places would drift apart on exactly the day one of them gets changed.
 *
 * **`https` is a *declaration*, not a certificate already issued.** A freshly created site has
 * `https: true` before anyone has had time to allow the CA to be installed; this link is still the
 * right link to open, and whatever the browser warns about belongs to the elevation pass not yet
 * done.
 */
export function siteUrl(site: { domain: string; https: boolean }): string {
  return `${site.https ? "https" : "http"}://${site.domain}`;
}

/** The two things a click on a domain produces, kept apart from *doing* them. */
export interface SiteVisit {
  /** The project whose services must be started before opening, or `null` if there is nothing to
   *  start. */
  startProject: string | null;
  /** The address to open afterwards. */
  url: string;
}

/**
 * What clicking a site's domain should do.
 *
 * **An extension's site is only opened, nothing is started** — `service.start` takes a *project
 * name*, and an extension site has no project; guessing a name would send the daemon something it
 * will refuse. What serves it is that extension's business, and the button stays clickable rather
 * than becoming a dead row in the middle of the table.
 *
 * The `disabled` state is deliberately *not* considered here: it says whether the web server
 * produces a server block, and opening it to see exactly that error is still an answer, not a
 * button that does nothing.
 */
export function siteVisit(site: {
  domain: string;
  https: boolean;
  owner: SiteOwner;
}): SiteVisit {
  return {
    startProject: site.owner.type === "project" ? site.owner.name : null,
    url: siteUrl(site),
  };
}
