import { useCallback, useEffect, useEffectEvent, useLayoutEffect, useRef, useState } from "react";
import {
  mongoCollectionPage,
  mongoDeleteDocument,
  mongoInsertDocuments,
  mongoNextIds,
  mongoUpdateDocument,
  type DocUpdateOps,
} from "../../mongo/api";
import type { TypedDocument, TypedValue } from "../../mongo/bsonTypes";
import {
  mergeDocumentFields,
  MONGO_FILTER_OPERATORS,
  mongoOperatorArity,
  type MongoFilter,
  type MongoFilterOperator,
} from "../../mongo/filters";
import ActionBar from "../../../../components/ActionBar";
import Document from "../Document";
import FilterBar from "../FilterBar";
import InsertDocumentsDialog from "../InsertDocumentsDialog";
import LoadingOverlay from "../../../../components/LoadingOverlay";
import Pagination from "../../../../components/Pagination";
import { PlusIcon, ReloadIcon } from "../../../../icons";
import { useTranslation } from "../../../../i18n";
import { errorMessage } from "../../../../core/errors";
import { useReloadShortcut, withReloadShortcut } from "../../../../core/reload";
import { initialFilterRows, toQueryFilters, type FilterRow } from "../../filters";
import {
  fileDocuments,
  rememberedDocuments,
  sameRequest,
  type DocumentCache,
  type DocumentRequest,
} from "./request";
import styles from "./NoSqlTable.module.css";
import { cacheKey } from "../../schemaTokens";

interface Props {
  /** Whether this is what the user is actually looking at — the Data tab, in the connection tab the
   *  tab bar is showing. This stays mounted behind both, so it is what says when a page of
   *  documents is worth reading, when a reload the user cannot see would be wasted, and which of
   *  the panes mounted at once `Ctrl+R` belongs to. */
  active: boolean;
  connectionId: string;
  selectedDb: string;
  selectedCollection: string;
  onError: (message: string) => void;
  layoutWidth?: number;
  /** Where the filter bar is kept between visits — see {@link FilterCache}. */
  filterCache: FilterCache;
  /** Where the documents themselves are kept between visits — see {@link DocumentCache}. */
  documentCache: DocumentCache;
  /** Which shape of this database the cache is allowed to speak for. The workspace moves it
   *  whenever the app changes that shape, and everything remembered under the shape before is then
   *  read again rather than shown — see {@link DocumentRequest.schemaToken}. */
  schemaToken: number;
  /** Told when a document has been inserted or deleted here. What the collection holds has changed,
   *  which the list catches up with itself; the figures the Statistics tab has read for the database
   *  are counted from the same documents, and it is the workspace that holds those. */
  onDocumentsChanged?: () => void;
  /** The connection is marked read-only: the insert button is closed and the cards below neither
   *  open for editing nor offer a delete or a clone. Everything that reads is untouched. */
  readOnly?: boolean;
}

/** Where a loaded page came from. Held in state rather than read off the props, so the write
 * helpers stay bound to the collection the documents on screen were fetched from — the props
 * already carry the newly selected collection while the old cards are still mounted. */
interface Target {
  connectionId: string;
  database: string;
  collection: string;
}

const DOC_PAGE_SIZES = [20, 50, 100, 200];

/** One collection's filter bar as it was left behind: the rows still being edited, the conditions
 * that were actually running against the list, and the fields the bar had learned to offer —
 * those are read off the documents that have been seen, which a restored filter may well have
 * narrowed to none. */
export interface RememberedFilters {
  rows: FilterRow<MongoFilterOperator>[];
  applied: MongoFilter[];
  fields: string[];
  /** Which shape of the database the bar was written against — see {@link Props.schemaToken}. */
  schemaToken: number;
}

/** Every collection's bar, by the collection it belongs to. Held by the workspace rather than
 * here: the list is unmounted whenever the header leaves the Data tab, and a cache living inside
 * it would go with it — the conditions have to outlive a trip to the Stats tab, not just a trip to
 * another collection. */
export type FilterCache = Map<string, RememberedFilters>;

/**
 * The bar remembered for a collection, or nothing when there is nothing worth speaking for.
 *
 * Conditions written before the app last changed this collection are nothing: the name they are
 * filed under may since have been dropped and given to a different collection altogether, whose
 * documents need carry no field the conditions name. Deleting the entry at the moment of the change
 * is not enough on its own — the list is still holding the bar in state and files it straight back
 * on the way out — so the check has to be here, where the cache is read.
 */
