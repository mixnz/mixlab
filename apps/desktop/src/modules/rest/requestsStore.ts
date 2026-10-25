import { createStore, useStore, useStoreLoaded } from "../../core/jsonStore";
import type { ParsedRequest } from "./parsePaste";
import {
  addRecent,
  addSaved,
  bumpRecent,
  findRecentTarget,
  findRequest,
  loadRequests,
  moveToRecent,
  newRequest,
  persistRequests,
  pinToSaved,
  saveRequests,
  removeRequest,
  updateRequest,
} from "./requests";
import type { RequestLists, RestRequest } from "./types";

/**
 * The request list, shared by every REST tab.
 *
 * One thing on disk is one thing in memory: read once, written through here, handed to every tab
 * that asks. A request edited in one tab is the same request in the next — which matters more
 * here than for connections, because a draft lives in the request itself.
 */

const EMPTY: RequestLists = { saved: [], recent: [] };

/* No `persist` here: `persistRequests` in `requests.ts` owns the file, and every writer below
   already calls it. The mechanics — read once, replace wholesale, `loaded` kept apart from the
   value — are `core/jsonStore.ts`. */
const store = createStore<RequestLists>({ defaults: EMPTY, load: loadRequests });

export function useRequestLists(): RequestLists {
  return useStore(store);
}

/** Whether the read has finished. Empty-because-unread and empty-because-nothing-is-saved look
 *  the same in the lists; a tab restoring the requests it had open needs them told apart. */
export function useRequestListsLoaded(): boolean {
  return useStoreLoaded(store);
}

/** What the store currently holds, for callers outside a component — the send path, which needs
 *  the request as it stands rather than as it was when a handler was made. */
export function currentLists(): RequestLists {
  return store.get();
}

/** A fresh request at the top of Saved, returned so the caller can open a tab on it. */
export function createRequest(): RestRequest {
  const request = newRequest(crypto.randomUUID(), Date.now());
  const lists = addSaved(store.get(), request);
  store.publish(lists);
  persistRequests(lists);
  return request;
}

/**
 * Writes a request's new state through, wherever it lives.
 *
 * This is the whole of "saving": there is no unsaved state, no Save button and no dialog asking
 * whether to keep anything, because every edit lands here as it is made.
 */
export function saveRequest(request: RestRequest): void {
  const lists = updateRequest(store.get(), request);
  store.publish(lists);
  persistRequests(lists);
}

/** Adds a request that is not in either group yet — a duplicate, and from Phase 2 a paste. */
export function addRequest(request: RestRequest): void {
  const lists = addSaved(store.get(), request);
  store.publish(lists);
  persistRequests(lists);
}

/**
 * A pasted command, as a row in Recent — or the row that was already there.
 *
 * The same command pasted twice is one request: the row already aimed at that method and URL comes
 * to the head of the group and is stamped as used, rather than a second copy of it appearing. The
 * request to open a tab on is returned either way, so the caller does not need to know which
 * happened.
 */
export function pasteRequest(parsed: ParsedRequest): RestRequest {
  const now = Date.now();
  const existing = findRecentTarget(store.get(), parsed.method, parsed.url);
  const lists = existing
    ? bumpRecent(store.get(), existing.id, now)
    : addRecent(store.get(), {
        ...newRequest(crypto.randomUUID(), now),
        ...parsed,
        origin: "paste",
      });
  store.publish(lists);
  persistRequests(lists);
  // After a bump the row is a new object carrying the new stamp; the one found before it is stale.
  return existing === undefined
    ? lists.recent[0]
    : (findRequest(lists, existing.id) ?? lists.recent[0]);
}

/**
 * A command pasted over a request nobody had typed into yet.
 *
 * The row keeps its id — so the tab it is open in carries on — and moves to Recent, because
 * everything in it came from the paste. Pressing New to have somewhere to paste into is not a
 * decision to keep the result, and a row of that kind sitting in Saved for good is not what anybody
 * asked for.
 *
 * The duplicate rule is the same as pasting anywhere else: when Recent already holds that command,
 * it is that row which comes to the head, and the husk goes. Otherwise pressing New first would be a
 * way to make a second copy of a request already there.
 *
 * Returns the request whose tab should be on screen.
 */
export function pasteOverBlank(blank: RestRequest, parsed: ParsedRequest): RestRequest {
  const now = Date.now();
  const existing = findRecentTarget(store.get(), parsed.method, parsed.url);
  if (existing !== undefined) {
    const lists = removeRequest(bumpRecent(store.get(), existing.id, now), blank.id);
    store.publish(lists);
    persistRequests(lists);
    return findRequest(lists, existing.id) ?? existing;
  }
  const filled: RestRequest = { ...blank, ...parsed };
  const lists = moveToRecent(store.get(), filled, now);
  store.publish(lists);
  persistRequests(lists);
  return findRequest(lists, blank.id) ?? filled;
}

/** Pinning a Recent request: it moves to Saved and stops being something that can be evicted. */
export function pinRequest(id: string): void {
  const lists = pinToSaved(store.get(), id);
  store.publish(lists);
  persistRequests(lists);
}

export function deleteRequest(id: string): void {
  const lists = removeRequest(store.get(), id);
  store.publish(lists);
  persistRequests(lists);
}

/** Resolves once the file has been read. **Sync waits on it**: reading an unloaded store would
 *  report every saved request as deleted. */
export function requestsReady(): Promise<void> {
  return store.ready();
}

/** Sync's write: the Saved list as another machine left it, Recent untouched. Resolves once it is
 *  on disk and rejects when it is not. */
export function replaceSaved(saved: RestRequest[]): Promise<void> {
  const lists = { ...store.get(), saved };
  store.publish(lists);
  return saveRequests(lists);
}
