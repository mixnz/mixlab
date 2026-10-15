import { useEffect, useMemo, useState } from "react";
import { errorMessage } from "../../../../core/errors";
import { fileInto } from "../../../../core/paneCache";
import { useTranslation } from "../../../../i18n";
import { cacheKey } from "../../schemaTokens";
import { useSqlApi } from "../../sql/context";
import type { SqlCollation, SqlTableStructure } from "../../types";

/** One table's shape as it was last read, and which shape of the database it was read from — see
 * `TableStructure`'s `schemaToken` prop. */
export interface RememberedStructure {
  structure: SqlTableStructure;
  schemaToken: number;
}

/** Every table's shape, by the table it was read from. Held by the workspace rather than here: this
 * panel is unmounted whenever the sidebar has no table selected — changing database does it — and a
 * cache living in here would go with it. Keyed, not a single slot, so that a table come back to is
 * a table already read rather than the one thing the panel happened to be showing last. */
export type StructureCache = Map<string, RememberedStructure>;

/**
 * How many tables' shapes are kept before the one read longest ago is let go.
 *
 * An entry is small next to a page of rows — a few hundred columns and indexes at the very worst —
 * but a session spent walking a few thousand tables would hold every one of them for as long as the
 * connection stayed open, which is the sort of thing that is invisible until the machine starts
 * swapping. Twenty is well past however many tables anyone moves between in one piece of work, so
 * the cap is only ever met by the tables nobody is going back to. The same number the grid beside
 * this one keeps, for the same reason.
 */
const STRUCTURE_CACHE_LIMIT = 20;

interface Options {
  active: boolean;
  connectionId: string;
  selectedDb: string;
  selectedTable: string;
  schemaToken: number;
  structureCache: StructureCache;
  onError: (message: string) => void;
}

/** The table's shape on screen and where it came from: the cache it is filed in, the read that
 *  fills it, and the server's collations the column dialog picks from. */
export function useTableShape({
  active,
  connectionId,
  selectedDb,
  selectedTable,
  schemaToken,
  structureCache,
  onError,
}: Options) {
  const { t } = useTranslation();
  const api = useSqlApi();
  /** Bumped whenever the cache is written to or dropped, since a Map is the same object either way
   *  and nothing would re-render off it on its own. Only the setter is ever read: the count says
   *  nothing, it only says that the cache is worth looking at again. */
  const [, setCacheToken] = useState(0);
  const [loading, setLoading] = useState(false);
  const [collations, setCollations] = useState<SqlCollation[]>([]);

  // Read once per connection rather than per table: the list belongs to the server, and every
  // column dialog opened on it picks from the same one.
  useEffect(() => {
    let cancelled = false;
    api.collations(connectionId)
      .then((result) => {
        if (!cancelled) setCollations(result);
      })
      // Not worth an error banner over: without a list the dialog falls back to a text box, which
      // is what it was before there was one.
      .catch(() => {
        if (!cancelled) setCollations([]);
      });
    return () => {
      cancelled = true;
    };
  }, [api, connectionId]);

  /** The table the panel is showing, as one string: what the cache is keyed on. */
  const tableKey = cacheKey(selectedDb, selectedTable);
  /** What is filed for the table now selected. Read out of the cache by name, so another table's
   *  columns can never appear under this one's — and plainly rather than memoised: it is one map
   *  lookup, and a memo would only add a list of dependencies to get wrong. */
  const remembered = structureCache.get(tableKey);
  /** The columns and indexes on screen, or null when there are none to show. Columns read before
   *  the app last changed this database are none: the name they are filed under may since have
   *  been dropped and given to a different table altogether. */
  const structure = remembered?.schemaToken === schemaToken ? remembered.structure : null;

  useEffect(() => {
    // Nothing to do until the tab is looked at, and nothing to do once it has been read: this is
    // what makes moving between the tabs — and coming back to a table — free.
    if (!active || structure !== null) return;
    let cancelled = false;
    setLoading(true);
    api.tableStructure(connectionId, selectedDb, selectedTable)
      .then((result) => {
        if (cancelled) return;
        const entry = { structure: result, schemaToken };
        fileInto(structureCache, tableKey, entry, STRUCTURE_CACHE_LIMIT);
        setCacheToken((n) => n + 1);
      })
      .catch((e) => {
        if (cancelled) return;
        // Nothing is cached for a read that failed, so coming back to the tab tries again rather
        // than settling on an empty panel.
        onError(errorMessage(t, e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
    // `schemaToken` is in here so that a change made while a read is still out drops that read
    // rather than letting it land: columns filed under the shape before are turned down when read
    // back, and with nothing else moving there would be nothing left to ask for them again.
  }, [api, connectionId, selectedDb, selectedTable, tableKey, active, structure, schemaToken, onError, structureCache, t]);

  // Memoized because the grids (`ColumnsPanel`, `IndexesPanel`) are, on these: a fresh `[]` each render while nothing is
  // loaded would rebuild them on every render for no change at all.
  const columns = useMemo(() => structure?.columns ?? [], [structure]);
  const indexes = useMemo(() => structure?.indexes ?? [], [structure]);
  const skipIndexes = structure?.skipIndexes ?? [];
  const engine = structure?.engine ?? null;

  /** Dropping this table's entry is the reload: the effect above reads again the moment it finds
   *  nothing for the table on screen. Only this table's — the others were read from the server just
   *  as truthfully, and dropping them would turn one reload into a re-read of every table visited
   *  so far. */
  function reload() {
    structureCache.delete(tableKey);
    setCacheToken((n) => n + 1);
  }

  return { tableKey, structure, loading, collations, columns, indexes, skipIndexes, engine, reload };
}
