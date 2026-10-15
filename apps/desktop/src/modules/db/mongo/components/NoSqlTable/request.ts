import type { MongoFilter } from "../../filters";
import type { TypedDocument } from "../../bsonTypes";
import { fileInto } from "../../../../../core/paneCache";

/** Everything one page of documents is read with — what makes two reads the same read. */
export interface DocumentRequest {
  connectionId: string;
  db: string;
  collection: string;
  page: number;
  pageSize: number;
  filters: MongoFilter[];
  reloadToken: number;
  /** Which shape of the database the documents were read from. Moved by the workspace every time
   *  this app changes that shape — a collection created, renamed or dropped, a dump restored — so
   *  that documents read from the shape before are never mistaken for documents read from the one
   *  on screen now. A name is not a promise: a collection dropped and made again under the same
   *  name would otherwise open on the documents of the one it replaced. */
  schemaToken: number;
}

/**
 * Whether the documents read for `loaded` are the documents `wanted` is asking for — which is what
 * says whether coming back to the list costs a read or nothing at all.
 *
 * `filters` are compared by identity rather than by value, because the bar replaces them wholesale
 * rather than editing them: a fresh array is a fresh request even when it says the same thing, and
 * that is what makes Apply re-read on conditions that have not changed. `reloadToken` is what the
 * reload button moves, and it is in here so that a reload is never the one request answered from
 * what is already in hand; `schemaToken` is that same thing for a change the app itself made.
 */
export function sameRequest(loaded: DocumentRequest | null, wanted: DocumentRequest): boolean {
  return (
    loaded !== null &&
    loaded.connectionId === wanted.connectionId &&
    loaded.db === wanted.db &&
    loaded.collection === wanted.collection &&
    loaded.page === wanted.page &&
    loaded.pageSize === wanted.pageSize &&
    loaded.filters === wanted.filters &&
    loaded.reloadToken === wanted.reloadToken &&
    loaded.schemaToken === wanted.schemaToken
  );
}

/**
 * One collection's list as it was last left: the page that was read, and where the user was in it.
 *
 * The documents are kept, not only the count — coming back to a collection is meant to be coming
 * back to what was on screen, not to a fresh read of it. That does mean the cards can be behind the
 * server; the reload button, `Ctrl+R` and a change the app made itself are what say otherwise, and
 * they are the only things that do.
 */
export interface RememberedDocuments {
  documents: TypedDocument[];
  total: number;
  /** The read these documents came from — the page, the conditions and the shape of the database,
   *  together. Kept as one thing rather than as loose fields: documents and the filters they answer
   *  must never be put back separately, or the list would show one collection's page under
   *  another collection's conditions. */
  request: DocumentRequest;
  /** How far down the list of cards the user had got. Only the vertical: the cards are a column
   *  that fills the width, so there is nothing to be across. */
  scrollTop: number;
}

/** Every collection's list, by the collection it belongs to. Held by the workspace rather than by
 * the list, for the same reason the filter bar's cache is: the list is unmounted whenever the
 * sidebar has no collection selected — changing database does it — and a cache living in there
 * would go with it. */
export type DocumentCache = Map<string, RememberedDocuments>;

/**
 * What is remembered for a collection, or nothing when there is nothing worth speaking for.
 *
 * An entry read before the app last changed the database's shape is nothing: the name it is filed
 * under may since have been dropped and given to a different collection altogether. Emptying the
 * cache at the moment of the change is not enough on its own — the list on screen is still holding
 * its own copy in state and files it back on the way out — so the check has to be here, where the
 * cache is read.
 */
export function rememberedDocuments(
  cache: DocumentCache,
  key: string,
  schemaToken: number,
): RememberedDocuments | undefined {
  const entry = cache.get(key);
  return entry?.request.schemaToken === schemaToken ? entry : undefined;
}

/**
 * How many collections' lists are kept before the least recently left one is let go.
 *
 * An entry here is a whole page of documents, and a document is an arbitrarily deep tree rather
 * than a row of scalars — so if anything this is the cache with the most to hold per entry. Twenty
 * is well past however many collections anyone moves between in one piece of work.
 */
const DOCUMENT_CACHE_LIMIT = 20;

/** Files a collection's list away, letting the collection left longest ago go once it is full. */
export function fileDocuments(
  cache: DocumentCache,
  key: string,
  entry: RememberedDocuments,
): void {
  fileInto(cache, key, entry, DOCUMENT_CACHE_LIMIT);
}
