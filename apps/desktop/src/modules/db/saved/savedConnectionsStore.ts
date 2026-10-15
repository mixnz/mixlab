import {
  addSavedConnection,
  loadSavedConnections,
  removeSavedConnection,
  updateSavedConnection,
} from "./savedConnections";
import type { SavedConnection } from "../types";
import { createStore, useStore, useStoreLoaded } from "../../../core/jsonStore";

/**
 * The saved connection list, shared by every tab.
 *
 * Each tab used to read the list for itself — `connections.json`, then one credential-store lookup
 * per connection, all of it again per tab — and then kept its own copy. Two things came of that: a
 * connection saved in one tab stayed invisible in the others until the app was restarted, and a
 * newly opened tab spent that first read showing an empty sidebar, which widens as the names
 * arrive and shoves the form sideways.
 *
 * The list is one thing on disk, so it is one thing in memory: read once, written through here,
 * and handed to every tab that asks. A tab opened later renders the finished list on its first
 * frame — there is nothing left to arrive.
 */

/* No `persist`: every write goes through `savedConnections.ts`, which writes and hands the new
   list back. The snapshot is what it returns, not something assembled here. The mechanics — read
   once, replace wholesale, `loaded` kept apart from the value — are `core/jsonStore.ts`. */
const store = createStore<SavedConnection[]>({ defaults: [], load: loadSavedConnections });

/** The shared list, kept in step across every tab that calls this. */
export function useSavedConnections(): SavedConnection[] {
  return useStore(store);
}

/**
 * Whether the read has finished.
 *
 * The list is empty before the file has been read and empty after it when nothing is saved, and
 * nothing looking only at the list can tell those two apart. Anything that treats "not in the
 * list" as "deleted" — a tab restoring the connection it had open — has to ask this first.
 */
export function useSavedConnectionsLoaded(): boolean {
  return useStoreLoaded(store);
}

/* Writes go through the module they always did — it owns the split between `connections.json` and
   the OS credential store — and the list it hands back becomes the new shared snapshot. */

export async function addConnection(entry: SavedConnection): Promise<void> {
  store.publish(await addSavedConnection(entry));
}

export async function updateConnection(entry: SavedConnection): Promise<void> {
  store.publish(await updateSavedConnection(entry));
}

export async function removeConnection(id: string): Promise<void> {
  store.publish(await removeSavedConnection(id));
}
