import type {
  ArtifactAvailability,
  ExtensionOffer,
  ExtensionSummary,
  SiteSummary,
} from "@mixengine/api";

/** The offers a person can still install — T200, D4. An installed add-on is one card up, and a
 *  second row for it was the screen saying the same thing twice. */
export function notInstalled(offers: ExtensionOffer[]): ExtensionOffer[] {
  return offers.filter((offer) => !offer.installed);
}

/** Whether this machine has anything to install — T200, D8. Answered by the listing, so nobody
 *  opens a plan dialog whose only content is a refusal. */
export function installable(artifact: ArtifactAvailability): boolean {
  return artifact.type !== "other_targets";
}

/** The systems an add-on *is* published for, for the hover beside a disabled Install. */
export function publishedTargets(artifact: ArtifactAvailability): string[] {
  return artifact.type === "other_targets" ? artifact.targets : [];
}

export type KindKey =
  | "mixengine.extensions.kind.service"
  | "mixengine.extensions.kind.web-app"
  | "mixengine.extensions.kind.recipe";

/** The translation key for a kind, or `null` for one this build does not know — shown as the
 *  daemon wrote it, by `serviceStateLabel.ts`' rule. */
export function kindKey(kind: string): KindKey | null {
  switch (kind) {
    case "service":
    case "web-app":
    case "recipe":
      return `mixengine.extensions.kind.${kind}`;
    default:
      return null;
  }
}

/** What an installed add-on is for, or `null` — T200, D5. A daemon older than T200 sends no
 *  description, and an empty line under the name would look like a rendering fault. */
export function summaryDescription(row: ExtensionSummary): string | null {
  const text = row.description?.trim() ?? "";
  return text === "" ? null : text;
}

/** The ports an installed add-on holds, as `name number` pairs — T201, D5. The S3 endpoint of
 *  SeaweedFS and the SMTP port of Mailpit are what a person needs after installing, and no other
 *  screen shows them. `null` when it holds none, so the row draws no empty line. */
export function heldPorts(row: ExtensionSummary): string | null {
  if (row.ports.length === 0) return null;
  return row.ports.map((port) => `${port.name} ${port.wanted}`).join(" · ");
}

/** The site a `web-app` is served on, from `site.list` — T200, D6. `null` when the add-on names
 *  none or the listing has no such site, which only a broken install produces. */
export function webAppSite(row: ExtensionSummary, sites: SiteSummary[]): SiteSummary | null {
  if (!row.site) return null;
  return sites.find((site) => site.domain === row.site) ?? null;
}

export type RowAction = "open" | "turnOn" | "turnOff" | "start" | "stop";

/** Service states a Stop applies to: it is running, or on its way there. */
const STOPPABLE = new Set(["running", "starting", "degraded", "restarting"]);

/**
 * What a row offers besides Uninstall — T200, D6 and D7.
 *
 * A `web-app` is its site: Open always, and the one switch that changes it. A `service` gets the
 * one action its state allows, where the screen used to show Start and Stop side by side whatever
 * the state was. `stopping` offers nothing, because it is on its way to the answer already. An
 * unknown state offers Start, the action that can do no harm to a service that is not running.
 *
 * **A service that declares its page opens it, whatever its state** (T200a, D3): Open starts it
 * first when it is not running, so the person does not have to.
 */
export function rowActions(
  row: ExtensionSummary,
  serviceState: string | null | undefined,
  site: SiteSummary | null,
): RowAction[] {
  switch (row.kind) {
    case "web-app":
      if (site === null) return [];
      return site.state === "enabled" ? ["open", "turnOff"] : ["open", "turnOn"];
    case "service": {
      const run: RowAction[] =
        serviceState === "stopping"
          ? []
          : serviceState != null && STOPPABLE.has(serviceState)
            ? ["stop"]
            : ["start"];
      return row.ui ? ["open", ...run] : run;
    }
    default:
      return [];
  }
}

/** The add-ons being installed that no card has a row for — an install from a folder, which the
 *  registry does not offer — so the Installed card can show their progress until the job ends. */
export function installsWithoutARow(
  following: Record<string, number>,
  offers: ExtensionOffer[],
  installed: ExtensionSummary[],
): string[] {
  return Object.keys(following).filter(
    (id) => !offers.some((offer) => offer.id === id) && !installed.some((row) => row.id === id),
  );
}
