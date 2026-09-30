import { createStore, jsonFile, useStore } from "../../core/jsonStore";

/**
 * Every script the Query tab has run, newest first.
 *
 * The thing an editor is asked for most often is the query from twenty minutes ago — the one that
 * was right, before it was edited into something else. Nothing in MixLab used to remember it.
 *
 * Shared across tabs the way the connection list is: one list in memory, written through here, so
 * a query run in one tab is in the history of the next one opened. Entries are stamped with the
 * saved connection they were run against and the panel shows only that one's, because a query
 * against `staging` is not an answer to a question about `production`.
 */

/** One run, as it is remembered. The result is summarised rather than kept: this is a record of
 *  what was asked, not a cache of what came back. */
export interface QueryHistoryEntry {
  sql: string;
  /** The saved connection it ran against, or the empty string for a connection nobody saved. */
  profileId: string;
  database: string;
  /** Epoch milliseconds. Absolute rather than relative, so a list read tomorrow still reads right. */
  startedAt: number;
  durationMs: number;
  /** Rows in the last result set, or null for a script that produced none. */
  rowCount: number | null;
  /** The reason it failed, when it did. A failed query is worth keeping — it is usually the one
   *  about to be fixed and run again. */
  error: string | null;
}

/** How many runs are kept. Enough to cover a day's work several times over; small enough that the
 *  file stays something the app reads once at startup without thinking about it. */
const MAX_ENTRIES = 300;

const store = createStore<QueryHistoryEntry[]>({
  defaults: [],
  ...jsonFile<QueryHistoryEntry[]>("query-history.json", "entries", []),
});

/** Publish and write behind. A history that could not be written is not worth interrupting anyone
 *  over, and it is still right in memory. */
function keep(list: QueryHistoryEntry[]): void {
  void store.save(list).catch(() => {});
}

export function useQueryHistory(): QueryHistoryEntry[] {
  return useStore(store);
}

/**
 * The list with this run at the front of it.
 *
 * A script run twice in a row is remembered once: pressing Run again after reading the results is
 * the commonest thing there is, and a history of the same query forty times is a history of
 * nothing. Only an immediate repeat is collapsed — the same query run again after something else
 * is a separate occasion, and its place in the order is what makes it findable.
 */
function withEntry(entry: QueryHistoryEntry): QueryHistoryEntry[] {
  const snapshot = store.get();
  const first = snapshot[0];
  const repeat =
    first !== undefined &&
    first.sql === entry.sql &&
    first.profileId === entry.profileId &&
    first.database === entry.database;
  return [entry, ...(repeat ? snapshot.slice(1) : snapshot)].slice(0, MAX_ENTRIES);
}

/**
 * Adds a run to the front of the list.
 *
 * **The file is read first, and that is the whole point of the wait.** Nothing else reads it until
 * the History dialog is opened, which is usually much later in a session than the first query is
 * run — so a run recorded straight away would be added to an empty list and written as the whole
 * history, and everything from every earlier session would be gone.
 */
export function recordQuery(entry: QueryHistoryEntry): void {
  void store.ready().then(
    () => keep(withEntry(entry)),
    // The file could not be read. The run is still worth showing for the rest of the session, but
    // nothing is written back: what is on disk is unknown, and writing over it would lose it.
    () => store.remember(withEntry(entry))
  );
}

/**
 * Forgets one run.
 *
 * Matched on what it was rather than on where it sits: the list is re-published on every run, and an
 * index read when the dialog rendered would point at a different entry by the time it is pressed.
 * The three fields together are as good as an id — a run is a query, against a connection, at a
 * millisecond.
 */
export function removeQueryHistoryEntry(entry: QueryHistoryEntry): void {
  void store.ready().then(
    () =>
      keep(
        store
          .get()
          .filter(
            (kept) =>
              kept.startedAt !== entry.startedAt ||
              kept.profileId !== entry.profileId ||
              kept.sql !== entry.sql
          )
      ),
    () => {}
  );
}

/**
 * Forgets this connection's runs, and only this connection's.
 *
 * The list this is pressed from shows one connection's history, so that is the whole of what it may
 * clear — a button that emptied production's history while staging's was on screen would be a
 * button nobody could trust. There is no undo, which is why it asks first.
 */
export function clearQueryHistory(profileId: string): void {
  void store.ready().then(
    () => keep(store.get().filter((entry) => entry.profileId !== profileId)),
    () => {}
  );
}
