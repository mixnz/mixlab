import type { ComponentType } from "react";
import {
  AddOnIcon,
  BlueprintIcon,
  DashboardIcon,
  FolderIcon,
  GlobeIcon,
  LockIcon,
  LogIcon,
  PackageIcon,
  PulseIcon,
  PuzzleIcon,
  ServerIcon,
  SlidersIcon,
  type IconProps,
} from "../../../../icons";
import { useTranslation, type TranslationKey } from "../../../../i18n";
import type { MixEngineScreen } from "../../tabState";
import styles from "./Sidebar.module.css";

/**
 * Twelve screens, five groups — T119, decision D11.
 *
 * **Ordered by what people do, not by the shape of the API.** Eleven flat items have to be read in
 * full before you find anything, and two of them used to have almost the same name — T118 renamed
 * the *Extensions* label to *Add-ons* so PHP Extensions could stand under its own name.
 *
 * **Static headings, not collapsible.** A collapsible group is a place for exactly the thing people
 * are looking for to hide in, and a collapsed state is something to store, migrate and then get
 * wrong. The list has twelve items; it fits on the screen.
 *
 * **`projects` and `metrics` are not among the 9 screens `client-surface.md` lists**, and the
 * reason is the same as when this table was flat: `client-surface.md` assumes a client asks
 * `project.list` for exactly one dropdown, whereas `project.*` already has enough methods (`list,
 * create, show, update, delete, export`) for a management screen — and Sites is unusable without a
 * project (Decision D4, `docs/specs/2026-09-06-mixengine-runtimes-services-logs-design.md`).
 * `metrics` is data of a different shape: a 24-hour chart, not a row in the service table (D1,
 * `docs/specs/2026-09-07-mixengine-metrics-settings-design.md`).
 *
 * **Neither is `phpExtensions`** — `client-surface.md` puts the extension toggles inside the
 * Packages screen, and they are still there: the same component, drawn in two places (T118). The
 * *Environment* group is the second place, because that is where people go looking for it.
 *
 * This table is still the **only** place that decides the order, just as when it was flat.
 */
const GROUPS: readonly {
  labelKey: TranslationKey | null;
  items: readonly { screen: MixEngineScreen; labelKey: TranslationKey; Icon: ComponentType<IconProps> }[];
}[] = [
  {
    labelKey: "mixengine.sidebar.groupOverview",
    items: [
      { screen: "dashboard", labelKey: "mixengine.sidebar.dashboard", Icon: DashboardIcon },
      { screen: "metrics", labelKey: "mixengine.sidebar.metrics", Icon: PulseIcon },
      { screen: "logs", labelKey: "mixengine.sidebar.logs", Icon: LogIcon },
    ],
  },
  {
    labelKey: "mixengine.sidebar.groupWebsites",
    items: [
      { screen: "projects", labelKey: "mixengine.sidebar.projects", Icon: FolderIcon },
      { screen: "sites", labelKey: "mixengine.sidebar.sites", Icon: GlobeIcon },
      { screen: "domains", labelKey: "mixengine.sidebar.domains", Icon: LockIcon },
    ],
  },
  {
    labelKey: "mixengine.sidebar.groupEnvironment",
    items: [
      { screen: "packages", labelKey: "mixengine.sidebar.packages", Icon: PackageIcon },
      { screen: "phpExtensions", labelKey: "mixengine.sidebar.phpExtensions", Icon: PuzzleIcon },
      { screen: "servicesDetail", labelKey: "mixengine.sidebar.servicesDetail", Icon: ServerIcon },
    ],
  },
  {
    labelKey: "mixengine.sidebar.groupLibrary",
    items: [
      { screen: "blueprints", labelKey: "mixengine.sidebar.blueprints", Icon: BlueprintIcon },
      { screen: "extensions", labelKey: "mixengine.sidebar.extensions", Icon: AddOnIcon },
    ],
  },
  // No heading, and `margin-top: auto` in the CSS pushes it to the bottom: Settings belongs to no
  // group and is the item people look for where it has always been.
  {
    labelKey: null,
    items: [{ screen: "settings", labelKey: "mixengine.sidebar.settings", Icon: SlidersIcon }],
  },
];

export default function Sidebar({
  screen,
  onSelect,
}: {
  screen: MixEngineScreen;
  onSelect: (screen: MixEngineScreen) => void;
}) {
  const { t } = useTranslation();

  return (
    <nav className={styles.sidebar} aria-label={t("mixengine.sidebar.label")}>
      {GROUPS.map((group, index) => (
        <div key={group.labelKey ?? `group-${index}`} className={styles.group}>
          {group.labelKey !== null && <h3 className={styles.heading}>{t(group.labelKey)}</h3>}
          {group.items.map((item) => (
            <button
              key={item.labelKey}
              type="button"
              className={styles.item}
              aria-current={item.screen === screen ? "page" : undefined}
              // The screen's id, for whatever has to reach one without reading its label: the tab
              // restores no screen, so the screenshot scenes click their way to one.
              data-screen={item.screen}
              onClick={() => onSelect(item.screen)}
            >
              <item.Icon size={16} className={styles.icon} />
              {t(item.labelKey)}
            </button>
          ))}
        </div>
      ))}
    </nav>
  );
}
