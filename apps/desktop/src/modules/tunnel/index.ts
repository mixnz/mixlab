import { lazy } from "react";

import type { ModuleDefinition } from "../../shell/module";
import { GlobeIcon } from "../../icons";
import TunnelSettings from "./components/TunnelSettings";

/* Loaded when the tab is first opened, as the other modules are; the icon and the label are eager,
   because they are on the tab strip before any tab of this kind exists. */
/** Tunnel: an address on this machine, shared on the internet through a Cloudflare quick tunnel —
 *  T203a. Needs no MixEngine. */
export const tunnelModule: ModuleDefinition = {
  id: "tunnel",
  labelKey: "app.moduleTunnel",
  Icon: GlobeIcon,
  defaultTitleKey: "tunnelTab.newTabTitle",
  Tab: lazy(() => import("./TunnelTab")),
  singleTab: true,
  settings: { labelKey: "tunnelTab.settingsTitle", Icon: GlobeIcon, Section: TunnelSettings },
};
