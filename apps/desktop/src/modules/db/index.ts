import { lazy } from "react";

import type { ModuleDefinition } from "../../shell/module";
import { DatabaseGenericIcon } from "../../icons";
import ToolsSection from "./components/ToolsSection";
import { DB_SHORTCUTS } from "./shortcuts";
import { querySnippetsSyncable } from "./querySnippets";
import { connectionSecretsSyncable, connectionsSyncable } from "./sync";

/* Loaded when a tab of this module is first opened, not at launch. The workspace behind it is the
   heaviest thing in the bundle — CodeMirror here, xterm in the terminal — and a launch that parses
   all three to show one is paying for two nobody asked for. Everything else in this file stays
   eager: the icon and the label are on the tab strip before any tab of this kind exists. */
/** Database: the module MixLab started as. */
export const dbModule: ModuleDefinition = {
  id: "db",
  labelKey: "app.moduleDatabase",
  Icon: DatabaseGenericIcon,
  defaultTitleKey: "app.newConnectionTitle",
  Tab: lazy(() => import("./DbTab")),
  /* Named after the module, like every other pane in that list — `labelKey` again rather than a
     second string saying the same word, so the pane and the `[+]` menu cannot drift apart.

     It was called "Dump tools" and wore a spanner, which read as one of the dialog's own errands
     sitting between two modules' panes rather than as the database module's. The dump tools are
     still all that is in there; they are a heading inside the pane now. */
  settings: { labelKey: "app.moduleDatabase", Icon: DatabaseGenericIcon, Section: ToolsSection },
  shortcuts: DB_SHORTCUTS,
  syncable: [connectionsSyncable, connectionSecretsSyncable, querySnippetsSyncable],
};
