import { afterEach, describe, expect, it, vi } from "vitest";
import type { TabInfo } from "./tabs";
import { parseSession, writeSession } from "./session";

const MODULE_IDS = ["db", "rest", "terminal"];

const SESSION = {
  tabs: [
    { id: "a", moduleId: "db", title: "demo@192.168.50.86" },
    { id: "b", moduleId: "terminal", title: "localhost" },
  ],
  activeId: "b",
};

const stored = (value: unknown) => JSON.stringify(value);

describe("parseSession", () => {
  it("reads back what was written", () => {
    expect(parseSession(stored(SESSION), MODULE_IDS)).toEqual(SESSION);
  });

  it("has nothing to say on a first launch", () => {
    expect(parseSession(null, MODULE_IDS)).toBeNull();
  });

  /* Everything below is a string some other version of the app wrote, so none of it is trusted —
     see the note on the function. Each of these once meant a tab bar that threw on the way up. */
  it("gives up on anything that is not a session", () => {
    expect(parseSession("{ half-written", MODULE_IDS)).toBeNull();
    expect(parseSession(stored(null), MODULE_IDS)).toBeNull();
    expect(parseSession(stored("a string"), MODULE_IDS)).toBeNull();
    expect(parseSession(stored({ activeId: "a" }), MODULE_IDS)).toBeNull();
    expect(parseSession(stored({ tabs: "not an array" }), MODULE_IDS)).toBeNull();
  });

  it("drops the tabs it cannot draw and keeps the rest", () => {
    const session = {
      tabs: [
        { id: "a", moduleId: "db", title: "demo" },
        // A module taken out of the registry since this was written.
        { id: "b", moduleId: "gopher", title: "gopher://" },
        { id: "c", title: "no module at all" },
        { id: "", moduleId: "rest", title: "no id" },
        null,
      ],
      activeId: "a",
    };
    expect(parseSession(stored(session), MODULE_IDS)).toEqual({
      tabs: [{ id: "a", moduleId: "db", title: "demo" }],
      activeId: "a",
    });
  });

  it("gives up when nothing is left to draw", () => {
    const session = { tabs: [{ id: "a", moduleId: "gopher", title: "x" }], activeId: "a" };
    expect(parseSession(stored(session), MODULE_IDS)).toBeNull();
  });

  it("falls back to the last tab when the active one did not survive", () => {
    const session = {
      tabs: [
        { id: "a", moduleId: "db", title: "demo" },
        { id: "b", moduleId: "rest", title: "GET /users" },
      ],
      activeId: "gone",
    };
    expect(parseSession(stored(session), MODULE_IDS)?.activeId).toBe("b");
  });

  it("keeps nothing a stored tab was carrying beyond the three fields", () => {
    // Badges hold React elements and are never written; a session from a version that did write
    // something extra must not put it back into the tab bar's state.
    const session = { tabs: [{ ...SESSION.tabs[0], badges: ["stale"] }], activeId: "a" };
    expect(parseSession(stored(session), MODULE_IDS)?.tabs[0]).toEqual(SESSION.tabs[0]);
  });

  /* The one thing a stored tab carries that the shell does not understand. It goes through
     untouched — only the module that wrote it knows what shape it should have, and a shell that
     could tell a good `savedId` from a bad one is a shell that knows the database module exists. */
  it("carries a tab's module state through untouched", () => {
    const state = { savedId: "c-1", nested: { openIds: ["r-1", "r-2"], activeId: null } };
    const session = { tabs: [{ ...SESSION.tabs[0], state }], activeId: "a" };
    expect(parseSession(stored(session), MODULE_IDS)?.tabs[0].state).toEqual(state);
  });

  it("reads a session written before there was any module state", () => {
    expect(parseSession(stored(SESSION), MODULE_IDS)?.tabs[0].state).toBeUndefined();
  });

  it("passes rubbish state through rather than dropping the tab", () => {
    // Validation lives in the module's own `parseXTabState`, which will make `null` of these. A
    // tab is still a tab, and its neighbour has nothing to do with what it was carrying.
    for (const state of [42, "nonsense", null, []]) {
      const session = {
        tabs: [{ ...SESSION.tabs[0], state }, SESSION.tabs[1]],
        activeId: "b",
      };
      const parsed = parseSession(stored(session), MODULE_IDS);
      expect(parsed?.tabs).toHaveLength(2);
      expect(parsed?.tabs[0].state).toEqual(state);
    }
  });

  it("takes a dropped tab's state away with it", () => {
    const session = {
      tabs: [{ id: "z", moduleId: "gopher", title: "x", state: { savedId: "c-1" } }, SESSION.tabs[0]],
      activeId: "a",
    };
    expect(parseSession(stored(session), MODULE_IDS)?.tabs).toEqual([SESSION.tabs[0]]);
  });

  /* T108: a tab of a module the profile hides is dropped on the way in, exactly as a tab of a
     module that no longer exists is. One code path, one behaviour — `readSession` is simply handed
     the visible ids instead of every id. */
  it("drops a tab whose module is hidden", () => {
    const raw = JSON.stringify({
      tabs: [
        { id: "a", moduleId: "mixengine", title: "Dashboard" },
        { id: "b", moduleId: "db", title: "MariaDB" },
      ],
      activeId: "b",
    });
    const session = parseSession(raw, ["mixengine"]);
    expect(session?.tabs.map((tab) => tab.moduleId)).toEqual(["mixengine"]);
    expect(session?.activeId).toBe("a");
  });
});

