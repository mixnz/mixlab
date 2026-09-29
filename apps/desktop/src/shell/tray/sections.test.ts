import { describe, expect, it } from "vitest";
import { visibleModules } from "../profiles";
import { MODULES, MODULE_PRESETS } from "../registry";
import { traySections } from "./sections";

/* Whether there is a panel at all is `traySections(...).length > 0` — the backend's `panel` flag,
   which decides whether a click on the icon opens the panel or the main window (ADR 0058). */
describe("the tray's sections", () => {
  it("come from MixEngine when it is visible", () => {
    expect(traySections(visibleModules(MODULE_PRESETS.mixengine)).map((m) => m.id)).toEqual(["mixengine"]);
  });

  it("are none on the Database tools preset, so the icon opens the window instead", () => {
    expect(traySections(visibleModules(MODULE_PRESETS.databaseTools))).toEqual([]);
  });

  it("follow the registry's order", () => {
    const ids = traySections(visibleModules(MODULE_PRESETS.everything)).map((m) => m.id);
    expect(ids).toEqual(MODULES.map((m) => m.id).filter((id) => ids.includes(id)));
  });
});
