import { describe, expect, it } from "vitest";
import { parseSession } from "../src/shell/session";
import { MODULES } from "../src/shell/registry";
import { parseDbTabState } from "../src/modules/db/tabState";
import { parseRestTabState } from "../src/modules/rest/tabState";
import { parseTerminalTabState } from "../src/modules/terminal/tabState";
import { parseToolsTabState } from "../src/modules/tools/tabState";
import { TOOLS } from "../src/modules/tools/registry";
import { MODULE_IDS, THEMES, storageFor } from "./args.mjs";
import { SCENES } from "./scenes.mjs";

/* The one test that fails when a redesign changes how a tab is restored — before anyone runs the
   capture and wonders why a scene opens on the wrong screen. */
const PARSERS = {
  // The MixEngine tab restores nothing: it always opens on Dashboard, and a scene that wants another
  // screen clicks its way there. Whatever a scene put in its slot would be ignored, so it puts none.
  mixengine: (state) => state,
  db: parseDbTabState,
  rest: parseRestTabState,
  terminal: parseTerminalTabState,
  tools: parseToolsTabState,
};

describe("scenes", () => {
  it("are the six the spec names, in order", () => {
    expect(SCENES.map((scene) => scene.id)).toEqual(["hero", "sites", "database", "rest", "terminal", "tools"]);
  });

  it("seed the module ids the registry has", () => {
    expect([...MODULE_IDS].sort()).toEqual(MODULES.map((module) => module.id).sort());
  });

  for (const scene of SCENES) {
    for (const theme of THEMES) {
      it(`${scene.id}-${theme}: the session restores its tab, and its module accepts the slot`, () => {
        const storage = storageFor(scene, theme);
        const session = parseSession(storage["mixlab-session"], MODULE_IDS);
        expect(session?.tabs).toHaveLength(1);
        expect(session?.activeId).toBe(session?.tabs[0].id);
        expect(PARSERS[scene.moduleId](session?.tabs[0].state)).toEqual(scene.state);
      });
    }
  }

  it("open a tool the tools registry has", () => {
    const tools = SCENES.filter((scene) => scene.moduleId === "tools");
    for (const scene of tools) expect(TOOLS.map((tool) => tool.id)).toContain(scene.state.toolId);
  });

  it("carry a headline and a description for the frame", () => {
    for (const scene of SCENES) {
      expect(scene.headline.length).toBeGreaterThan(0);
      expect(scene.description.length).toBeGreaterThan(0);
    }
  });
});