/** A `localStorage` that can be told to refuse, which is the case worth testing. */
function fakeStorage(setItem?: () => never) {
  const store = new Map<string, string>();
  return {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: setItem ?? ((key: string, value: string) => void store.set(key, value)),
    removeItem: (key: string) => void store.delete(key),
    read: (key: string) => store.get(key) ?? null,
    seed: (key: string, value: string) => void store.set(key, value),
  };
}

afterEach(() => vi.unstubAllGlobals());

describe("writeSession", () => {
  const TABS: TabInfo[] = [
    { id: "a", moduleId: "db", title: "demo", badges: [], state: { savedId: "c-1" } },
    { id: "b", moduleId: "terminal", title: "localhost", badges: [] },
  ];

  it("stores the tabs, without the parts that are not the session's", () => {
    const storage = fakeStorage();
    vi.stubGlobal("localStorage", storage);

    writeSession(TABS, "b");
    const written = JSON.parse(storage.read("mixlab-session") ?? "null");
    expect(written).toEqual({
      tabs: [
        { id: "a", moduleId: "db", title: "demo", state: { savedId: "c-1" } },
        { id: "b", moduleId: "terminal", title: "localhost" },
      ],
      activeId: "b",
    });
    // Badges are worked out afresh each launch, so storing them would only mean showing a stale
    // one until the pane caught up.
    expect(written.tabs.some((tab: Record<string, unknown>) => "badges" in tab)).toBe(false);
  });

  it("survives a storage that refuses to store", () => {
    // `QuotaExceededError` once the origin is full, which a tab with a large `state` is enough
    // for. Uncaught, this comes out of an effect that runs on every tab and badge change — so it
    // does not fail the write, it fails the window, repeatedly.
    const storage = fakeStorage(() => {
      throw new DOMException("full", "QuotaExceededError");
    });
    vi.stubGlobal("localStorage", storage);

    expect(() => writeSession(TABS, "b")).not.toThrow();
  });

  it("survives a module slot that is not JSON", () => {
    const storage = fakeStorage();
    vi.stubGlobal("localStorage", storage);

    const cycle: Record<string, unknown> = {};
    cycle.self = cycle;
    const tabs: TabInfo[] = [{ id: "a", moduleId: "db", title: "demo", badges: [], state: cycle }];
    expect(() => writeSession(tabs, "a")).not.toThrow();

    // And `BigInt`, which `JSON.stringify` refuses outright rather than looping on.
    const big: TabInfo[] = [
      { id: "a", moduleId: "db", title: "demo", badges: [], state: { n: 1n } },
    ];
    expect(() => writeSession(big, "a")).not.toThrow();
  });

  it("leaves the older session where it is when it cannot write a newer one", () => {
    // An older session is better than none: the alternative is a launch that opens a blank tab
    // because the launch before it ran out of quota once.
    const storage = fakeStorage();
    vi.stubGlobal("localStorage", storage);
    storage.seed("mixlab-session", '{"tabs":[],"activeId":"a"}');

    const cycle: Record<string, unknown> = {};
    cycle.self = cycle;
    writeSession([{ id: "a", moduleId: "db", title: "d", badges: [], state: cycle }], "a");
    expect(storage.read("mixlab-session")).toBe('{"tabs":[],"activeId":"a"}');
  });
});
