import { SESSION_KEY as STORAGE_KEY } from "./storageKeys";
import type { TabInfo } from "./tabs";

/**
 * Which tabs were open when the app was last closed, and which of them was in front.
 *
 * What the tab bar itself draws — a tab's id, the module it holds, the name it was carrying — and
 * one opaque slot per tab that the module behind it filled in. This file does not know what is in
 * that slot and never looks: only the module that wrote it can say whether `{ savedId: "…" }` is a
 * connection that still exists, and a shell that could tell would be a shell that knows the
 * database module by name. Everything about the shape of it, and about what may go in there, is in
 * `docs/specs/2026-08-23-tab-session-context-design.md` — §4 in particular: ids only,
 * because this is `localStorage`.
 *
 * A restored tab is a place on the strip, and the module behind it decides what to do with its own
 * slot the first time the tab is looked at.
 *
 * Badges are left out because they cannot be stored: a badge holds a React element. They come back
 * the moment the module mounts and reports them, which is the only time anything knows what they
 * should say.
 *
 * `localStorage`, like `shell/theme.ts` — this is a handful of strings about the window, read once
 * on the way up and written as it changes, and a file on disk read through an async plugin would
 * mean an empty tab bar for the first frame of every launch.
 */


/** One tab as it is stored. A subset of {@link TabInfo}, and deliberately not that type: adding a
 *  field there must not silently start writing it to disk. */
export interface StoredTab {
  id: string;
  moduleId: string;
  title: string;
  /** The module's own, written by it and read back by it. Never inspected here — see the note at
   *  the top of the file. Absent in a session from before there was one. */
  state?: unknown;
}

export interface StoredSession {
  tabs: StoredTab[];
  /** One of `tabs`, already checked. The tab the user was looking at, and the only one the app
   *  brings up on launch. */
  activeId: string;
}

function isStoredTab(value: unknown, knownModuleIds: string[]): value is StoredTab {
  if (typeof value !== "object" || value === null) return false;
  const tab = value as Record<string, unknown>;
  return (
    typeof tab.id === "string" &&
    tab.id !== "" &&
    typeof tab.title === "string" &&
    typeof tab.moduleId === "string" &&
    // A module that has been renamed or taken out leaves tabs nothing can render. They are dropped
    // rather than shown as an error: the user did not ask for a tab of a module that no longer
    // exists, and `moduleById` throws on one.
    knownModuleIds.includes(tab.moduleId)
  );
}

/**
 * What was stored, read defensively, or `null` when there is nothing usable there.
 *
 * Pure, and takes the module ids rather than reading the registry, so what it does with a stored
 * module nobody recognises can be tested without one. Everything that arrives here is a string
 * some older version of the app wrote, so nothing in it is trusted: a half-written file, a shape
 * from three versions ago and a hand-edited one all have to come out as `null` or as a list of
 * tabs that can actually be drawn.
 */
export function parseSession(raw: string | null, knownModuleIds: string[]): StoredSession | null {
  if (raw === null) return null;

  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return null;
  }

  if (typeof parsed !== "object" || parsed === null) return null;
  const session = parsed as Record<string, unknown>;
  if (!Array.isArray(session.tabs)) return null;

  const tabs = session.tabs
    .filter((tab): tab is StoredTab => isStoredTab(tab, knownModuleIds))
    // `state` is copied across without a look at it. Whatever came out of `JSON.parse` can go back
    // in, so there is nothing to guard against at this level; a module handed something it does
    // not recognise simply does not restore, and its neighbours never hear about it.
    .map(({ id, moduleId, title, state }) => ({ id, moduleId, title, state }));
  if (tabs.length === 0) return null;

  // A dropped module can take the active tab with it, and a session with no valid active tab is
  // still a session — the last tab on the strip is where the app opens instead.
  const stored = session.activeId;
  const activeId =
    typeof stored === "string" && tabs.some((tab) => tab.id === stored)
      ? stored
      : tabs[tabs.length - 1].id;

  return { tabs, activeId };
}

/**
 * The session as stored, or `null` on the first launch — and on any launch after one that wrote
 * something this version cannot read.
 *
 * `knownModuleIds` is the **visible** list and not the registry's: a tab of a module the profile
 * hides is dropped rather than mounted blind, which is the same treatment a tab of a module that no
 * longer exists already gets. One code path, one behaviour — T108.
 */
export function readSession(knownModuleIds: string[]): StoredSession | null {
  return parseSession(localStorage.getItem(STORAGE_KEY), knownModuleIds);
}

/**
 * Writes the session down, or gives up quietly.
 *
 * Nothing here is worth an error: the session is a convenience, and the app it would fail is one
 * the user is in the middle of using. Two things can fail. `localStorage` throws
 * `QuotaExceededError` once the origin is full — a tab whose module keeps a large `state` is
 * enough — and `JSON.stringify` throws on a module slot holding something that is not JSON, a
 * cycle or a `BigInt`. Uncaught, either one comes out of an effect that runs on every tab and
 * badge change, which is to say it takes the whole window down over and over.
 *
 * The read side has always been this defensive — `parseSession` treats anything it cannot
 * understand as no session at all. This is the same answer on the way out: the next launch opens a
 * fresh tab, which is what the first launch does.
 */
export function writeSession(tabs: TabInfo[], activeId: string): void {
  const session: StoredSession = {
    tabs: tabs.map(({ id, moduleId, title, state }) => ({ id, moduleId, title, state })),
    activeId,
  };
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(session));
  } catch {
    // Deliberately not cleared either: what is already stored is an older session, and an older
    // session is better than none.
  }
}
