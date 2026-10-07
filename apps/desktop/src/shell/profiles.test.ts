import { describe, expect, it } from "vitest";
import { MODULES, MODULE_PRESETS } from "./registry";
import {
  MODULES_PRESET_KEY,
  MODULES_STORAGE_KEY,
  PRESETS_BEFORE_T203,
  defaultModuleId,
  normalizeModules,
  presetMark,
  presetOf,
  resolveStoredModules,
  visibleModules,
  withModule,
  type ShellStorage,
} from "./profiles";

const KNOWN = MODULES.map((m) => m.id);

/** A `localStorage` with nothing in it but what a test put there. */
function storageOf(entries: Record<string, string>): ShellStorage {
  const map = new Map(Object.entries(entries));
  return {
    getItem: (key) => map.get(key) ?? null,
    setItem: (key, value) => void map.set(key, value),
  };
}

describe("normalizeModules", () => {
  it("keeps a set this build knows, as it was given", () => {
    expect(normalizeModules(["db", "mixengine"], KNOWN)).toEqual(["db", "mixengine"]);
  });

  it("drops an id this build does not have", () => {
    expect(normalizeModules(["db", "gopher"], KNOWN)).toEqual(["db"]);
  });

  it("collapses a repeated id", () => {
    expect(normalizeModules(["db", "db"], KNOWN)).toEqual(["db"]);
  });

  /* A window with no modules in it is not a window. Empty is the same answer as unreadable, so the
     caller has one branch to write and one to test. */
  it("answers null for anything that leaves nothing to draw", () => {
    expect(normalizeModules([], KNOWN)).toBeNull();
    expect(normalizeModules(["gopher"], KNOWN)).toBeNull();
    expect(normalizeModules(null, KNOWN)).toBeNull();
    expect(normalizeModules("db", KNOWN)).toBeNull();
    expect(normalizeModules({ db: true }, KNOWN)).toBeNull();
    expect(normalizeModules([1, 2], KNOWN)).toBeNull();
  });
});

describe("presetOf", () => {
  it("recognises each preset from its set, whatever order it arrives in", () => {
    expect(presetOf(MODULE_PRESETS.mixengine)).toBe("mixengine");
    expect(presetOf([...MODULE_PRESETS.everything].reverse())).toBe("everything");
    expect(presetOf([...MODULE_PRESETS.databaseTools].reverse())).toBe("databaseTools");
  });

  it("answers null for a set of someone's own", () => {
    expect(presetOf(["mixengine", "terminal"])).toBeNull();
  });
});

describe("visibleModules", () => {
  it("draws in the registry's order, not the order it was handed", () => {
    const ids = visibleModules(["tools", "mixengine", "db"]).map((m) => m.id);
    expect(ids).toEqual(MODULES.filter((m) => ids.includes(m.id)).map((m) => m.id));
    expect(ids[0]).toBe("mixengine");
  });

  it("ignores an id this build does not have", () => {
    expect(visibleModules(["mixengine", "gopher"]).map((m) => m.id)).toEqual(["mixengine"]);
  });
});

describe("withModule", () => {
  it("appends an id the set does not have", () => {
    expect(withModule(["mixengine"], "db", KNOWN)).toEqual(["mixengine", "db"]);
  });

  /* The very same array, not an equal one: it is how the caller knows there is nothing to write
     and nothing to say in a notice. */
  it("answers the set it was given when the id is already in it", () => {
    const enabled = ["mixengine", "db"];
    expect(withModule(enabled, "db", KNOWN)).toBe(enabled);
  });

  it("answers the set it was given for an id this build does not have", () => {
    const enabled = ["mixengine"];
    expect(withModule(enabled, "gopher", KNOWN)).toBe(enabled);
  });

  it("never touches the set it was given", () => {
    const enabled = ["mixengine"];
    withModule(enabled, "db", KNOWN);
    expect(enabled).toEqual(["mixengine"]);
  });

  /* Order is the registry's, always — T108's D1 — so the stored set is free to carry any order at
     all and `visibleModules` still draws MixEngine first. */
  it("appends rather than sorting, because nothing reads an order out of the set", () => {
    expect(visibleModules(withModule(["db"], "mixengine", KNOWN)).map((m) => m.id)).toEqual([
      "mixengine",
      "db",
    ]);
  });
});

/* Named per preset rather than positional, on purpose: with T109 the default is implicit in the
   registry's order, and a module inserted at the head of `MODULES` would move it silently. These
   three cases are what say so out loud. */
describe("defaultModuleId", () => {
  it("opens MixEngine for the profiles that lead with it", () => {
    expect(defaultModuleId(visibleModules(MODULE_PRESETS.mixengine))).toBe("mixengine");
    expect(defaultModuleId(visibleModules(MODULE_PRESETS.everything))).toBe("mixengine");
  });

  it("opens the database client for the toolbox profile", () => {
    expect(defaultModuleId(visibleModules(MODULE_PRESETS.databaseTools))).toBe("db");
  });

  /* The *first* visible module, not merely a visible one — `visibleModules` has already put the
     set into the registry's order by the time this sees it. */
  it("takes the first module the registry lists, whatever order the set arrived in", () => {
    expect(defaultModuleId(visibleModules(["terminal", "db"]))).toBe("db");
  });
});

