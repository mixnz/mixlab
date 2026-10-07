import type {
  ArtifactAvailability,
  ExtensionOffer,
  ExtensionSummary,
  SiteSummary,
} from "@mixengine/api";

import { toggleMode } from "./serviceStateLabel";

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

/** What a row has sent and not yet heard back about — the Dashboard's in-flight rule, on Add-ons. */
export type PendingAction = "start" | "stop" | "turnOn" | "turnOff";

/** The word a row shows while its action is in flight. A lookup rather than `${action}ing`, for the
 *  Dashboard's reason: a key built by concatenation is a key nobody can grep for. */
export function pendingLabelKey(action: PendingAction): "mixengine.extensions.starting" | "mixengine.extensions.stopping" {
  return action === "start" || action === "turnOn" ? "mixengine.extensions.starting" : "mixengine.extensions.stopping";
}

/** Whether a row is between states: an action from it is still in flight, or the daemon says its
 *  service is `starting`, `stopping` or `restarting`. A start can take twenty seconds, and a row that
 *  looks idle all the while reads as a press that did nothing. */
export function rowMoving(
  row: ExtensionSummary,
  serviceState: string | null | undefined,
  pending: PendingAction | undefined,
): boolean {
  if (pending !== undefined) return true;
  return row.kind === "service" && toggleMode(serviceState, false) === "moving";
}

/** The service states after one daemon message. Only `service_state_changed` moves a state, and it
 *  names the field `service` (see `daemonState.applyEvent`). Anything else hands back the same
 *  table, so a stream of job progress does not redraw the screen. */
export function serviceStatesAfter(
  states: Record<string, string | null | undefined>,
  raw: string,
): Record<string, string | null | undefined> {
  let event: { type?: unknown; service?: unknown; to?: unknown };
  try {
    event = JSON.parse(raw) as typeof event;
  } catch {
    return states;
  }
  if (event.type !== "service_state_changed") return states;
  if (typeof event.service !== "string" || typeof event.to !== "string") return states;
  return { ...states, [event.service]: event.to };
}
