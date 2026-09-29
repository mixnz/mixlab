/**
 * Which screen of the MixEngine tab is on screen.
 *
 * **Not persisted.** It used to be kept in the tab's session slot and restored on the next launch;
 * the tab now always opens on Dashboard, and on Dashboard again every time the daemon comes up —
 * see `MixEngineTab.tsx`. The slot an older build wrote is cleared the first time a screen is
 * chosen.
 */
export type MixEngineScreen =
  | "dashboard"
  | "projects"
  | "sites"
  | "domains"
  | "packages"
  | "phpExtensions"
  | "servicesDetail"
  | "logs"
  | "blueprints"
  | "extensions"
  | "metrics"
  | "settings";
