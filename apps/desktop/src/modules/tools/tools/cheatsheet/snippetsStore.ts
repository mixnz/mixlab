import { createStore, jsonFile, useStore } from "../../../../core/jsonStore";
import { applySyncChanges, type SyncableCollection, type SyncItem } from "../../../../core/syncCollection";
import { readSnippets, type Snippet } from "./snippets";

/**
 * Snippets the user wrote, kept between sessions.
 *
 * **Only templates, never parameter values.** The values live in the tab's state and are lost when
 * the tab closes, like every other tool in the module — and that is where passwords pass through.
 * The parent spec's "saved cheatsheet" exception fits neatly inside the "do not save what the user
 * types" rule for exactly that reason: what is saved is the mould, not what is poured into it.
 *
 * The built-in set is not here — it is a constant in `builtin.ts`.
 */

const DEFAULTS: Snippet[] = [];

/* The shape checker lives in `snippets.ts` rather than here: it is the pure part, and the pure part
   is the testable part. This file only keeps the hookup to disk, which has nothing to test. */
const file = jsonFile<unknown>("tools-snippets.json", "snippets", DEFAULTS);
const store = createStore<Snippet[]>({
  defaults: DEFAULTS,
  load: async () => readSnippets(await file.load()),
  persist: file.persist,
});

export function useSnippets(): Snippet[] {
  return useStore(store);
}

/** Writes quietly in the background. A snippet that did not make it to disk is a lost snippet, and
 *  that is a pity rather than worth an error dialog blocking the screen. */
export function saveSnippets(next: Snippet[]): void {
  void store.save(next).catch(() => {});
}

export function snippetToSync(snippet: Snippet): SyncItem {
  const { id, ...data } = snippet;
  return { id, data };
}

/** Validated by the same check the file gets, so another machine cannot hand this one a shape its
 *  own disk would have been refused. */
export function snippetFromSync(synced: SyncItem): Snippet | null {
  const [parsed] = readSnippets([{ ...(synced.data as object), id: synced.id }]);
  return parsed ?? null;
}

export const snippetsSyncable: SyncableCollection = {
  id: "tools-snippets",
  labelKey: "toolsSync.snippets",
  read: async () => {
    await store.ready();
    return store.get().map(snippetToSync);
  },
  write: async (changes) => {
    await store.ready();
    const { items, skipped } = applySyncChanges(store.get(), changes, (s) => s.id, snippetFromSync);
    await store.save(items);
    return skipped;
  },
};
