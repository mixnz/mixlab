import type { ModuleDefinition, TabBadge } from "./module";

/**
 * What the tab bar knows about one open tab, and the two ways a module changes it.
 *
 * Both updaters hand back **the array they were given** when the change is no change. That is not
 * an optimisation: a module reports its title from an effect, the shell passes a fresh callback on
 * every render, and a `setTabs` that always allocates turns the pair into a render loop that
 * React stops with "Maximum update depth exceeded". Bailing out here breaks the loop for every
 * module, including the ones not written yet.
 */
export interface TabInfo {
  id: string;
  /** Which module's workspace this tab holds. Looked up in the registry to render it. */
  moduleId: string;
  title: string;
  /** What the module asked the tab bar to show for it. Reported rather than worked out up here:
   *  only the module knows what its own state means — see {@link TabBadge}. */
  badges: TabBadge[];
  /** Whatever the module asked to have kept for this tab between launches. Opaque here: the shell
   *  carries it to `localStorage` and back and never reads it — see `shell/session.ts`. */
  state?: unknown;
}

/**
 * The visible modules a new tab can still be opened of.
 *
 * **Not a count of modules.** A window drawing one module is not a window that wants one tab: a
 * terminal-only window is one where a second tab is a second shell, and a db-only one where it is a
 * second connection. What decides it is the module's own `singleTab` — see {@link ModuleDefinition}
 * — and MixEngine is the one that sets it, because two of its tabs are two views of the one daemon
 * this window was opened to drive.
 *
 * Everything that offers a new tab reads this: the `[+]` button and its menu, `Ctrl/Cmd+T`, the
 * number chords, and the shortcut table that lists them. An empty result is a window with nothing
 * left to open, and the button is not drawn at all — the close button then stands alone, and
 * closing the last tab replaces it, which is how such a window reloads its module.
 *
 * `openIds` is the module of every tab on the strip, which is all of the tabs this reads.
 *
 * Derived on every render rather than kept: the visible list changes with the profile and the tabs
 * change with every click, and a remembered answer to a question this cheap is an answer that goes
 * out of date somewhere nobody is looking.
 */
export function openableModules(
  visible: ModuleDefinition[],
  openIds: readonly string[],
): ModuleDefinition[] {
  return visible.filter((module) => !module.singleTab || !openIds.includes(module.id));
}

/** Ids flattened to one string, for a memo that must not rebuild on a fresh array holding the same
 *  ids. Module ids hold no comma. {@link idsOf} reads them back. */
export function idsKey(ids: readonly string[]): string {
  return ids.join(",");
}

/** The ids {@link idsKey} flattened; none for an empty key. */
export function idsOf(key: string): string[] {
  return key === "" ? [] : key.split(",");
}

/**
 * The id of the first tab of `moduleId` on the strip, or `undefined` for a module with none.
 *
 * Where the number chord of a `singleTab` module goes once its tab exists: it opens one while there
 * is none and goes to that one afterwards, rather than falling silent — the same key a browser
 * gives a pinned tab.
 *
 * **First** and not *the*: the flag stops a second tab being opened, not a second tab existing. A
 * session written by a build older than the flag can hold two, and the chord has to land somewhere.
 */
export function firstTabOfModule(tabs: TabInfo[], moduleId: string): string | undefined {
  return tabs.find((tab) => tab.moduleId === moduleId)?.id;
}

/**
 * The tab `delta` places along from the active one, wrapping round both ends.
 *
 * Strip order rather than the order tabs were last looked at: that is what every browser's
 * `Ctrl+Tab` does, and a cycle whose next stop depends on where you have been is one you cannot
 * find your way round by looking at the strip.
 *
 * Hands back `activeId` when there is nowhere else to go — no tabs, one tab, or an id that is not
 * in the list — so the caller has nothing to check for.
 */
export function tabIdAtOffset(tabs: TabInfo[], activeId: string, delta: number): string {
  const at = tabs.findIndex((t) => t.id === activeId);
  if (at < 0 || tabs.length === 0) return activeId;
  // `+ tabs.length` before the modulo: JavaScript's `%` keeps the sign of its left side, so -1
  // would land on -1 rather than on the last tab.
  return tabs[(at + delta + tabs.length) % tabs.length].id;
}

export function retitleTab(tabs: TabInfo[], id: string, title: string): TabInfo[] {
  const tab = tabs.find((t) => t.id === id);
  if (tab === undefined || tab.title === title) return tabs;
  return tabs.map((t) => (t === tab ? { ...t, title } : t));
}

/** Compared by identity, not contents: a badge holds a React element, which nothing can compare
 *  by value, and every module builds its list with `useMemo` for exactly this reason. */
export function rebadgeTab(tabs: TabInfo[], id: string, badges: TabBadge[]): TabInfo[] {
  const tab = tabs.find((t) => t.id === id);
  if (tab === undefined || tab.badges === badges) return tabs;
  return tabs.map((t) => (t === tab ? { ...t, badges } : t));
}

/**
 * What the module wants remembered for this tab, or `undefined` to forget it.
 *
 * Compared with `Object.is`, exactly as {@link rebadgeTab} compares badges and for the same
 * reason: this is fed from a module's effect, `App` hands down a fresh callback on every render,
 * and a `setTabs` that always allocates closes the loop. Which means the module must not build a
 * new state object on every render either — the bail-out here can only catch what is handed to it.
 */
export function restateTab(tabs: TabInfo[], id: string, state: unknown): TabInfo[] {
  const tab = tabs.find((t) => t.id === id);
  if (tab === undefined || Object.is(tab.state, state)) return tabs;
  return tabs.map((t) => (t === tab ? { ...t, state } : t));
}
