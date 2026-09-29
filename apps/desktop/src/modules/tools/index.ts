import { lazy } from "react";

import type { ModuleDefinition } from "../../shell/module";
import { ToolsIcon } from "../../icons";
import { snippetsSyncable } from "./tools/cheatsheet/snippetsStore";

/* Loaded when a tab of this module is first opened, not at start-up — for the same reason as the
   other three modules. The icon and label are eager: they are on the tab strip before any tab of
   this kind exists. */
/** Tools: the small utilities a dev reaches for while working with a DB, an API or a server. */
export const toolsModule: ModuleDefinition = {
  id: "tools",
  labelKey: "app.moduleTools",
  Icon: ToolsIcon,
  defaultTitleKey: "toolbox.newTabTitle",
  Tab: lazy(() => import("./ToolsTab")),
  syncable: [snippetsSyncable],
};
