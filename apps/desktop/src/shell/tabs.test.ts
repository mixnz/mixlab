import { describe, expect, it } from "vitest";
import {
  firstTabOfModule,
  openableModules,
  rebadgeTab,
  restateTab,
  retitleTab,
  tabIdAtOffset,
  type TabInfo,
} from "./tabs";
import { visibleModules } from "./profiles";
import { MODULE_PRESETS } from "./registry";

const TABS: TabInfo[] = [
  { id: "a", moduleId: "db", title: "Kết nối mới", badges: [] },
  { id: "b", moduleId: "rest", title: "Yêu cầu mới", badges: [] },
];

function tabOf(moduleId: string, id: string = moduleId): TabInfo {
  return { id, moduleId, title: "", badges: [] };
}

describe("retitleTab", () => {
  it("hands back the very same array when the title has not moved", () => {
    expect(retitleTab(TABS, "b", "Yêu cầu mới")).toBe(TABS);
  });

  it("hands back the very same array when no tab has that id", () => {
    expect(retitleTab(TABS, "zzz", "gì đó")).toBe(TABS);
  });

  it("renames the one tab and leaves its neighbour untouched", () => {
    const next = retitleTab(TABS, "b", "GET /users");
    expect(next).not.toBe(TABS);
    expect(next[1].title).toBe("GET /users");
    expect(next[0]).toBe(TABS[0]);
  });
});

describe("rebadgeTab", () => {
  it("hands back the very same array when the badges are the same list", () => {
    expect(rebadgeTab(TABS, "a", TABS[0].badges)).toBe(TABS);
  });

  it("hands back the very same array when no tab has that id", () => {
    expect(rebadgeTab(TABS, "zzz", [])).toBe(TABS);
  });

  it("replaces the badges of the one tab", () => {
    const badges = [{ id: "readOnly", icon: null, label: "Chỉ đọc" }];
    const next = rebadgeTab(TABS, "a", badges);
    expect(next).not.toBe(TABS);
    expect(next[0].badges).toBe(badges);
    expect(next[1]).toBe(TABS[1]);
  });
});

describe("restateTab", () => {
  it("hands back the very same array when the state is the same object", () => {
    const state = { savedId: "c-1" };
    const withState = restateTab(TABS, "a", state);
    expect(restateTab(withState, "a", state)).toBe(withState);
  });

  it("hands back the very same array when there was no state and none is given", () => {
    expect(restateTab(TABS, "a", undefined)).toBe(TABS);
  });

  it("hands back the very same array when no tab has that id", () => {
    expect(restateTab(TABS, "zzz", { savedId: "c-1" })).toBe(TABS);
  });

  it("replaces the state of the one tab and leaves its neighbour alone", () => {
    const state = { savedId: "c-1" };
    const next = restateTab(TABS, "a", state);
    expect(next).not.toBe(TABS);
    expect(next[0].state).toBe(state);
    expect(next[1]).toBe(TABS[1]);
  });

  it("forgets a tab's state when handed undefined", () => {
    const withState = restateTab(TABS, "a", { savedId: "c-1" });
    expect(restateTab(withState, "a", undefined)[0].state).toBeUndefined();
  });

  /* Two equal objects are two objects. The bail-out is identity on purpose — see the note at the
     top of `tabs.ts` — and a module that builds a fresh state object every render is the caller
     that has to change, not this. */
  it("does not compare state by value", () => {
    const withState = restateTab(TABS, "a", { savedId: "c-1" });
    expect(restateTab(withState, "a", { savedId: "c-1" })).not.toBe(withState);
  });
});

/* What the `[+]` button, the menu behind it and the number chords are still allowed to open.

   The question is a module's own and never a count of modules: a window drawing nothing but the
   terminal is a window where a second tab is the whole point, and one drawing nothing but
   MixEngine is a window where a second tab is a second copy of one daemon's control panel. */
describe("openableModules", () => {
  const everything = visibleModules(MODULE_PRESETS.everything);
  const mixengineOnly = visibleModules(MODULE_PRESETS.mixengine);

  it("offers every visible module while the window holds no tabs at all", () => {
    expect(openableModules(everything, []).map((m) => m.id)).toEqual(everything.map((m) => m.id));
  });

  it("drops a module that says one tab of it is all there is, once it has one", () => {
    expect(openableModules(everything, [tabOf("mixengine")]).map((m) => m.id)).toEqual([
      "db",
      "rest",
      "terminal",
      "tools",
      "tunnel",
    ]);
  });

  it("leaves a MixEngine-only window with nothing left to open", () => {
    expect(openableModules(mixengineOnly, [tabOf("mixengine")])).toEqual([]);
  });

  /* A connection, a session, a request — a tab each, and the module is no less openable for having
     three of them. This is the case a rule written on `visible.length` would have broken. */
  it("keeps a module whose tabs are its whole point, however many are open", () => {
    const terminalOnly = visibleModules(["terminal"]);
    const tabs = [tabOf("terminal", "a"), tabOf("terminal", "b"), tabOf("terminal", "c")];
    expect(openableModules(terminalOnly, tabs).map((m) => m.id)).toEqual(["terminal"]);
  });

  it("says nothing about a tab of a module this window is not drawing", () => {
    expect(openableModules(mixengineOnly, [tabOf("db")]).map((m) => m.id)).toEqual(["mixengine"]);
  });
});

/* Where the chord of a module that is already open goes. */
describe("firstTabOfModule", () => {
  it("finds the tab of that module", () => {
    expect(firstTabOfModule(TABS, "rest")).toBe("b");
  });

  it("says nothing when the module has no tab", () => {
    expect(firstTabOfModule(TABS, "mixengine")).toBeUndefined();
    expect(firstTabOfModule([], "mixengine")).toBeUndefined();
  });

  /* "First" and not "the": a `singleTab` module cannot be opened twice, but a session written
     before it said so can hold two, and the chord has to land somewhere either way. */
  it("takes the one nearest the front of the strip", () => {
    const tabs = [tabOf("db"), tabOf("mixengine", "one"), tabOf("mixengine", "two")];
    expect(firstTabOfModule(tabs, "mixengine")).toBe("one");
  });
});

describe("tabIdAtOffset", () => {
  it("walks one along the strip, in either direction", () => {
    expect(tabIdAtOffset(TABS, "a", 1)).toBe("b");
    expect(tabIdAtOffset(TABS, "b", -1)).toBe("a");
  });

  it("wraps round both ends", () => {
    expect(tabIdAtOffset(TABS, "b", 1)).toBe("a");
    expect(tabIdAtOffset(TABS, "a", -1)).toBe("b");
  });

  it("stays put when there is nowhere else to go", () => {
    expect(tabIdAtOffset([TABS[0]], "a", 1)).toBe("a");
    expect(tabIdAtOffset([], "a", 1)).toBe("a");
  });

  it("stays put on an id the strip does not hold", () => {
    expect(tabIdAtOffset(TABS, "zzz", 1)).toBe("zzz");
  });
});
