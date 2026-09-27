import { useSyncExternalStore } from "react";
import type { RunResult } from "./loop";

/**
 * A download shorter than this never shows. A run shows nothing while it only asks what changed:
 * `down` waits for `downloading`, and `up` for `uploading`.
 */
export const SHOW_AFTER_MS = 200;

/** Once shown, each icon this long at least, so a run does not flash one on and off. */
export const SHOW_AT_LEAST_MS = 600;

/**
 * Which way sync is moving, as the window draws it: `down` while a full run pulls what the server
 * said changed, `up` once this machine's changes are actually being sent. `null` when neither
 * shows.
 */
export type SyncDirection = "down" | "up" | null;

export interface SyncActivity {
  /** Which icon shows, if any. */
  direction: SyncDirection;
  /** When the last full run finished without a failure, in ms since the epoch. */
  lastSyncedAt: number | null;
  /** The first failure of the last run that had one, until a full run succeeds; `undefined` when none. */
  lastError: unknown;
}

export interface ActivityStore {
  /** For `useSyncExternalStore`: stable. */
  subscribe: (listener: () => void) => () => void;
  /** The same object until something changes. */
  get: () => SyncActivity;
  /** The run under way is about to pull: the server named something that changed elsewhere. */
  downloading: () => void;
  /** The run under way is sending this machine's changes. */
  uploading: () => void;
  runEnded: (result: RunResult) => void;
}

/**
 * The loop's runs, as the window draws them. In memory, like `replaced.ts`: the Settings dialog
 * that shows most of it is not mounted while sync runs.
 *
 * Every change of icon goes through `want`: from nothing it waits `delay`, and away from an icon it
 * waits until that icon has been up {@link SHOW_AT_LEAST_MS}. A later wish replaces a pending one,
 * so back-to-back runs keep their icon instead of blinking between them.
 */
export function createActivity(): ActivityStore {
  let value: SyncActivity = { direction: null, lastSyncedAt: null, lastError: undefined };
  const listeners = new Set<() => void>();
  let pending: ReturnType<typeof setTimeout> | null = null;
  let shownAt = 0;

  function set(next: Partial<SyncActivity>): void {
    value = { ...value, ...next };
    for (const listener of listeners) listener();
  }

  function want(direction: SyncDirection, delay = 0): void {
    if (pending !== null) {
      clearTimeout(pending);
      pending = null;
    }
    if (direction === value.direction) return;
    // Nothing on screen and nothing to show: a run that ended before its delay was up.
    if (direction === null && value.direction === null) return;
    const wait = value.direction === null ? delay : Math.max(0, SHOW_AT_LEAST_MS - (Date.now() - shownAt));
    const apply = () => {
      pending = null;
      shownAt = Date.now();
      set({ direction });
    };
    if (wait === 0) apply();
    else pending = setTimeout(apply, wait);
  }

  return {
    subscribe(listener) {
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },
    get: () => value,
    downloading() {
      // A run that only asks shows nothing. A push is the local check every focus and every half
      // minute runs, and reading eleven collections took 240–465ms; a full run first asks the
      // server what changed (T189). Shown for either, alt-tabbing looked like syncing. `down` is
      // for a pull the server said has something to fetch, and it still waits its delay.
      want("down", SHOW_AFTER_MS);
    },
    uploading() {
      // At once, however short: it is the one moment something of this machine's leaves it, and
      // the person who just saved an edit is looking for it. Up for the rest of the run, since a
      // run that sent something is the news, not the pages it reads after.
      want("up");
    },
    runEnded({ run, error, finished }) {
      if (error !== undefined) set({ lastError: error });
      else if (run === "full" && finished) set({ lastSyncedAt: Date.now(), lastError: undefined });
      want(null);
    },
  };
}

/** The one the loop reports to (`index.ts`) and the window reads. */
export const syncActivity = createActivity();

export function useSyncActivity(): SyncActivity {
  return useSyncExternalStore(syncActivity.subscribe, syncActivity.get);
}

/**
 * How long ago `at` was, in the app's language: "2 minutes ago", "3 hours ago". `null` under a
 * minute, where a sentence of its own ("just now") reads better than "0 minutes ago".
 */
export function syncedAgo(at: number, now: number, lang: string): string | null {
  const minutes = Math.floor((now - at) / 60_000);
  if (minutes < 1) return null;
  const format = new Intl.RelativeTimeFormat(lang, { numeric: "always" });
  if (minutes < 60) return format.format(-minutes, "minute");
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return format.format(-hours, "hour");
  return format.format(-Math.floor(hours / 24), "day");
}
