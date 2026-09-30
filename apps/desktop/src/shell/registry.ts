import type { SyncableCollection } from "../core/syncCollection";
import type { ModuleDefinition } from "./module";
import { preferencesSyncable } from "./preferencesSync";
import { dbModule } from "../modules/db";
import { mixengineModule } from "../modules/mixengine";
import { restModule } from "../modules/rest";
import { terminalModule } from "../modules/terminal";
import { toolsModule } from "../modules/tools";

/** Every module the app can open a tab of. Adding one is a line here — and this is the only file
 *  outside `src/modules/` that names any of them.
 *
 *  **The order is the app's order.** The `[+]` menu, `Ctrl/Cmd+1 … N` and the Settings dialog's
 *  pane column all draw the visible modules in this order and never in the order a stored setting
 *  happens to carry — see `shell/profiles.ts`. MixEngine leads because this is MixEngine's window;
 *  the four that follow are in the order the standalone client listed them.
 *
 *  **The first entry a profile shows is the tab it opens by default** — T109. There is no constant
 *  for it: `defaultModuleId` is a filter's first element, so the default is a module this list has
 *  by construction, and a preset that hides MixEngine leads with whatever it does show. */
export const MODULES: ModuleDefinition[] = [
  mixengineModule,
  dbModule,
  restModule,
  terminalModule,
  toolsModule,
];

/** Every collection the account screen can offer (D5), in the app's order. The one place a
 *  collection is joined to the app, the way `MODULES` is for tabs. */
export const SYNCABLE: SyncableCollection[] = [
  preferencesSyncable,
  ...MODULES.flatMap((m) => m.syncable ?? []),
];

/** The three answers to "what will you use MixLab for?" — T108, the desktop client design's D11. */
export type PresetId = "mixengine" | "everything" | "databaseTools";

/** In the order the first-run screen and the Settings pane offer them. */
export const PRESET_IDS: PresetId[] = ["mixengine", "everything", "databaseTools"];

/**
 * What each preset turns on.
 *
 * A **set**, written as an array: nothing reads an order out of it. Here rather than in
 * `shell/profiles.ts` because these are module ids written by hand, and this file is the one place
 * outside `src/modules/` allowed to write one — see `docs/standards/desktop/adding-a-module.md`.
 */
export const MODULE_PRESETS: Record<PresetId, string[]> = {
  mixengine: ["mixengine"],
  everything: MODULES.map((m) => m.id),
  databaseTools: ["db", "rest", "terminal", "tools"],
};

export function moduleById(id: string): ModuleDefinition {
  const found = MODULES.find((m) => m.id === id);
  // A tab's `moduleId` only ever comes from this list, so a miss is a programming error rather than
  // something to put in front of the user.
  if (!found) throw new Error(`Unknown module: ${id}`);
  return found;
}
