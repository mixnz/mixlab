import { useCallback, useEffect, useEffectEvent, useRef, useState } from "react";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import { initialFilterRows, type FilterRow } from "../../filters";
import type { SqlApi } from "../../sql/api";
import type { FilterOperator, SqlFilter } from "../../sql/filters";
import type { SqlColumnMeta } from "../../types";
import { cacheKey as tableCacheKey } from "../../schemaTokens";
import { rememberedFilters, type FilterCache } from "./filters";
import {
  fileTable,
  rememberedTable,
  sameRequest,
  type Sort,
  type TableCache,
  type TableRequest,
} from "./request";

const DEFAULT_PAGE_SIZE = 1000;

interface Options {
  api: SqlApi;
  active: boolean;
  connectionId: string;
  selectedDb: string;
  selectedTable: string;
  schemaToken: number;
  filterCache: FilterCache;
  tableCache: TableCache;
  onError: (message: string) => void;
}

/** What the grid shows and where it came from: the page, the order and the conditions it asks for,
 *  the rows that answered, and the memory of both that outlives a trip to another table. */
export default function useTableData({
  api,
  active,
  connectionId,
  selectedDb,
  selectedTable,
  schemaToken,
  filterCache,
  tableCache,
  onError,
}: Options) {
  const { t } = useTranslation();
  const tableKey = tableCacheKey(selectedDb, selectedTable);
  // Everything below opens on what this table was last left showing, when it has been here before.
  // A grid mounted afresh — the connection reopened, another database picked and this one come back
  // to — is then the grid that was left, rather than a first read of the table all over again.
  const restored = rememberedTable(tableCache, tableKey, schemaToken);
  const [page, setPage] = useState(restored?.request.page ?? 0);
  const [pageSize, setPageSize] = useState(restored?.request.pageSize ?? DEFAULT_PAGE_SIZE);
  const [rows, setRows] = useState<Record<string, unknown>[]>(restored?.rows ?? []);
  const [columns, setColumns] = useState<string[]>(restored?.columns ?? []);
  const [columnMeta, setColumnMeta] = useState<Record<string, SqlColumnMeta>>(
    restored?.columnMeta ?? {},
  );
  const [primaryKey, setPrimaryKey] = useState<string[]>(restored?.primaryKey ?? []);
  const [autoIncrementColumn, setAutoIncrementColumn] = useState<string | null>(
    restored?.autoIncrementColumn ?? null,
  );
  const [total, setTotal] = useState(restored?.total ?? 0);
  const [sort, setSort] = useState<Sort | null>(restored?.request.sort ?? null);
  // The filter bar edits `filterRows` freely; only Apply copies them into `appliedFilters`, which
  // is what the fetch below reads. Keeping the two apart is what stops a half-typed condition
  // from reloading the grid on every keystroke.
  //
  // Both start from whatever this table's bar was left carrying, so a grid mounted afresh — the
  // connection reopened, or a table picked after none was — opens on the conditions it closed on.
  // A trip to Structure or Query no longer comes through here at all: the grid stays mounted
  // behind those tabs, bar and all.
  const [filterRows, setFilterRows] = useState<FilterRow<FilterOperator>[]>(
    () => rememberedFilters(filterCache, tableKey, schemaToken)?.rows ?? []
  );
  const [appliedFilters, setAppliedFilters] = useState<SqlFilter[]>(
    () => rememberedFilters(filterCache, tableKey, schemaToken)?.applied ?? []
  );
  // The table whose columns the bar was last seeded from. The seed needs the column list, which
  // is only known once the first fetch lands (or from the cache, when there is one).
  const filtersSeededForRef = useRef<string | null>(null);

  /** Where the grid is scrolled to, kept up to date as it moves. This, and not the box itself, is
   *  what the position is read from — the box cannot be asked at either of the two moments it
   *  matters. On the way out React has already detached the ref, and a pane hidden behind another
   *  tab has no layout box at all; both would answer "the top", and filing that would lose a grid
   *  left halfway down. */
  const scrollPosRef = useRef({ top: restored?.scrollTop ?? 0, left: restored?.scrollLeft ?? 0 });

  // The request the rows on screen came from. Held from one render to the next so that coming back
  // to the Data tab can tell "nothing has changed since these rows were read" — which is free — from
  // "the table, the page, the order or the conditions moved while the tab was hidden", which is a
  // read owed. It is also what says which conditions the rows answer, so that what is filed away for
  // the next visit is a page of rows and the very filters that produced it.
  const requestRef = useRef<TableRequest | null>(null);

  /**
   * Files the grid away as it stands, under the table it belongs to, for the next visit to open on.
   *
   * Only what a read actually produced is filed. Until the first one lands there is nothing here
   * worth keeping — an entry written then would be restored later as a grid that has already been
   * read, every row of it empty, and the fetch that should have filled it is the very thing the
   * entry says is not owed. And the request filed is the one the rows came from, never the one the
   * form is asking for next: conditions applied while the read was still out belong to the rows
   * that answer them, not to the rows already on screen.
   */
  function rememberTable(key: string) {
    const loaded = requestRef.current;
    if (columns.length === 0 || loaded === null) return;
    fileTable(tableCache, key, {
      columns,
      columnMeta,
      primaryKey,
      autoIncrementColumn,
      rows,
      total,
      request: loaded,
      scrollTop: scrollPosRef.current.top,
      scrollLeft: scrollPosRef.current.left,
    });
  }

  /** The bar as it was last committed, for the two writes that cannot read the state themselves: the
   * render that first sees another table — where `filterRows` still belongs to the outgoing one, but
   * `schemaToken` is already counted for the incoming one — and a cleanup, which runs long after the
   * last render. Declared above both so either can reach it. */
  const filterStateRef = useRef({
    key: tableKey,
    rows: filterRows,
    applied: appliedFilters,
    schemaToken,
  });
  useEffect(() => {
    filterStateRef.current = {
      key: viewTableKey,
      rows: filterRows,
      applied: appliedFilters,
      schemaToken,
    };
  });

  /**
   * Files the bar away under the table it belongs to and the shape of the database it was written
   * against. That shape is what a later visit judges it by: conditions the workspace has let go of
   * are filed straight back from here, and the token is the only thing that tells them apart from
   * conditions that still mean something.
   *
   * Only a bar that has had its opening row is worth remembering. Before the columns land there is
   * nothing in it, and filing that away would read on the way back in as a bar deliberately
   * emptied — leaving the table with no `id =` row to start from, for good.
   */
  // A callback rather than an effect event: the table swap below calls it during render.
  const rememberFilters = useCallback(() => {
    const { key, rows, applied, schemaToken: token } = filterStateRef.current;
    if (filtersSeededForRef.current !== key) return;
    filterCache.set(key, { rows, applied, schemaToken: token });
  }, [filterCache]);

  // What the fetch below reads — the page, the order and the conditions — is about one table, so
  // it is swapped over here, during the render that first sees a new table, rather than from an
  // effect. An effect would be too late: it and the fetch's own effect run after the same commit,
  // so the request would already have gone out naming the new table with the previous one's page,
  // sort column and filters — and a condition on a column the new table hasn't got comes back an
  // error rather than an empty result.
  const [viewTableKey, setViewTableKey] = useState(tableKey);
  if (viewTableKey !== tableKey) {
    // Put the outgoing table's bar away before its state is replaced. This is where it has to
    // happen: by the time any effect runs, `filterRows` already belongs to the new table.
    rememberFilters();
    rememberTable(viewTableKey);
    setViewTableKey(tableKey);
    // A table that has been here before opens where it was left rather than at its first page in
    // its own order; `pageSize` falls back to the one in hand rather than to the default, so a
    // size chosen for the last table is still carried onto a table never seen before.
    setPage(restored?.request.page ?? 0);
    setPageSize(restored?.request.pageSize ?? pageSize);
    setSort(restored?.request.sort ?? null);
    // The rows of the bar are put back alongside the columns they name, in the effect below.
    setAppliedFilters(rememberedFilters(filterCache, tableKey, schemaToken)?.applied ?? []);
  }

  // The same swap, for the app having changed this table under the grid rather than the user
  // having moved to another one. The conditions that were running were written against columns
  // that may since have been renamed or dropped, and sending them again would answer the table
  // with `Unknown column` rather than with rows; the workspace has already let go of the bar they
  // came from, so this picks up nothing and the grid opens unfiltered. Here rather than in the
  // effect below for the reason the swap above is: by then the fetch has already gone out.
  const [viewSchemaToken, setViewSchemaToken] = useState(schemaToken);
  if (viewSchemaToken !== schemaToken) {
    setViewSchemaToken(schemaToken);
    setAppliedFilters(rememberedFilters(filterCache, tableKey, schemaToken)?.applied ?? []);
  }

  // The bar is put away on the way out as well as on the way to another table: the grid is
  // unmounted when the connection tab closes, and what it was carrying should be there again if
  // the same table is opened later.
  useEffect(() => {
    return () => {
      rememberFilters();
    };
  }, [rememberFilters]);

  /** The grid as it stands, for the write on the way out — the same reason `filterStateRef` exists:
   *  a cleanup runs long after the last render and cannot read the state itself. */
  const tableStateRef = useRef<(key: string) => void>(rememberTable);
  useEffect(() => {
    tableStateRef.current = rememberTable;
  });

  // Filed away on the way out as well as on the way to another table: this is unmounted when the
  // sidebar loses its selection — picking another database does it — and what was on screen should
  // be there again when the table is opened later.
  useEffect(() => {
    return () => {
      tableStateRef.current(filterStateRef.current.key);
    };
  }, []);

  const [loading, setLoading] = useState(false);
  // Bumped by the reload action, by an insert and by a delete, to re-run the fetch below with the
  // request otherwise unchanged.
  const [reloadToken, setReloadToken] = useState(0);

  /** Marks the rows a remembered table came back with as loaded — or not — for the fetch below.
   *
   *  What those rows were read with, so the fetch can tell that nothing is owed — but only if the
   *  bar is asking the same question the rows answer. The two are kept apart (the bar in
   *  `filterCache`, the rows here), and conditions applied while a read was still out are filed
   *  with no rows to match, so the pair has to be checked rather than assumed. By value: the arrays
   *  come from different places, and the fetch's own identity check is the one that has to be
   *  satisfied afterwards, which is why this render's array is what gets marked as loaded.
   *
   *  An effect event because it reads the applied filters and the reload token as they stand, and
   *  must not be what makes the table swap below run: a condition applied is a refetch, not a
   *  table put back from memory. */
  const markRememberedRows = useEffectEvent((cached: TableRequest) => {
    requestRef.current =
      JSON.stringify(cached.filters) === JSON.stringify(appliedFilters)
        ? { ...cached, filters: appliedFilters, reloadToken }
        : null;
  });

  // Runs when the selected table changes, and when the database's shape does (not on page/pageSize
  // changes). A table that has been here before is put back as it was left — its rows included, so
  // the trip costs nothing and shows what it showed; otherwise everything is cleared so the grid
  // stays hidden until the fetch below resolves.
  useEffect(() => {
    // The page, the size, the sort and the applied filters have already been put back above, during
    // the render that saw the table change — see the note there.
    const key = tableCacheKey(selectedDb, selectedTable);
    const cached = rememberedTable(tableCache, key, schemaToken);
    // Where the grid goes back to once its rows are in the DOM — see `useGridLayout`'s layout
    // effect. Set here rather than left to the box itself, which is at the top whatever the last
    // table did.
    scrollPosRef.current = { top: cached?.scrollTop ?? 0, left: cached?.scrollLeft ?? 0 };
    // A table that has been here before gets its own bar back; only a first visit is seeded with
    // the opening `id =` row.
    const remembered = rememberedFilters(filterCache, key, schemaToken);
    if (cached) {
      setColumns(cached.columns);
      setColumnMeta(cached.columnMeta);
      setPrimaryKey(cached.primaryKey);
      setAutoIncrementColumn(cached.autoIncrementColumn);
      setRows(cached.rows);
      setTotal(cached.total);
      setFilterRows(remembered?.rows ?? initialFilterRows(cached.columns, "eq"));
      filtersSeededForRef.current = key;
      markRememberedRows(cached.request);
    } else {
      setRows([]);
      setTotal(0);
      setColumns([]);
      setColumnMeta({});
      setPrimaryKey([]);
      setAutoIncrementColumn(null);
      setFilterRows(remembered?.rows ?? []);
      // Nothing is owed a seed once the bar has been restored, or the fetch would overwrite it.
      filtersSeededForRef.current = remembered ? key : null;
      // Nothing read for this table yet, so the fetch below is owed one — whatever the previous
      // table left marked here.
      requestRef.current = null;
    }
    // `schemaToken` is in here as well as the table: a change the app made to this database leaves
    // what is on screen describing a shape the server no longer has, and running this again is what
    // empties the grid and marks a read owed. The two caches are the workspace's, one each for its
    // life, so they never run it.
  }, [selectedDb, selectedTable, schemaToken, tableCache, filterCache]);

  useEffect(() => {
    // Nothing is read for a tab nobody is looking at: the sidebar walked while the Structure tab is
    // up would otherwise send a page of rows and a count per table passed over.
    if (!active) return;
    // Built here rather than during render: it is this read's, and every field of it is already
    // something this effect runs again for.
    const request: TableRequest = {
      connectionId,
      db: selectedDb,
      table: selectedTable,
      page,
      pageSize,
      sort,
      filters: appliedFilters,
      reloadToken,
      schemaToken,
    };
    if (sameRequest(requestRef.current, request)) return;

    const db = selectedDb;
    const table = selectedTable;
    let cancelled = false;
    setLoading(true);
    api.tableData(connectionId, db, table, {
      page,
      pageSize,
      sortColumn: sort?.column ?? null,
      sortDesc: sort?.desc ?? false,
      filters: appliedFilters,
    })
      .then((result) => {
        if (cancelled) return;
        const key = tableCacheKey(db, table);
        // What was just read is what the next visit to this table opens on. Filed here rather than
        // only on the way out: a read that landed is worth keeping even if the app is closed on
        // this very table, and the way out has nothing to add beyond where the grid is scrolled to.
        fileTable(tableCache, key, {
          columns: result.columns,
          columnMeta: result.columnMeta,
          primaryKey: result.primaryKey,
          autoIncrementColumn: result.autoIncrementColumn,
          rows: result.rows,
          total: result.total,
          request,
          // Where the grid is scrolled to right now, which for a first read of a table and for a
          // reload is the top, and otherwise is wherever the user had got to within the page.
          scrollTop: scrollPosRef.current.top,
          scrollLeft: scrollPosRef.current.left,
        });
        // First look at this table's columns — the bar has been waiting for them to put its
        // opening `id` row together.
        if (filtersSeededForRef.current !== key) {
          setFilterRows(initialFilterRows(result.columns, "eq"));
          filtersSeededForRef.current = key;
        }
        setRows(result.rows);
        setColumns(result.columns);
        setColumnMeta(result.columnMeta);
        setPrimaryKey(result.primaryKey);
        setAutoIncrementColumn(result.autoIncrementColumn);
        setTotal(result.total);
        // Only a read that landed counts: a request that failed leaves nothing marked, so coming
        // back to the tab tries again rather than settling on an empty grid.
        requestRef.current = request;
      })
      .catch((e) => {
        // The table on screen is not the one this read was for. Its failure belongs to a table the
        // user has already left, and reported here it reads as this one's.
        if (cancelled) return;
        onError(errorMessage(t, e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [
    connectionId,
    selectedDb,
    selectedTable,
    page,
    pageSize,
    sort,
    appliedFilters,
    reloadToken,
    schemaToken,
    active,
    // One each for the life of the workspace — the engine's api, its caches, the setter its error
    // banner reads — so listing them costs no read.
    api,
    tableCache,
    onError,
    t,
  ]);

  return {
    page,
    setPage,
    pageSize,
    setPageSize,
    rows,
    setRows,
    columns,
    columnMeta,
    primaryKey,
    autoIncrementColumn,
    total,
    sort,
    setSort,
    filterRows,
    setFilterRows,
    appliedFilters,
    setAppliedFilters,
    loading,
    reloadToken,
    setReloadToken,
    scrollPosRef,
  };
}
