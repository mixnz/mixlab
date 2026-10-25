import { Store } from "@tauri-apps/plugin-store";
import type { KeyValue, Method, RequestLists, RestRequest } from "./types";

/**
 * The request list on disk, and the pure reducers that shape it.
 *
 * Two groups, both flat: **Saved** is what someone chose to keep, **Recent** is what pasting a
 * cURL command left behind. Only the reducers are tested; the file access around them is four
 * lines of `Store` and has nothing to get wrong.
 */

/** How many pasted requests are kept. Filling up drops the one least recently *sent* — see the
 *  spec's §5. Enforced from Phase 2, which is where anything is put in Recent at all. */
export const RECENT_LIMIT = 10;

const FILE = "rest-requests.json";
const KEY = "lists";

let storePromise: Promise<Store> | null = null;

function getStore(): Promise<Store> {
  if (!storePromise) storePromise = Store.load(FILE);
  return storePromise;
}

/** An empty row for the Params or Headers table. Ticked, because a row is typed in to be used. */
export function newRow(id: string): KeyValue {
  return { id, enabled: true, key: "", value: "" };
}

/** A request as it starts life: a GET with nothing in it. */
export function newRequest(id: string, now: number): RestRequest {
  return {
    id,
    name: "",
    method: "GET",
    url: "",
    params: [],
    headers: [],
    body: { kind: "none" },
    auth: { kind: "none" },
    origin: "manual",
    createdAt: now,
    lastUsedAt: now,
  };
}

/**
 * A request nobody has put anything into.
 *
 * Pressing New opens a request that is already in the list — that is what makes every edit land
 * without a Save button. The cost is that changing your mind leaves a husk behind, and a sidebar
 * of them is what a hundred second thoughts look like. One is dropped when its tab closes and any
 * left over are swept on load, so the two paths that can strand one both clear up after
 * themselves.
 *
 * The bar is deliberately low: a method other than the GET it was born as, a row half typed, a
 * body that exists at all, or one press of Send is enough to keep it. Only a request identical to
 * a brand new one goes.
 */
export function isBlank(request: RestRequest): boolean {
  const untouched = (rows: KeyValue[]) => rows.every((row) => row.key === "" && row.value === "");
  return (
    request.name === "" &&
    request.url.trim() === "" &&
    request.method === "GET" &&
    untouched(request.params) &&
    untouched(request.headers) &&
    request.body.kind === "none" &&
    request.auth.kind === "none" &&
    // Sending stamps this; a request that has been used is a request, whatever is left in it.
    request.lastUsedAt === request.createdAt
  );
}

/** The lists with the husks taken out, or the lists themselves when there are none — so a load
 *  that changes nothing does not look like a change. */
export function sweepBlank(lists: RequestLists): RequestLists {
  const saved = lists.saved.filter((r) => !isBlank(r));
  const recent = lists.recent.filter((r) => !isBlank(r));
  if (saved.length === lists.saved.length && recent.length === lists.recent.length) return lists;
  return { saved, recent };
}

export function findRequest(lists: RequestLists, id: string): RestRequest | undefined {
  return lists.saved.find((r) => r.id === id) ?? lists.recent.find((r) => r.id === id);
}

/** Newest first, which is where someone looks for what they just made. */
export function addSaved(lists: RequestLists, request: RestRequest): RequestLists {
  return { ...lists, saved: [request, ...lists.saved] };
}

/**
 * The list with this request's new state in it, in the group it is already in.
 *
 * Editing a Recent request does **not** promote it to Saved — pinning is the only thing that
 * does. A request in neither group is one whose row went away while a tab on it stayed open, and
 * the list is returned untouched.
 */
export function updateRequest(lists: RequestLists, request: RestRequest): RequestLists {
  const swap = (list: RestRequest[]) => list.map((r) => (r.id === request.id ? request : r));
  return { saved: swap(lists.saved), recent: swap(lists.recent) };
}

/**
 * The Recent entry aimed at the same place, if there is one.
 *
 * Same method and same URL is what "the same command" means here: pasting one line twice should
 * leave one row, not two. Saved is not searched — a request someone kept is theirs, and reordering
 * or restamping it because a paste happened to match would be a surprise.
 */
export function findRecentTarget(
  lists: RequestLists,
  method: Method,
  url: string,
): RestRequest | undefined {
  return lists.recent.find((request) => request.method === method && request.url === url);
}

