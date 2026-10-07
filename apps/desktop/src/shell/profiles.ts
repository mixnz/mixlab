import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { onPreferencesChanged } from "../core/preferences";
import type { ModuleDefinition } from "./module";
import { MODULES, MODULE_PRESETS, PRESET_IDS, type PresetId } from "./registry";
import { ACCENT_KEY, LANGUAGE_KEY, MODULES_KEY, MODULES_PRESET_KEY, SESSION_KEY, THEME_KEY } from "./storageKeys";

/**
 * Which modules this window draws.
 *
 * The setting is a **set of module ids** and nothing else: the order the tab bar, the `[+]` menu,
 * the number chords and the Settings panes are drawn in is the registry's, always. That is what
 * keeps a checkbox from silently moving `Ctrl/Cmd+1`, and what makes a half-written or hand-edited
 * value something that can only be wrong about *which* modules, never about their order.
 *
 * Nothing here reads storage at module scope. The functions below are pure, and the ones that do
 * reach `localStorage` take it as an argument — the test suite runs in node, where there is none.
 * The whole design is in
 * `docs/specs/2026-09-09-t108-a-module-visibility-setting-design.md`.
 *
 * **This is visibility, not capability.** A hidden module keeps its backend commands, its state and
 * its files; it is not drawn. Turning it off deletes nothing and turning it back on finds
 * everything where it was.
 */

/**
 * The ids in `stored` as a set this build can act on, or `null` when there is nothing usable.
 *
 * Everything that arrives here is a string some older version of this app wrote, so nothing in it
 * is trusted — the same stance `parseSession` takes. An empty result is `null` rather than `[]`:
 * a window with no modules in it is not a window, and the caller then has one branch to write.
 */
export function normalizeModules(stored: unknown, knownIds: string[]): string[] | null {
  if (!Array.isArray(stored)) return null;
  const kept = stored.filter(
    (id, at): id is string =>
      typeof id === "string" && knownIds.includes(id) && stored.indexOf(id) === at,
  );
  return kept.length > 0 ? kept : null;
}

/**
 * `enabled` with `moduleId` in it, or `enabled` itself when it is already there or is not a module
 * this build has.
 *
 * **The same array back means nothing changed** — the caller writes no setting and says nothing in
 * a notice. Appended rather than inserted in the registry's order: order never comes out of the
 * stored set (T108's D1), `visibleModules` is a filter over `MODULES`, and a second place that
 * knew the order would be a second place that could be wrong about it.
 *
 * This is how a tab request for a hidden module turns it on — T110's D1. Visibility, not
 * capability: the module's commands were registered either way.
 */
export function withModule(enabled: string[], moduleId: string, knownIds: string[]): string[] {
  if (enabled.includes(moduleId) || !knownIds.includes(moduleId)) return enabled;
  return [...enabled, moduleId];
}

/** Which preset `enabled` is, or `null` for a set of someone's own. Set equality — the presets
 *  carry no order either. */
export function presetOf(enabled: string[]): PresetId | null {
  const set = new Set(enabled);
  return (
    PRESET_IDS.find((id) => {
      const preset = MODULE_PRESETS[id];
      return preset.length === set.size && preset.every((moduleId) => set.has(moduleId));
    }) ?? null
  );
}

/** The modules to draw, in the registry's order. */
export function visibleModules(enabled: string[]): ModuleDefinition[] {
  return MODULES.filter((module) => enabled.includes(module.id));
}

