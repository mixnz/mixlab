import { announcePreferencesChanged } from "../core/preferences";
import type { SyncableCollection, SyncItem } from "../core/syncCollection";
import { LANGUAGE_KEY, MODULES_KEY, PALETTE_KEY, THEME_KEY } from "./storageKeys";

/**
 * The shell's own preferences, one record per `localStorage` key (D5). An allow-list: the session,
 * the tab layout and anything else in storage are this machine's.
 *
 * Values travel as the strings they are stored as. Each owner already checks what it reads — an
 * unknown theme reads as the default. An `accent` from a build before colour themes is not on the
 * list, so it is skipped rather than written — so a value another machine sends is no more trusted than
 * one a person typed into the devtools.
 */
const KEYS: Record<string, string> = {
  theme: THEME_KEY,
  palette: PALETTE_KEY,
  language: LANGUAGE_KEY,
  modules: MODULES_KEY,
};

/** What the collection needs of `localStorage`, so a test can hand it a map instead. */
export interface PreferenceStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

export function preferencesCollection(
  storage: () => PreferenceStorage,
  announce: () => void,
): SyncableCollection {
  return {
    id: "preferences",
    labelKey: "sync.preferences",
    read: async () => {
      const items: SyncItem[] = [];
      for (const [id, key] of Object.entries(KEYS)) {
        const value = storage().getItem(key);
        if (value !== null) items.push({ id, data: value });
      }
      return items;
    },
    write: async (changes) => {
      const skipped: string[] = [];
      let changed = false;
      for (const synced of changes.upserts) {
        const key = KEYS[synced.id];
        if (key === undefined || typeof synced.data !== "string") {
          skipped.push(synced.id);
          continue;
        }
        storage().setItem(key, synced.data);
        changed = true;
      }
      for (const id of changes.removed) {
        const key = KEYS[id];
        if (key === undefined) continue;
        storage().removeItem(key);
        changed = true;
      }
      if (changed) announce();
      return skipped;
    },
  };
}

/** The one the app uses. `localStorage` is reached when sync runs, never when this file loads. */
export const preferencesSyncable = preferencesCollection(() => localStorage, announcePreferencesChanged);
