/**
 * An `UpgradePlan` as sentences — T193, D7. What each entry means is the daemon's; this only picks
 * the sentence and fills in the names it sent.
 */
import type { UpgradeItem, UpgradePlan } from "@mixengine/api";

import type { TranslationKey } from "../../i18n";

export type Translate = (key: TranslationKey, params?: Record<string, string | number>) => string;

function itemLine(plan: UpgradePlan, item: UpgradeItem, t: Translate): string {
  const names = { subject: plan.subject, from: plan.from, to: plan.to };
  switch (item.item) {
    case "default":
      return t("mixengine.upgrade.default", names);
    case "site":
      return t("mixengine.upgrade.site", { ...names, site: item.site });
    case "extension_pool":
      return item.moves
        ? t("mixengine.upgrade.extensionPoolMoves", { ...names, pool: item.pool })
        : t("mixengine.upgrade.extensionPoolStays", {
            ...names,
            pool: item.pool,
            requires: item.requires ?? "",
          });
    case "dropped_extension":
      return t("mixengine.upgrade.droppedExtension", { ...names, name: item.name });
    case "pin":
      return t("mixengine.upgrade.pin", {
        ...names,
        project: item.project,
        pinFrom: item.from,
        pinTo: item.to,
      });
    case "manifest":
      return t("mixengine.upgrade.manifest", {
        ...names,
        project: item.project,
        path: item.path,
        constraint: item.constraint,
      });
    case "tool":
      return t("mixengine.upgrade.tool", { ...names, name: item.name });
    case "instance":
      return item.restarts
        ? t("mixengine.upgrade.instanceRestarts", { ...names, service: item.service })
        : t("mixengine.upgrade.instanceStopped", { ...names, service: item.service });
    case "front_end_restart":
      return t("mixengine.upgrade.frontEndRestart", { ...names, service: item.service });
    case "no_downgrade":
      return t("mixengine.upgrade.noDowngrade", { ...names, service: item.service });
  }
}

export function planLines(plan: UpgradePlan, t: Translate): string[] {
  return plan.entries.map((entry) => itemLine(plan, entry.item, t));
}

export function oldVersionLine(plan: UpgradePlan, t: Translate): string {
  const names = { subject: plan.subject, from: plan.from };
  switch (plan.old.state) {
    case "will_be_removed":
    case "removed":
      return t("mixengine.upgrade.willBeRemoved", names);
    case "will_be_kept":
    case "kept":
      return t("mixengine.upgrade.willBeKept", { ...names, because: plan.old.because.join("; ") });
  }
}