/**
 * What the window opens when nothing has named a module: the very first tab of a profile with no
 * session, the tab that replaces the last one closed, the tab that replaces the last one when a
 * module is turned off, `Ctrl/Cmd+T`, and the `[+]` button while there is only one module to offer.
 *
 * **The first of whatever list it is handed.** The three that replace a tab pass the visible
 * modules; the two that open one pass the openable ones — see `openableModules` in `shell/tabs.ts`,
 * which is a narrower list and can be empty. Neither caller may hand it an empty one, and the two
 * that could tell first: `Workspace` draws no `[+]` and registers no `Ctrl/Cmd+T` when there is
 * nothing to open.
 *
 * **The first module the profile shows** — and since `visible` is in the registry's order, that is
 * MixEngine for the *MixEngine* and *Everything* profiles and the database client for *Database
 * tools*, which is the whole of T109. There is no constant and no table of preferences per preset:
 * a second hand-written list of module ids could only ever restate the order this one already
 * carries, and would one day contradict it.
 *
 * Session restore wins over this, always: `Workspace` reads the session in a `useState`
 * initializer and only reaches here when there is nothing to restore.
 *
 * `visible` must not be empty. Deliberately unguarded — a default that is not in `enabled` would
 * make `Workspace` open a tab its own visibility effect drops and then reopen it, which is a render
 * loop where this is a crash. The three guards that make an empty set unreachable are T108's, at
 * the edges where such a value arrives: `normalizeModules`, the last checkbox, `useStartupProfile`.
 */
export function defaultModuleId(visible: ModuleDefinition[]): string {
  return visible[0].id;
}

/** Where the set is kept: `localStorage`, beside the theme, the accent, the language and the
 *  session — a handful of strings about the window, read once on the way up. */
export const MODULES_STORAGE_KEY = MODULES_KEY;

/** Where the preset beside the list is kept — T203, D0. */
export { MODULES_PRESET_KEY };

/** The mark for a set that is not a preset. */
const CUSTOM = "custom";

/**
 * What each preset held before T203. A home no T203 build has written holds one of these lists and
 * no mark, and that list means the preset. Not `MODULE_PRESETS`, which now says something else.
 */
export const PRESETS_BEFORE_T203: Record<PresetId, string[]> = {
  mixengine: ["mixengine"],
  everything: ["mixengine", "db", "rest", "terminal", "tools"],
  databaseTools: ["db", "rest", "terminal", "tools"],
};

function sameSet(a: string[], b: string[]): boolean {
  return a.length === b.length && a.every((id) => b.includes(id));
}

/** The mark written beside `enabled`: the preset it is, or `custom`. */
export function presetMark(enabled: string[]): PresetId | typeof CUSTOM {
  return presetOf(enabled) ?? CUSTOM;
}

/** The mark as stored, or `null` for none or one this build does not know. */
function readMark(raw: string | null): PresetId | typeof CUSTOM | null {
  if (raw === CUSTOM) return CUSTOM;
  return PRESET_IDS.find((id) => id === raw) ?? null;
}

/** Keys a build older than the module set wrote. Any one of them present means this webview
 *  profile has been used before — see {@link resolveStoredModules}. `public/storage-keys.js` has
 *  already moved them to these names by the time this is read. */
const LEGACY_SHELL_KEYS = [SESSION_KEY, THEME_KEY, ACCENT_KEY, LANGUAGE_KEY];

/** As much of `Storage` as this file uses. Taken as an argument rather than reached for, so the
 *  decisions below can be tested in node, where there is no `localStorage`. */
export interface ShellStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

/**
 * The profile, as far as storage alone can say it — or `null` when only the backend can.
 *
 * Two questions, in order:
 *
 * 1. **A stored set**, if there is one this build can act on.
 * 2. **Any other shell key**, which means a build older than this task has run in this profile.
 *    That window has a theme, a session and four modules' worth of saved work, and handing it the
 *    *MixEngine* preset would make its database tabs vanish on an upgrade. It gets *Everything*.
 *
 * None of the keys in (2) can be written before the first-run screen is answered — `theme.ts` only
 * ever *removes* a key for a default value, the language is changed from inside Settings, and the
 * session is written by an effect of a workspace that does not mount until the profile is decided.
 * So the evidence is one-directional and the screen cannot manufacture it for itself.
 *
 * `null` is the third question, and it is the only one that costs an IPC call: see
 * {@link importHappened}.
 */
