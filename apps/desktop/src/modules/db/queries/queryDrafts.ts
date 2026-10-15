import { Store } from "@tauri-apps/plugin-store";

/**
 * The script left in the Query tab, kept between sessions.
 *
 * A half-written query is work, and closing a tab is not a decision to throw it away — the editor
 * used to open empty every time whatever had been in it. One draft per saved connection and
 * database, because that is the pair the script was written against: the same editor pointed at
 * `staging` and at `production` is two different pieces of work.
 *
 * Nothing is kept for a connection that was never saved. There would be no name to file it under
 * that survives the app closing — the connection's id is minted fresh on every connect.
 */

/**
 * Past this, a draft is not written while the typing is going on.
 *
 * A script this size is a dump someone pasted in, and copying a quarter of a megabyte to a JSON
 * file on every pause in the typing costs more than the convenience is worth.
 *
 * **The ceiling applies to the debounced write only**, which is the one that would happen hundreds
 * of times. The write at the moment the tab goes away happens once, and a script too big to save
 * cheaply is not a script anyone would rather lose — see {@link saveDraftNow}.
 */
const MAX_DRAFT = 256 * 1024;

/** How long the typing has to stop before the draft is written. Long enough that a burst of typing
 *  is one write and not thirty; short enough that what is lost to a crash is a sentence. */
const IDLE_MS = 1000;

let storePromise: Promise<Store> | null = null;

function getStore(): Promise<Store> {
  if (!storePromise) storePromise = Store.load("query-drafts.json");
  return storePromise;
}

/** One draft's name on disk. A saved connection's id is a UUID, so the first `|` in
 *  the key is always the separator, however exotic the database name after it happens to be. */
function key(profileId: string, database: string): string {
  return `${profileId}|${database}`;
}

/** The draft for this connection and database, or the empty string when there is none. */
export async function loadDraft(profileId: string, database: string): Promise<string> {
  if (profileId === "") return "";
  const store = await getStore();
  return (await store.get<string>(key(profileId, database))) ?? "";
}

/** The write waiting to happen, by the draft it is for. Keyed rather than a single timer: switching
 *  database mid-pause must not throw away the write the previous one was owed. */
const pending = new Map<string, ReturnType<typeof setTimeout>>();

/**
 * Remembers the script, a moment after the typing stops.
 *
 * Called on every keystroke and does almost nothing on almost all of them.
 */
export function saveDraft(profileId: string, database: string, sql: string): void {
  if (profileId === "") return;
  const id = key(profileId, database);
  clearTimeout(pending.get(id));
  pending.set(
    id,
    setTimeout(() => {
      pending.delete(id);
      // Held in the tab but not written, until the tab goes away — see {@link MAX_DRAFT}.
      if (sql.length <= MAX_DRAFT) write(id, sql);
    }, IDLE_MS)
  );
}

/**
 * Writes the draft now, without waiting for the typing to stop.
 *
 * For the moment the draft stops being the current one — the database changed under it, or the tab
 * is going away. The debounce above is there to spare the disk, not to delay a write that has run
 * out of time, and {@link MAX_DRAFT} is not applied here for the same reason: this is one write,
 * and the alternative to a slow one is the work being gone.
 */
export function saveDraftNow(profileId: string, database: string, sql: string): void {
  if (profileId === "") return;
  const id = key(profileId, database);
  clearTimeout(pending.get(id));
  pending.delete(id);
  write(id, sql);
}

/**
 * Puts the draft under its name, or takes the name away.
 *
 * A draft that has been emptied is removed rather than stored as `""`: the file should not grow an
 * entry for every database anyone ever opened the tab on.
 */
function write(id: string, sql: string): void {
  const keep = sql.trim() !== "";
  void getStore()
    .then(async (store) => {
      if (keep) await store.set(id, sql);
      else await store.delete(id);
      await store.save();
    })
    // A draft that could not be written is not worth interrupting anyone about. The script is
    // still on screen, which is where it matters.
    .catch(() => {});
}