/* The three questions of D3, in order. Only the fourth answer — "ask" — costs an IPC call, and it
   is the only one this cannot decide here; `null` is what it answers instead. */
describe("resolveStoredModules", () => {
  it("uses a stored set when there is one", () => {
    const storage = storageOf({ [MODULES_STORAGE_KEY]: JSON.stringify(["mixengine", "db"]) });
    expect(resolveStoredModules(storage, KNOWN)).toEqual(["mixengine", "db"]);
  });

  /* The window this task is most able to hurt: a MixLab install from before T108. It has a theme
     and a session and four modules' worth of saved work, and no `mixlab-modules` — and handing it
     the MixEngine preset would make its database tabs disappear on an upgrade. */
  it("gives a window that predates this setting everything", () => {
    for (const key of ["mixlab-session", "mixlab-theme", "mixlab-accent", "mixlab-lang"]) {
      expect(resolveStoredModules(storageOf({ [key]: "x" }), KNOWN)).toEqual(
        MODULE_PRESETS.everything,
      );
    }
  });

  it("treats an unusable stored value as no setting at all", () => {
    expect(resolveStoredModules(storageOf({ [MODULES_STORAGE_KEY]: "{" }), KNOWN)).toBeNull();
    expect(resolveStoredModules(storageOf({ [MODULES_STORAGE_KEY]: "[]" }), KNOWN)).toBeNull();
    expect(
      resolveStoredModules(storageOf({ [MODULES_STORAGE_KEY]: JSON.stringify(["gopher"]) }), KNOWN),
    ).toBeNull();
  });

  it("has nothing to go on in a profile that has never been used", () => {
    expect(resolveStoredModules(storageOf({}), KNOWN)).toBeNull();
  });

  /* An unusable value in a profile that has been used still lands on Everything: the two questions
     are asked in order, and the second one does not care why the first had no answer. */
  it("still answers everything when an unusable value sits beside a used profile", () => {
    const storage = storageOf({ [MODULES_STORAGE_KEY]: "{", "mixlab-session": "x" });
    expect(resolveStoredModules(storage, KNOWN)).toEqual(MODULE_PRESETS.everything);
  });
});

/* T203, D0. The mark beside the list: a preset id, `custom`, or absent on a home no T203 build has
   written yet. */
describe("resolveStoredModules with a preset mark", () => {
  const list = (ids: string[]) => JSON.stringify(ids);

  it("shows this build's composition of a marked preset", () => {
    const storage = storageOf({
      [MODULES_STORAGE_KEY]: list(PRESETS_BEFORE_T203.everything),
      [MODULES_PRESET_KEY]: "everything",
    });
    expect(resolveStoredModules(storage, KNOWN)).toEqual(MODULE_PRESETS.everything);
  });

  it("reads a list equal to a preset's composition before T203, with no mark, as that preset", () => {
    const storage = storageOf({ [MODULES_STORAGE_KEY]: list(PRESETS_BEFORE_T203.databaseTools) });
    expect(resolveStoredModules(storage, KNOWN)).toEqual(MODULE_PRESETS.databaseTools);
  });

  it("a set that equals an old preset but is marked custom stays custom", () => {
    const storage = storageOf({
      [MODULES_STORAGE_KEY]: list(PRESETS_BEFORE_T203.everything),
      [MODULES_PRESET_KEY]: "custom",
    });
    expect(resolveStoredModules(storage, KNOWN)).toEqual(PRESETS_BEFORE_T203.everything);
  });

  it("lets a list changed where the mark is unknown win over the mark", () => {
    const storage = storageOf({
      [MODULES_STORAGE_KEY]: list(["mixengine", "db"]),
      [MODULES_PRESET_KEY]: "everything",
    });
    expect(resolveStoredModules(storage, KNOWN)).toEqual(["mixengine", "db"]);
  });

  it("answers a mark that arrived before its list", () => {
    const storage = storageOf({ [MODULES_PRESET_KEY]: "mixengine" });
    expect(resolveStoredModules(storage, KNOWN)).toEqual(MODULE_PRESETS.mixengine);
  });

  it("ignores a mark that is not a preset", () => {
    const storage = storageOf({
      [MODULES_STORAGE_KEY]: list(["db"]),
      [MODULES_PRESET_KEY]: "quantum",
    });
    expect(resolveStoredModules(storage, KNOWN)).toEqual(["db"]);
  });
});

describe("presetMark", () => {
  it("is the preset a set is, or custom", () => {
    expect(presetMark(MODULE_PRESETS.everything)).toBe("everything");
    expect(presetMark(["db"])).toBe("custom");
  });
});