function rememberedFilters(
  cache: FilterCache,
  key: string,
  schemaToken: number,
): RememberedFilters | undefined {
  const entry = cache.get(key);
  return entry?.schemaToken === schemaToken ? entry : undefined;
}

/** The only field every document is guaranteed to carry, and so the one the bar can offer before
 * a page has been loaded to read any others off. */
const ID_FIELD = ["_id"];

/** A page of a collection, one Document card per row.
 *
 * This component is the only thing here that talks to the database: it fetches a page, and
 * it performs every write a card asks for, reporting the errors and showing the progress.
 * What a card has changed but not yet written — the working copy, the staged edits, the open
 * inline editor — belongs to Document. Cards only hand back a document id and the ops to
 * apply, plus a "save what you have staged" hook so a refetch can wait for them. */
function NoSqlTable({
  active,
  connectionId,
  selectedDb,
  selectedCollection,
  onError,
  filterCache,
  documentCache,
  schemaToken,
  onDocumentsChanged,
  readOnly = false,
}: Props) {
  const { t } = useTranslation();
  const collectionKey = cacheKey(selectedDb, selectedCollection);
  // Everything below opens on what this collection was last left showing, when it has been here
  // before. A list mounted afresh — the connection reopened, another database picked and this one
  // come back to — is then the list that was left, rather than a first read of it all over again.
  const restored = rememberedDocuments(documentCache, collectionKey, schemaToken);
  const [page, setPage] = useState(restored?.request.page ?? 0);
  const [pageSize, setPageSize] = useState(restored?.request.pageSize ?? 50);
  const [documents, setDocuments] = useState<TypedDocument[]>(restored?.documents ?? []);
  const [target, setTarget] = useState<Target | null>(
    restored ? { connectionId, database: selectedDb, collection: selectedCollection } : null,
  );
  const [total, setTotal] = useState(restored?.total ?? 0);
  const [loading, setLoading] = useState(false);
  // Bumped by the reload action, by a delete and by an insert, to run the fetch below again with
  // the conditions unchanged.
  const [reloadToken, setReloadToken] = useState(0);
  // Counted rather than a flag: a Save and a rename can overlap, and the last one to finish
  // is what should clear the indicator.
  const [writeCount, setWriteCount] = useState(0);
  // Bumped on every load and folded into each card's key, so cards remount and rebuild
  // themselves from the new data rather than carrying the previous page's state into it.
  const [loadId, setLoadId] = useState(0);
  // What the insert form opens on: an empty array for a blank document, or the documents a
  // clone starts from. `null` is the form being closed.
  const [insertSeeds, setInsertSeeds] = useState<TypedDocument[] | null>(null);
  // The filter bar edits `filterRows` freely; only Apply copies them into `appliedFilters`, which
  // is what the fetch below reads. Keeping the two apart is what stops a half-typed condition
  // from reloading the list on every keystroke.
  //
  // All three start from whatever this collection's bar was left carrying, so a list mounted
  // afresh — the connection reopened, or a collection picked after none was — opens on the
  // conditions it closed on. A trip to the Stats tab no longer comes through here at all: the list
  // stays mounted behind it, bar and all.
  const [filterRows, setFilterRows] = useState<FilterRow<MongoFilterOperator>[]>(
    () =>
      rememberedFilters(filterCache, collectionKey, schemaToken)?.rows ??
      initialFilterRows(ID_FIELD, "eq"),
  );
  const [appliedFilters, setAppliedFilters] = useState<MongoFilter[]>(
    () => rememberedFilters(filterCache, collectionKey, schemaToken)?.applied ?? [],
  );
  // What the field select offers: seeded from the collection's first page and added to by every
  // page after it, never narrowed — see `mergeDocumentFields`.
  const [fields, setFields] = useState<string[]>(
    () => rememberedFilters(filterCache, collectionKey, schemaToken)?.fields ?? ID_FIELD,
  );

  const scrollRef = useRef<HTMLDivElement>(null);

  /** How far down the cards the list is, kept up to date as it moves. This, and not the box itself,
   *  is what the position is read from — the box cannot be asked at either of the two moments it
   *  matters. On the way out React has already detached the ref, and a list hidden behind the Stats
   *  tab has no layout box at all; both would answer "the top", and filing that would lose a list
   *  left halfway down. */
  const scrollPosRef = useRef(restored?.scrollTop ?? 0);

  // The request the documents on screen came from. Held from one render to the next so that coming
  // back to the Data tab can tell "nothing has changed since these were read" — which is free —
  // from "the collection, the page or the conditions moved while the tab was hidden", which is a
  // read owed. It is also what says which conditions the documents answer, so that what is filed
  // away for the next visit is a page and the very filters that produced it.
  //
  // Empty even when a page has just been restored above: whether those documents answer the
  // conditions the bar is carrying is a comparison, not something a mount can assume, and the
  // effect below is where it is made.
  const requestRef = useRef<DocumentRequest | null>(null);

  /**
   * Files the list away as it stands, under the collection it belongs to, for the next visit.
   *
   * Only what a read actually produced is filed. Until the first one lands there is nothing here
   * worth keeping — an entry written then would be restored later as a list that has already been
   * read, with no cards in it, and the fetch that should have filled it is the very thing the entry
   * says is not owed. And the request filed is the one the documents came from, never the one the
   * bar is asking for next: conditions applied while the read was still out belong to the documents
   * that answer them, not to the ones already on screen.
   */
  function rememberDocuments(key: string) {
    const loaded = requestRef.current;
    if (loaded === null) return;
    fileDocuments(documentCache, key, {
      documents,
      total,
      request: loaded,
      scrollTop: scrollPosRef.current,
    });
  }

  const flushersRef = useRef<Set<() => Promise<void>>>(new Set());

  /** Called by each card on mount; returns its own unregister function. */
  const registerFlush = useCallback((flush: () => Promise<void>) => {
    flushersRef.current.add(flush);
    return () => {
      flushersRef.current.delete(flush);
    };
  }, []);

  const flushAll = useCallback(async () => {
    await Promise.all(Array.from(flushersRef.current, (flush) => flush()));
  }, []);

  /** The bar as it was last committed, for the two writes that cannot read the state themselves:
   * the render that first sees another collection — where `filterRows` still belongs to the
   * outgoing one, but `schemaToken` is already counted for the incoming one — and a cleanup, which
   * runs long after the last render. Declared above both so either can reach it. */
  const filterStateRef = useRef({
    key: collectionKey,
    rows: filterRows,
    applied: appliedFilters,
    fields,
    schemaToken,
  });
  useEffect(() => {
    filterStateRef.current = {
      key: viewCollectionKey,
      rows: filterRows,
      applied: appliedFilters,
      fields,
      schemaToken,
    };
  });

  /** Files the bar away under the collection it belongs to and the shape of the database it was
   * written against. That shape is what a later visit checks it by: conditions the workspace has
   * let go of are filed straight back from here, and only the token tells them apart from
   * conditions that still mean something. */
  // A callback rather than an effect event: the collection swap below calls it during render.
  const rememberFilters = useCallback(() => {
    const { key, ...remembered } = filterStateRef.current;
    filterCache.set(key, remembered);
  }, [filterCache]);

  // Everything above is about one collection, and the page and the applied filters are what the
  // fetch below reads. They are swapped over here, during the render that first sees a new
  // collection, rather than from an effect: an effect would run after the same commit as the
  // fetch's own, so the request would already have gone out naming the new collection with the
  // previous one's page and conditions — a filter on a field these documents don't carry, which
  // comes back matching nothing.
  const [viewCollectionKey, setViewCollectionKey] = useState(collectionKey);
  if (viewCollectionKey !== collectionKey) {
    // Put the outgoing collection's bar away before its state is replaced. This is where it has
    // to happen: by the time any effect runs, `filterRows` already belongs to the new collection.
    rememberFilters();
    rememberDocuments(viewCollectionKey);
    // A collection that has been here before gets its own bar back. Only a first visit starts
    // over on an empty `_id =` row and the one field every document is known to carry — a filter
    // names a field, and these documents need not carry one by that name.
    const remembered = rememberedFilters(filterCache, collectionKey, schemaToken);
    setViewCollectionKey(collectionKey);
    // And opens on the page it was left on rather than at its first; `pageSize` falls back to the
    // one in hand rather than to the default, so a size chosen for the last collection is still
    // carried onto a collection never seen before.
    setPage(restored?.request.page ?? 0);
    setPageSize(restored?.request.pageSize ?? pageSize);
    setAppliedFilters(remembered?.applied ?? []);
    setFilterRows(remembered?.rows ?? initialFilterRows(ID_FIELD, "eq"));
    setFields(remembered?.fields ?? ID_FIELD);
  }

  // The same swap, for the app having changed this collection under the list rather than the user
  // having moved to another one. The conditions that were running were written against fields the
  // documents need no longer carry, and the workspace has already let go of the bar they came
  // from, so this picks up nothing and the list opens unfiltered. The whole bar goes, not only what
  // was applied: rows left standing would be filed back under the new shape on the way out, which
  // is the entry the workspace has just thrown away arriving as one that still means something.
  const [viewSchemaToken, setViewSchemaToken] = useState(schemaToken);
  if (viewSchemaToken !== schemaToken) {
    setViewSchemaToken(schemaToken);
    const remembered = rememberedFilters(filterCache, collectionKey, schemaToken);
    setAppliedFilters(remembered?.applied ?? []);
    setFilterRows(remembered?.rows ?? initialFilterRows(ID_FIELD, "eq"));
    setFields(remembered?.fields ?? ID_FIELD);
  }

  // The bar is put away on the way out as well as on the way to another collection: the list is
  // unmounted when the sidebar loses its selection — picking another database does it — and the
  // conditions should be there again when the collection is opened later.
  useEffect(() => {
    return () => {
      rememberFilters();
    };
  }, [rememberFilters]);

  /** The list as it stands, for the write on the way out — the same reason `filterStateRef` exists:
   *  a cleanup runs long after the last render and cannot read the state itself. */
  const documentStateRef = useRef<(key: string) => void>(rememberDocuments);
  useEffect(() => {
    documentStateRef.current = rememberDocuments;
  });

  useEffect(() => {
    return () => {
      documentStateRef.current(filterStateRef.current.key);
    };
  }, []);

  /** Asks for the page again with everything else unchanged — what a document deleted and an
   *  insert want, which is to see the page they were made on as it now stands. */
  const reload = useCallback(() => setReloadToken((n) => n + 1), []);

  /** What the reload button and `Ctrl+R` do: refetch from the first page.
   *
   * Back to page one on purpose: a reload is asked for when the collection is expected to have
   * moved underneath the list, and the documents the user wants to see after that are the newest
   * ones — a page counted off a collection that has since grown or shrunk names different documents
   * anyway. The token is bumped as well as the page reset, so a reload pressed on page one is still
   * a refetch rather than a no-op. */
  const reloadFromFirstPage = useCallback(() => {
    // And back to the head of it: what the first page is worth is the documents at its top, and an
    // offset carried over from halfway down page five would land the list in the middle of page one
    // instead. Read by the layout effect below when the new documents arrive.
    scrollPosRef.current = 0;
    setPage(0);
    setReloadToken((n) => n + 1);
  }, []);

  /** Marks the documents a remembered collection came back with as loaded — or not — for the fetch
   *  below.
   *
   *  What those documents were read with, so the fetch can tell that nothing is owed — but only if
   *  the bar is asking the same question they answer. The two are kept apart (the bar in
   *  `filterCache`, the documents here), and conditions applied while a read was still out are
   *  filed with nothing to match, so the pair has to be checked rather than assumed. By value: the
   *  arrays come from different places, and the fetch's own identity check is the one that has to
   *  be satisfied afterwards, which is why this render's array is what gets marked.
   *
   *  An effect event because it reads the applied filters and the reload token as they stand, and
   *  must not be what makes the collection swap below run: a condition applied is a refetch, not a
   *  collection put back from memory. */
  const markRememberedDocuments = useEffectEvent((cached: DocumentRequest) => {
    requestRef.current =
      JSON.stringify(cached.filters) === JSON.stringify(appliedFilters)
        ? { ...cached, filters: appliedFilters, reloadToken }
        : null;
  });

  // Runs when the selected collection changes, and when the database's shape does (not on
  // page/pageSize changes). A collection that has been here before is put back as it was left —
  // its documents included, so the trip costs nothing and shows what it showed; otherwise the
  // cards are cleared and the fetch below is owed a read.
  useEffect(() => {
    const key = cacheKey(selectedDb, selectedCollection);
    const cached = rememberedDocuments(documentCache, key, schemaToken);
    // Where the list goes back to once its cards are in the DOM — see the layout effect below. Set
    // here rather than left to the box itself, which is at the top whatever the last one did.
    scrollPosRef.current = cached?.scrollTop ?? 0;
    // Cards are keyed on this, so it moves whenever the documents under them are replaced: a card
    // must never carry the state it built for one document onto another at the same index.
    setLoadId((n) => n + 1);
    if (cached) {
      setDocuments(cached.documents);
      setTotal(cached.total);
      setTarget({ connectionId, database: selectedDb, collection: selectedCollection });
      markRememberedDocuments(cached.request);
    } else {
      setDocuments([]);
      setTotal(0);
      setTarget(null);
      // Nothing read for this collection yet, so the fetch below is owed one — whatever the
      // previous collection left marked here.
      requestRef.current = null;
    }
    // `schemaToken` is in here as well as the collection: a change the app made to this database
    // leaves what is on screen describing something the server no longer has, and running this
    // again is what empties the list and marks a read owed. The cache is the workspace's, one for
    // its life, so it never runs it.
  }, [connectionId, selectedDb, selectedCollection, schemaToken, documentCache]);

  useEffect(() => {
    // Nothing is read for a tab nobody is looking at: the sidebar walked while the Stats tab is up
    // would otherwise send a page of documents and a count per collection passed over.
    if (!active) return;
    // Built here rather than during render: it is this read's, and every field of it is already
    // something this effect runs again for.
    const request: DocumentRequest = {
      connectionId,
      db: selectedDb,
      collection: selectedCollection,
      page,
      pageSize,
      filters: appliedFilters,
      reloadToken,
      schemaToken,
    };
    if (sameRequest(requestRef.current, request)) return;

    const db = selectedDb;
    const collection = selectedCollection;
    let cancelled = false;
    setLoading(true);
    mongoCollectionPage(connectionId, db, collection, page, pageSize, appliedFilters)
      .then((result) => {
        if (cancelled) return;
        if (result.documents.length === 0 && page > 0 && result.total > 0) {
          setPage((p) => Math.max(0, p - 1));
          return;
        }
        // What was just read is what the next visit to this collection opens on. Filed here rather
        // than only on the way out: a read that landed is worth keeping even if the app is closed
        // on this very collection, and the way out has nothing to add beyond the scroll position.
        fileDocuments(documentCache, cacheKey(db, collection), {
          documents: result.documents,
          total: result.total,
          request,
          // Where the list is scrolled to right now, which for a first read and for a reload is the
          // top, and otherwise is wherever the user had got to within the page.
          scrollTop: scrollPosRef.current,
        });
        // One batched update, so `target` can never describe a different collection than the
        // documents rendered alongside it.
        setDocuments(result.documents);
        setTarget({ connectionId, database: db, collection });
        setTotal(result.total);
        setLoadId((n) => n + 1);
        // Whatever these documents carry that the bar didn't know about yet is now filterable.
        setFields((known) => mergeDocumentFields(known, result.documents));
        // Only a read that landed counts: a request that failed leaves nothing marked, so coming
        // back to the tab tries again rather than settling on an empty list.
        requestRef.current = request;
      })
      .catch((e) => onError(errorMessage(t, e)))
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [
    connectionId,
    selectedDb,
    selectedCollection,
    page,
    pageSize,
    appliedFilters,
    reloadToken,
    schemaToken,
    active,
    onError,
    // One each for the life of the workspace, so listing them costs no read.
    documentCache,
    t,
  ]);

  // Back to where the list was left. `scrollPosRef` is the only record of that: hiding the pane
  // behind the Stats tab is `display: none`, which takes its layout box away and the browser's
  // memory of the position with it. `documents` is in the deps because that is what says the cards
  // are in the DOM — a commit earlier than theirs has nothing of the right height to scroll within
  // and the position would only be clamped away. Re-applying a position the box is already at
  // costs nothing, which is what makes running this on every change of documents harmless.
  useLayoutEffect(() => {
    const node = scrollRef.current;
    if (!node || !active) return;
    node.scrollTop = scrollPosRef.current;
  }, [selectedDb, selectedCollection, active, documents]);

  /** Every move of the scrollbar, noted so that the position survives the list being hidden,
   *  swapped for another collection or unmounted. A box with no layout is not believed: it reports
   *  the top, and that is the one answer that must not be filed. */
  function handleScroll() {
    const node = scrollRef.current;
    if (node && node.clientHeight > 0) scrollPosRef.current = node.scrollTop;
  }

  /** Applies one card's ops to its document. Resolves to whether the write landed, so the
   * card knows to adopt its edits or roll them back; the error itself is reported here. */
  const writeDocument = useCallback(
    async (id: TypedValue, ops: DocUpdateOps): Promise<boolean> => {
      if (!target) return false;
      setWriteCount((n) => n + 1);
      try {
        await mongoUpdateDocument(target.connectionId, target.database, target.collection, id, ops);
        return true;
      } catch (e) {
        onError(errorMessage(t, e));
        return false;
      } finally {
        setWriteCount((n) => n - 1);
      }
    },
    [target, onError, t],
  );

  /** The page as it stands, for the adopt below — which runs from a card's write and can land after
   *  the list has moved on, where this render's `documents` would say nothing about it. */
  const documentsRef = useRef(documents);
  documentsRef.current = documents;

  /**
   * A card's document as the server now holds it, taken into the page this list is holding.
   *
   * Each card owns its own working copy, so without this the list goes on holding every document as
   * it was read — and it is the list's copy that is filed away for the next visit, where a saved
   * change would then read as though it had never landed at all.
   *
   * Found by identity rather than by index or by `_id`: the array the card was rendered from may
   * since have been replaced — another collection selected, another page, a refetch — and then there
   * is nothing on screen for the write to be adopted into. What was filed for the collection it
   * belonged to still holds the document as it was read, and showing that again later would hide a
   * change that did land, so the entry goes and the next visit reads. `target` is the collection the
   * card was written to, the same way `writeDocument` above reads it.
   */
  const adoptDocument = useCallback(
    (fetched: TypedDocument, saved: TypedDocument) => {
      const at = documentsRef.current.indexOf(fetched);
      if (at < 0) {
        if (target) documentCache.delete(cacheKey(target.database, target.collection));
        return;
      }
      setDocuments((prev) =>
        prev[at] === fetched ? prev.map((doc, i) => (i === at ? saved : doc)) : prev,
      );
    },
    [target, documentCache],
  );

  const removeDocument = useCallback(
    async (id: TypedValue): Promise<boolean> => {
      if (!target) return false;
      // Deleting refetches the page, which replaces every card: give the others a chance to
      // write out what they have staged first. The card being deleted opts itself out.
      await flushAll();
      setWriteCount((n) => n + 1);
      try {
        await mongoDeleteDocument(target.connectionId, target.database, target.collection, id);
        reload();
        // The collection holds one document fewer, which the figures on the Stats tab were counted
        // from — and those are the workspace's, not this list's.
        onDocumentsChanged?.();
        return true;
      } catch (e) {
        onError(errorMessage(t, e));
        return false;
      } finally {
        setWriteCount((n) => n - 1);
      }
    },
    [target, flushAll, reload, onError, onDocumentsChanged, t],
  );

  /** Runs `action` once every card has written out its staged edits. Costs nothing when
   * nothing is staged — a card with no pending changes returns before it hits the network. */
  const flushThen = useCallback(
    (action: () => void) => {
      // A failed write is already reported by writeDocument; whatever else could surface here
      // must not leave the list stuck on a page the user has moved away from.
      flushAll()
        .catch((e) => onError(errorMessage(t, e)))
        .finally(action);
    },
    [flushAll, onError, t],
  );

  /** Runs the filter bar's rows against the collection. Like a reload, whatever the cards have
   * staged is written out first: the documents come back filtered, and a card holding unsaved
   * edits is about to be replaced. The result is a different set of documents, so the list goes
   * back to the first page — the page the user was on need not even exist under the new
   * conditions. */
  const applyFilters = useCallback(() => {
    // A new array every time on purpose: pressing Apply twice on the same conditions is a
    // request to refetch, and an equal-but-identical array would be a no-op.
    flushThen(() => {
      setAppliedFilters(toQueryFilters(filterRows, mongoOperatorArity));
      setPage(0);
    });
  }, [flushThen, filterRows]);

  /** Opens the insert form on `seeds` — an empty array for a blank document. Whatever the cards
   * have staged is written out first: the form is modal, so nothing would flush it while it is
   * up, and a clone seeded from a card should be inserted alongside the edits it was copied
   * from rather than instead of them. */
  const openInsert = useCallback(
    (seeds: TypedDocument[]) => flushThen(() => setInsertSeeds(seeds)),
    [flushThen],
  );

  // Handed to every card, so it has to keep one identity — the cards are memoized.
  const cloneDocument = useCallback((doc: TypedDocument) => openInsert([doc]), [openInsert]);

  const nextIds = useCallback(
    (count: number) => mongoNextIds(connectionId, selectedDb, selectedCollection, count),
    [connectionId, selectedDb, selectedCollection],
  );

  /** Hands the form's documents to the server. Errors are left to reject so the form can show
   * them and stay open with the drafts still in it. */
  async function submitInsert(documents: TypedDocument[]) {
    try {
      await mongoInsertDocuments(connectionId, selectedDb, selectedCollection, documents);
    } finally {
      // The insert is ordered rather than atomic, so a failure partway through still leaves the
      // documents before it written: refetch either way, tell the workspace the collection has
      // grown either way, and let the form report the error over the page it lands on.
      reload();
      onDocumentsChanged?.();
    }
    setInsertSeeds(null);
  }

  const busy = loading || writeCount > 0;
  const pageCount = Math.max(1, Math.ceil(total / pageSize));

  // Gated on the same state the button below is: a refetch asked for while one is already out, or
  // over a card mid-write, is one the button would refuse. Not from behind the insert form, which
  // holds documents that have been typed and not yet sent.
  useReloadShortcut(active && insertSeeds === null, () => {
    if (busy) return;
    flushThen(reloadFromFirstPage);
  });

  return (
    <div className={styles.noSqlTable}>
      <FilterBar
        fields={fields}
        operators={MONGO_FILTER_OPERATORS}
        defaultOperator="eq"
        operatorLabel={(op) => t(`noSqlTable.op.${op}`)}
        rows={filterRows}
        onChange={setFilterRows}
        onApply={applyFilters}
        applyDisabled={busy}
      />
      <div className={styles.scrollWrap}>
        <div className={styles.scroll} ref={scrollRef} onScroll={handleScroll}>
          {documents.length === 0 && !loading && <p className="muted">{t("noSqlTable.noDocuments")}</p>}
          {documents.map((doc, i) => (
            <Document
              key={`${loadId}:${i}`}
              doc={doc}
              displayNumber={page * pageSize + i + 1}
              readOnly={readOnly}
              registerFlush={registerFlush}
              onWrite={writeDocument}
              onSaved={adoptDocument}
              onDelete={removeDocument}
              // A clone is an insert seeded from this document, so it goes with the insert button
              // rather than staying on a card that can no longer write anything.
              onClone={readOnly ? undefined : cloneDocument}
            />
          ))}
        </div>
        {busy && <LoadingOverlay label={loading ? t("noSqlTable.loading") : t("noSqlTable.saving")} />}
      </div>
      <div className={styles.footer}>
        <ActionBar
          actions={[
            {
              key: "reload",
              icon: ReloadIcon,
              label: withReloadShortcut(t("noSqlTable.reloadDocuments")),
              disabled: busy,
              busy,
              // Same as changing page: whatever the cards have staged is written out before the
              // refetch replaces them.
              onClick: () => flushThen(reloadFromFirstPage),
            },
            {
              key: "insert",
              icon: PlusIcon,
              label: t("noSqlTable.insertDocuments"),
              disabled: readOnly || busy,
              disabledHint: readOnly ? t("common.readOnlyConnection") : undefined,
              onClick: () => openInsert([]),
            },
          ]}
        />
        <Pagination
          page={page}
          pageCount={pageCount}
          total={total}
          pageSize={pageSize}
          pageSizeOptions={DOC_PAGE_SIZES}
          loading={busy}
          onPageChange={(next) => flushThen(() => setPage(next))}
          onPageSizeChange={(n) =>
            flushThen(() => {
              setPageSize(n);
              setPage(0);
            })
          }
        />
      </div>
      {insertSeeds !== null && (
        <InsertDocumentsDialog
          collection={selectedCollection}
          seedDocs={insertSeeds}
          nextIds={nextIds}
          onCancel={() => setInsertSeeds(null)}
          onSubmit={submitInsert}
        />
      )}
    </div>
  );
}

export default NoSqlTable;
