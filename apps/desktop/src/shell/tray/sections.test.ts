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

  /* T203, D5: Tunnel lends a section whether or not a tunnel runs, so the Database tools preset,
     which lent none before it, now opens the panel; the panel's header still has Open MixLab. */
  it("come from Tunnel on the Database tools preset, so the icon opens the panel", () => {
    expect(traySections(visibleModules(MODULE_PRESETS.databaseTools)).map((m) => m.id)).toEqual(["tunnel"]);
  });

  it("follow the registry's order", () => {
    const ids = traySections(visibleModules(MODULE_PRESETS.everything)).map((m) => m.id);
    expect(ids).toEqual(MODULES.map((m) => m.id).filter((id) => ids.includes(id)));
  });
});