export function resolveStoredModules(storage: ShellStorage, knownIds: string[]): string[] | null {
  let list: string[] | null = null;
  const raw = storage.getItem(MODULES_STORAGE_KEY);
  if (raw !== null) {
    try {
      list = normalizeModules(JSON.parse(raw), knownIds);
    } catch {
      // A half-written or hand-edited value is no setting at all, and falls through to the next
      // question — which is the same treatment `parseSession` gives an unreadable session.
    }
  }
  const mark = readMark(storage.getItem(MODULES_PRESET_KEY));
  const known = (ids: string[]) => ids.filter((id) => knownIds.includes(id));

  // A preset mark wins while the list beside it agrees with it — this build's composition or the
  // one before T203. A list that says anything else was changed where the mark is unknown, by a
  // build that writes only the list, and the person's change wins (T203, D0).
  if (mark !== null && mark !== CUSTOM) {
    if (list === null || sameSet(list, MODULE_PRESETS[mark]) || sameSet(list, PRESETS_BEFORE_T203[mark])) {
      return known(MODULE_PRESETS[mark]);
    }
    return list;
  }
  if (list !== null) {
    // No mark at all: a home from before T203, whose list is a preset's old composition or not.
    if (mark === null) {
      const stored = list;
      const was = PRESET_IDS.find((id) => sameSet(stored, PRESETS_BEFORE_T203[id]));
      if (was) return known(MODULE_PRESETS[was]);
    }
    return list;
  }
  if (LEGACY_SHELL_KEYS.some((key) => storage.getItem(key) !== null)) {
    return MODULE_PRESETS.everything;
  }
  return null;
}

/** {@link resolveStoredModules} against the real storage. */
export function readEnabledModules(): string[] | null {
  return resolveStoredModules(
    localStorage,
    MODULES.map((m) => m.id),
  );
}

/** Writes the set down, or gives up quietly — `localStorage` throws once the origin is full, and
 *  nothing here is worth failing the window the user is in the middle of using. What is already
 *  stored is then an older answer, and an older answer is better than none. */
export function writeEnabledModules(enabled: string[]): void {
  try {
    localStorage.setItem(MODULES_STORAGE_KEY, JSON.stringify(enabled));
    localStorage.setItem(MODULES_PRESET_KEY, presetMark(enabled));
  } catch {
    /* see above */
  }
}

/** Whether T104's import brought a standalone-client user's data across. A call that cannot be made — the
 *  browser-only `npm run dev`, a command somehow not registered — reads as `false`, which shows the
 *  first-run screen: one click, rather than a silently wrong profile. */
export function importHappened(): Promise<boolean> {
  return invoke<boolean>("import_happened").catch(() => false);
}

/** What the window is doing about the question "which modules?". */
export type Startup =
  | { status: "deciding" }
  | { status: "asking"; choose: (preset: PresetId) => void }
  | { status: "ready"; enabled: string[]; setEnabled: (enabled: string[]) => void };

/**
 * Resolves the profile, and keeps it written down.
 *
 * Synchronous whenever it can be, which is every launch but one: an existing window never renders
 * `deciding` and there is no flash. The one launch that does await is a webview profile with no
 * shell settings in it at all, where the only thing that can tell a fresh install from an imported
 * one is the marker on disk.
 *
 * The write is an effect rather than something the resolution does, so every path that changes the
 * set — the first-run screen, the presets, the checkboxes, and later T110's handoff — is persisted
 * by the same line. **Nothing leaves the screen without writing**: were the answer not written, the
 * next launch would find the session the workspace had since written and answer *Everything* to a
 * question the user had already answered differently.
 */
export function useStartupProfile(): Startup {
  const [enabled, setEnabled] = useState<string[] | null>(readEnabledModules);
  const [asking, setAsking] = useState(false);
  useEffect(
    () =>
      onPreferencesChanged(() => {
        const next = readEnabledModules();
        if (next) setEnabled(next);
      }),
    [],
  );

  useEffect(() => {
    if (enabled) writeEnabledModules(enabled);
  }, [enabled]);

  useEffect(() => {
    if (enabled || asking) return;
    let cancelled = false;
    void importHappened().then((imported) => {
      if (cancelled) return;
      if (imported) setEnabled(MODULE_PRESETS.everything);
      else setAsking(true);
    });
    return () => {
      cancelled = true;
    };
  }, [enabled, asking]);

  const choose = useCallback((preset: PresetId) => {
    setEnabled(MODULE_PRESETS[preset]);
    setAsking(false);
  }, []);

  if (enabled) return { status: "ready", enabled, setEnabled };
  return asking ? { status: "asking", choose } : { status: "deciding" };
}
