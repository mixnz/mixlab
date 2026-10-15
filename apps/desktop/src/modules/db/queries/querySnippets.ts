import { createStore, jsonFile, useStore } from "../../../core/jsonStore";
import {
  applySyncChanges,
  asRecord,
  type SyncableCollection,
  type SyncItem,
} from "../../../core/syncCollection";

/**
 * Named queries, saved by hand and offered back by name as the editor is typed in.
 *
 * The difference between this and the history beside it is intent: history is everything that ran,
 * kept automatically and eventually dropped off the end; a snippet is something someone decided was
 * worth keeping and gave a name to. So snippets are never evicted, and they complete — typing the
 * first letters of the name offers the whole query, in the same list the table names come from.
 *
 * Not filed per connection: the queries worth naming tend to be the ones that work anywhere.
 */

export interface QuerySnippet {
  /** What is typed to insert it. Unique, case-insensitively — saving over a name replaces it. */
  name: string;
  sql: string;
}

const store = createStore<QuerySnippet[]>({
  defaults: [],
  ...jsonFile<QuerySnippet[]>("query-snippets.json", "snippets", []),
});

/** Every snippet, in the order they will be offered — by name, so the list reads the same twice. */
export function useQuerySnippets(): QuerySnippet[] {
  return useStore(store);
}

/** Saves under `name`, replacing whatever was there. Unlike the history, a failed write here is
 *  worth knowing about — the user asked for this one — so the promise is handed back. */
export async function saveSnippet(snippet: QuerySnippet): Promise<void> {
  const name = snippet.name.trim();
  if (name === "") return;
  const key = name.toLowerCase();
  const list = [...store.get().filter((s) => s.name.toLowerCase() !== key), { ...snippet, name }].sort(
    (a, b) => a.name.localeCompare(b.name)
  );
  await store.save(list);
}

export async function removeSnippet(name: string): Promise<void> {
  const key = name.toLowerCase();
  await store.save(store.get().filter((s) => s.name.toLowerCase() !== key));
}

/** A snippet is known by its name, case-insensitively (D5): renaming one is a deletion and a new
 *  record, the same as it is on this machine. */
export function snippetToSync(snippet: QuerySnippet): SyncItem {
  return { id: snippet.name.toLowerCase(), data: { name: snippet.name, sql: snippet.sql } };
}

export function snippetFromSync(synced: SyncItem): QuerySnippet | null {
  const data = asRecord(synced.data);
  if (!data || typeof data.name !== "string" || typeof data.sql !== "string") return null;
  return { name: data.name, sql: data.sql };
}

export const querySnippetsSyncable: SyncableCollection = {
  id: "query-snippets",
  labelKey: "dbSync.querySnippets",
  read: async () => {
    await store.ready();
    return store.get().map(snippetToSync);
  },
  write: async (changes) => {
    await store.ready();
    const { items, skipped } = applySyncChanges(
      store.get(),
      changes,
      (s) => s.name.toLowerCase(),
      snippetFromSync,
    );
    await store.save(items.sort((a, b) => a.name.localeCompare(b.name)));
    return skipped;
  },
};
