import { lazy } from "react";

import { EngineIcon } from "../../icons";
import type { ModuleDefinition } from "../../shell/module";

/* Loaded when a tab of this module is first opened, not at start-up — for the same reason as the
   other four modules. The icon and label are eager: they are on the tab strip before any tab of
   this kind exists. */
/** MixEngine: the local web dev environment running on this machine, managed from here. */
export const mixengineModule: ModuleDefinition = {
  id: "mixengine",
  labelKey: "app.moduleMixEngine",
  Icon: EngineIcon,
  defaultTitleKey: "mixengine.newTabTitle",
  /* One tab is all. This tab is the control panel of **one** daemon on **one** machine: opening a
     second one shows nothing more, just two copies of the same state side by side, and either
     could be the one the user read last time. Quite unlike the other four modules, where each tab
     is a connection, a session, a request. */
  singleTab: true,
  Tab: lazy(() => import("./MixEngineTab")),
  /* Loaded when the tray's frame first draws this section — T168, T192. The main window never
     loads it. */
  TraySection: lazy(() => import("./tray/TraySection")),
};
