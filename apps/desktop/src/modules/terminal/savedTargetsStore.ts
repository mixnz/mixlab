import {
  addSavedTarget,
  loadSavedTargets,
  removeSavedTarget,
  updateSavedTarget,
} from "./savedTargets";
import type { SavedTarget } from "./types";
import { createStore, useStore, useStoreLoaded } from "../../core/jsonStore";

/**
 * The list of saved targets, shared by every tab.
 *
 * Read once: if each tab read it itself, each tab would cost one file read plus one credential
 * store query per server, and a target saved in this tab would not show in another until the next
 * app launch. The list is one thing on disk, so it is one thing in memory.
 *
 * The mechanism lives in `core/jsonStore.ts` — read once, replace wholesale, `loaded` kept apart
 * from the value. Only this list's own part is left here: it does not read the file itself, but
 * goes through `savedTargets.ts`, which keeps the boundary between `terminal-hosts.json` and the
 * operating system's credential store.
 */

/* No `persist`: every write goes through `savedTargets.ts`, which both writes and returns the new
   list. The snapshot is what it returns, not something the store builds for itself. */
const store = createStore<SavedTarget[]>({ defaults: [], load: loadSavedTargets });

/** The shared list, kept in sync across every tab that calls it. */
export function useSavedTargets(): SavedTarget[] {
  return useStore(store);
}

/** Whether the file has finished reading. An empty list before reading and an empty list when
 *  there are no targets are two different things, and looking at the list cannot tell them apart.
 *  Anyone treating "not in the list" as "deleted" — a tab restoring the target it had open — has
 *  to ask this first. */
export function useSavedTargetsLoaded(): boolean {
  return useStoreLoaded(store);
}

/* Writes go through the module that has always written — it keeps the boundary between
   `terminal-hosts.json` and the credential store — and the list it returns becomes the new
   snapshot. */

/** The list once it has been read — for a caller outside React, such as the `saveTarget` action. */
export async function currentTargets(): Promise<SavedTarget[]> {
  await store.ready();
  return store.get();
}

export async function addTarget(target: SavedTarget): Promise<void> {
  store.publish(await addSavedTarget(target));
}

export async function updateTarget(target: SavedTarget): Promise<void> {
  store.publish(await updateSavedTarget(target));
}

export async function removeTarget(id: string): Promise<void> {
  store.publish(await removeSavedTarget(id));
}