/**
 * Recent with the paste at its head and no more than {@link RECENT_LIMIT} rows in it.
 *
 * Recent is ordered newest paste first and evicts by `lastUsedAt` — two different orders, on
 * purpose. What falls off is the row least recently **sent**, so a request still used every day is
 * not pushed out by ten pastes; and `lastUsedAt` is stamped by sending, so opening a row to look at
 * it does not save it either.
 */
export function addRecent(lists: RequestLists, request: RestRequest): RequestLists {
  return { ...lists, recent: trimRecent([request, ...lists.recent]) };
}

function trimRecent(recent: RestRequest[]): RestRequest[] {
  if (recent.length <= RECENT_LIMIT) return recent;
  const byUse = recent.map((request, index) => ({ request, index }));
  // Least recently used first; a tie goes to whichever sits further down, which is the older paste.
  byUse.sort((a, b) => a.request.lastUsedAt - b.request.lastUsedAt || b.index - a.index);
  const dropped = new Set(
    byUse.slice(0, recent.length - RECENT_LIMIT).map((entry) => entry.request.id),
  );
  return recent.filter((request) => !dropped.has(request.id));
}

/** The same command pasted again: the row already there comes to the head of Recent and is stamped
 *  as used, instead of a second copy of it appearing. */
export function bumpRecent(lists: RequestLists, id: string, now: number): RequestLists {
  const found = lists.recent.find((request) => request.id === id);
  if (found === undefined) return lists;
  return {
    ...lists,
    recent: [{ ...found, lastUsedAt: now }, ...lists.recent.filter((request) => request.id !== id)],
  };
}

/**
 * A blank request filled by a paste: out of Saved and on to the head of Recent.
 *
 * The row was made by pressing New, but nothing in it was ever typed — everything it now holds came
 * from the command pasted over it, which is exactly what "a request a paste left behind" means. It
 * keeps its id, so the tab it is open in carries on as though nothing had happened, and it is
 * trimmed against the ceiling like any other paste.
 *
 * Both timestamps are stamped at the paste, which is what makes this the same row `addRecent` would
 * have made from the same command: as a pasted request it begins now, and the husk it is written
 * over carried the time New was pressed — an hour-old stamp would have the ceiling evict the very
 * row that was just pasted.
 *
 * The other direction is {@link pinToSaved}, and the two are not symmetrical on purpose: pinning is
 * someone saying they meant to keep this, while this is the app noticing that nothing here was
 * theirs to begin with.
 */
export function moveToRecent(
  lists: RequestLists,
  request: RestRequest,
  now: number,
): RequestLists {
  const pasted: RestRequest = { ...request, origin: "paste", createdAt: now, lastUsedAt: now };
  return {
    saved: lists.saved.filter((row) => row.id !== request.id),
    recent: trimRecent([pasted, ...lists.recent.filter((row) => row.id !== request.id)]),
  };
}

/**
 * Pinning: out of Recent and on to the top of Saved, where nothing evicts it.
 *
 * `origin` changes with it, because pinning is someone saying they meant to keep this — and it is
 * the **only** thing that moves a row between the groups. Editing a Recent request does not, which
 * is why a half-typed one can still be dropped when Recent fills up.
 */
export function pinToSaved(lists: RequestLists, id: string): RequestLists {
  const found = lists.recent.find((request) => request.id === id);
  if (found === undefined) return lists;
  return {
    saved: [{ ...found, origin: "manual" }, ...lists.saved],
    recent: lists.recent.filter((request) => request.id !== id),
  };
}

export function removeRequest(lists: RequestLists, id: string): RequestLists {
  return {
    saved: lists.saved.filter((r) => r.id !== id),
    recent: lists.recent.filter((r) => r.id !== id),
  };
}

/** What is on disk, or two empty groups. A file that cannot be read is an empty sidebar for the
 *  session, not a crash. */
export async function loadRequests(): Promise<RequestLists> {
  const store = await getStore();
  const stored = await store.get<RequestLists>(KEY);
  return sweepBlank({ saved: stored?.saved ?? [], recent: stored?.recent ?? [] });
}

/** Writes the list as it now stands, and rejects when the disk says no — what sync waits on,
 *  since it agrees only on what the disk holds. */
export async function saveRequests(lists: RequestLists): Promise<void> {
  const store = await getStore();
  await store.set(KEY, lists);
  await store.save();
}

/** The same for someone typing. Failures are swallowed: the list is still right in memory, and
 *  nothing here is worth interrupting someone's typing over. */
export function persistRequests(lists: RequestLists): void {
  void saveRequests(lists).catch(() => {});
}
