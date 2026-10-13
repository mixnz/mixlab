import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { fileInto } from "../../../../core/paneCache";
import { gridStyle, useVirtualRows, widestValues } from "../../../../core/virtualRows";
import ActionBar from "../../../../components/ActionBar";
import Input from "../../../../components/Input";
import LoadingOverlay from "../../../../components/LoadingOverlay";
import { ChevronDownIcon, ChevronUpIcon, ReloadIcon } from "../../../../icons";
import { useTranslation } from "../../../../i18n";
import { errorMessage } from "../../../../core/errors";
import { useReloadShortcut, withReloadShortcut } from "../../../../core/reload";
import { mongoCollectionStats } from "../../mongo/api";
import { useOptionalSql } from "../../sql/context";
import type { TableStats } from "../../types";
import styles from "./DatabaseStats.module.css";

/** The columns the grid can be ordered by — every field of a row, the name included. */
type SortColumn = keyof TableStats;

interface Sort {
  column: SortColumn;
  desc: boolean;
}

/** Binary units, the ones a database reports its own sizes in. */
const SIZE_UNITS = ["B", "KB", "MB", "GB", "TB", "PB"] as const;

/**
 * A byte count as the closest unit that keeps it readable — `1536` becomes `1.50 KB`. Two decimals
 * below ten and one above, so the number stays about as wide whatever it is.
 */
function formatSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < SIZE_UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const digits = unit === 0 ? 0 : value < 10 ? 2 : 1;
  return `${value.toFixed(digits)} ${SIZE_UNITS[unit]}`;
}

/** A count with its thousands separated, which is what makes two of them comparable at a glance. */
function formatCount(value: number): string {
  return value.toLocaleString();
}

/** Where holding every row in the DOM stops being the cheap thing to do. A database of a few dozen
 *  tables is nothing; one of a few thousand is the whole grid laid out again every time the tab is
 *  shown, since a tab behind another one keeps no layout. */
const VIRTUAL_FROM = 60;

/** How tall a row of this grid is — stated, never measured; see `virtualRows.ts` for why that
 *  distinction is what makes a window of rows sound. 29px is what a row here comes to: a 20px
 *  line, 4px of padding above and below, and the 1px rule underneath. Was 33px on a 24px line. */
const ROW_HEIGHT = 29;

/** How one row reads, column by column, for the sizer row that gives each column its width. The
 *  same strings the cells below are drawn from, so the column is sized by what is actually in it. */
function cellText(row: TableStats, column: number): string {
  switch (column) {
    case 1:
      return row.name;
    case 2:
      return formatCount(row.rows);
    case 3:
      return formatSize(row.dataSize);
    case 4:
      return formatSize(row.indexSize);
    case 5:
      return formatSize(row.avgRecordSize);
    // The row number, which the sizer fills with the largest one rather than reading it off a row.
    default:
      return "";
  }
}

/** One database's figures as they were last read, and the order they were last put in. `rows` is
 * null once a reload has dropped them but before the new ones land — the order outlives the figures
 * it was applied to, so a reload comes back in the order it was asked from. */
export interface RememberedStats {
  rows: TableStats[] | null;
  sort: Sort;
  /** Which shape of the database the figures were read from — see {@link Props.schemaToken}. The
   *  order beside them is not judged by it: what the server said about a table is out of date the
   *  moment the table changes, but the column the user chose to sort by is theirs either way. */
  schemaToken: number;
}

/** Every database's figures, by the database they were read from — so a database changed and come
 * back to shows what it showed, and never the previous one's figures under its name. Held by the
 * workspace rather than here, so that the figures outlive this panel being unmounted — which is
 * what selecting no database does. */
export type StatsCache = Map<string, RememberedStats>;

/**
 * How many databases' figures are kept before the one read longest ago is let go.
 *
 * An entry is one row per table, which for a database of a few thousand tables is a few thousand
 * rows — and a connection walked database by database is exactly what would otherwise hold all of
 * them at once, for as long as it stayed open. Twenty is past however many databases anyone moves
 * between in one piece of work.
 */
const STATS_CACHE_LIMIT = 20;

/** What a database is ordered by until the user says otherwise: the heaviest table first, which is
 * the question this panel exists to answer. */
const DEFAULT_SORT: Sort = { column: "dataSize", desc: true };

interface Props {
  kind: "mysql" | "postgres" | "mongo" | "sqlite" | "clickhouse" | "mssql";
  connectionId: string;
  /** The database being measured. Never empty: the workspace shows a prompt instead of mounting
   *  this until one is selected. */
  database: string;
  /** Whether this is what the user is actually looking at — the Stats tab, in the connection tab
   *  the tab bar is showing. This stays mounted behind both, so it is what says when the figures
   *  are worth reading, when a reload the user cannot see would be wasted, and which of the panes
   *  mounted at once `Ctrl+R` belongs to. */
  active: boolean;
  onError: (message: string) => void;
  /** Where the figures read for each database are kept between visits — see {@link StatsCache}. */
  statsCache: StatsCache;
  /** Which shape of this database the figures are allowed to speak for. Moved by the workspace
   *  whenever the app changes that shape — a table or collection created, dropped, altered, a dump
   *  restored — every one of which is a change to what the figures say. Figures filed under the
   *  shape before are then read again rather than shown; emptying the cache is not enough on its
   *  own, since a Map is the same object before and after and nothing re-renders off it. */
  schemaToken: number;
}

/**
 * What the selected database holds, table by table: how many rows, how much of the disk the rows
 * take, how much their indexes take, and how big an average row is.
 *
 * Ordered by data size to begin with rather than by name — the question this answers is which
 * table is the heavy one, and the sidebar next to it already lists them alphabetically.
 *
 * Read once and kept: the workspace leaves this mounted while another tab is up, and holds the
 * figures for every database beside it, so moving between the tabs — or between databases — shows
 * what was read rather than asking for it again. They are only re-read when the reload button asks
 * or the app itself has changed the database, and either while the tab is hidden waits until it is
 * looked at again before costing anything.
 *
 * Every database reports the same four numbers, so one grid serves them all; only what the columns
 * are called changes, since the SQL engines count rows in tables and MongoDB documents in
 * collections. None of the counts is read by counting: MySQL's comes from `information_schema` (an
 * estimate, on InnoDB), PostgreSQL's from the planner's own estimate, and MongoDB's from the
 * collection's metadata — so this costs the server nothing to answer.
 */
function DatabaseStats({
  kind,
  connectionId,
  database,
  active,
  onError,
  statsCache: cache,
  schemaToken,
}: Props) {
  const { t } = useTranslation();
  const optionalSql = useOptionalSql();
  const [loading, setLoading] = useState(false);
  /** Bumped whenever the cache is written to, since a Map is the same object before and after and
   *  nothing would re-render off it on its own. */
  const [, setCacheToken] = useState(0);
  /** What the grid is narrowed to, as typed. Not filed with the figures: the sort is the user's
   *  standing choice about a database, while a search is about the question being asked right now. */
  const [filter, setFilter] = useState("");

  // Emptied whenever another database is put on screen, so its tables are not hidden by a search
  // typed against the one before — which would read as a database with almost nothing in it.
  useEffect(() => {
    setFilter("");
  }, [database]);

  /** What is filed for the database now selected. Read plainly rather than memoised: it is one map
   *  lookup, and a memo would only add a list of dependencies to get wrong. */
  const remembered = cache.get(database);
  /** The figures on screen, or null when there are none to show — which a read still owed, a
   *  reload, and figures read before the app last changed this database all count as. */
  const stats = remembered?.schemaToken === schemaToken ? remembered.rows : null;
  /** The order this database was last put in, which is part of what coming back to it means. Kept
   *  across a change of shape as well as across a reload: it is the user's choice, not something
   *  the server said. */
  const sort = remembered?.sort ?? DEFAULT_SORT;

  /** Files what is known about the selected database, letting the database read longest ago go once
   *  the cache is full — the only way anything is written here, so nothing gets past that ceiling.
   *  The render that would draw it is asked for at the same time, for the reason above. */
  const fileStats = useCallback(
    (entry: RememberedStats) => {
      fileInto(cache, database, entry, STATS_CACHE_LIMIT);
      setCacheToken((n) => n + 1);
    },
    [cache, database],
  );

  useEffect(() => {
    // Nothing to do until the tab is looked at, and nothing to do once it has been read: this is
    // what makes moving between the tabs — and coming back to a database — free.
    if (!active || stats !== null) return;
    let cancelled = false;
    setLoading(true);
    // `kind` is what decides, and a SQL workspace is what puts `optionalSql` above this component —
    // so on that branch it is there. Missing, this throws rather than quietly reading the figures
    // the other way round: a Mongo call down a SQL connection would come back as a puzzling error
    // from the server instead of naming what is actually wrong. See `useOptionalSql`.
    let read: Promise<TableStats[]>;
    if (kind === "mongo") {
      read = mongoCollectionStats(connectionId, database);
    } else if (optionalSql === null) {
      throw new Error(`DatabaseStats: no SQL connection for "${kind}"`);
    } else {
      read = optionalSql.api.tableStats(connectionId, database);
    }
    read
      .then((result) => {
        if (cancelled) return;
        // The order is kept across the read: a reload asked for while sorted by name comes back
        // sorted by name, rather than throwing the user back to the heaviest table first.
        fileStats({
          rows: result,
          sort: cache.get(database)?.sort ?? DEFAULT_SORT,
          schemaToken,
        });
      })
      .catch((e) => {
        if (cancelled) return;
        // Nothing is cached for a read that failed, so coming back to the tab tries again rather
        // than settling on an empty grid.
        onError(errorMessage(t, e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
    // `schemaToken` is in here so that a change made while a read is still out drops that read
    // rather than letting it land: figures filed under the shape before are turned down when read
    // back, and with nothing else moving there would be nothing left to ask for them again.
  }, [kind, optionalSql, connectionId, database, active, stats, schemaToken, onError, cache, fileStats, t]);

  /** Drops this database's figures, which is what makes the effect above read them again. The order
   *  they were in stays: it is the user's choice, not something the server said. */
  function reload() {
    fileStats({ rows: null, sort, schemaToken });
  }

  /** Puts the grid in a new order, and keeps it there for the next visit to this database. */
  function setSort(next: Sort) {
    fileStats({ rows: stats, sort: next, schemaToken });
  }

  // Dropping the figures is the reload, the same as the button's. Gated on the same state it is: a
  // re-read asked for while one is already out is one the button would refuse.
  useReloadShortcut(active, () => {
    if (loading) return;
    reload();
  });

  const sorted = useMemo(() => {
    const rows = [...(stats ?? [])];
    rows.sort((a, b) => {
      const left = a[sort.column];
      const right = b[sort.column];
      const order =
        typeof left === "string" && typeof right === "string"
          ? left.localeCompare(right)
          : Number(left) - Number(right);
      return sort.desc ? -order : order;
    });
    return rows;
  }, [stats, sort]);

  /** The rows on screen, each with the place it holds in the order — the `#` column counts positions
   *  in the whole database, so the heaviest table is the first one whether or not it is what was
   *  searched for, and a match keeps the rank that says how it compares to everything else. */
  const needle = filter.trim().toLowerCase();
  const shown = useMemo(() => {
    const all = sorted.map((row, position) => ({ row, position }));
    return needle ? all.filter((e) => e.row.name.toLowerCase().includes(needle)) : all;
  }, [sorted, needle]);

  /** What the footer adds up — the rows on screen, which under a search is what was searched for:
   *  the weight of one family of tables is most of the reason to narrow the grid at all. The average
   *  is their own, total bytes over total records, not the mean of the per-table averages, which
   *  would weigh a tiny table like a huge one. */
  const totals = useMemo(() => {
    const sum = shown.reduce(
      (acc, { row }) => ({
        rows: acc.rows + row.rows,
        dataSize: acc.dataSize + row.dataSize,
        indexSize: acc.indexSize + row.indexSize,
      }),
      { rows: 0, dataSize: 0, indexSize: 0 },
    );
    return { ...sum, avgRecordSize: sum.rows > 0 ? sum.dataSize / sum.rows : 0 };
  }, [shown]);

  const scrollRef = useRef<HTMLDivElement>(null);
  // Only the rows on screen are built into the DOM past a certain size. Sorting, the totals in the
  // footer and the reload all work off the whole set rather than off what is drawn, so none of them
  // notices.
  const virtual = shown.length >= VIRTUAL_FROM;
  const view = useVirtualRows(scrollRef, {
    total: shown.length,
    rowHeight: ROW_HEIGHT,
    enabled: virtual,
  });

  /** Narrows the grid, and puts it back to the top as it does: the matches are a new set of rows,
   *  and the offset the last set was read at says nothing about them. Written straight to the box
   *  rather than left to an effect, so the window of rows — which is worked out from the box — is
   *  already the top of the results in the frame the typing lands in. */
  function search(value: string) {
    const box = scrollRef.current;
    if (box) box.scrollTop = 0;
    setFilter(value);
  }

  /** The widest few values of each column, for the sizer row below. Three rather than one because
   *  length is only an approximation of width — all three go in, and the browser picks. */
  const widest = useMemo(
    () => widestValues(shown, 6, ({ row }, column) => cellText(row, column)),
    [shown]
  );

  /** Moves the clicked column to its next sort state: a new column opens on the order that reads
   *  it best — biggest first for a number, A to Z for a name — and clicking it again reverses. */
  function toggleSort(column: SortColumn) {
    setSort(
      sort.column === column
        ? { column, desc: !sort.desc }
        : { column, desc: column !== "name" },
    );
  }

  const nameLabel = t(kind !== "mongo" ? "dbStats.colTable" : "dbStats.colCollection");
  const countLabel = t(kind !== "mongo" ? "dbStats.colRows" : "dbStats.colDocuments");
  const averageLabel = t(kind !== "mongo" ? "dbStats.colAvgRow" : "dbStats.colAvgDocument");
  const searchLabel = t(kind !== "mongo" ? "dbStats.searchTables" : "dbStats.searchCollections");

  /** One sortable header. The chevron is always in the flow, empty when the column is not the one
   *  being sorted by, so the header does not change width as the sort moves between columns. */
  function header(column: SortColumn, label: string, numeric: boolean) {
    const active = sort.column === column;
    return (
      <th
        className={numeric ? `${styles.headerCell} ${styles.numeric}` : styles.headerCell}
        aria-sort={active ? (sort.desc ? "descending" : "ascending") : "none"}
        title={t(
          active ? (sort.desc ? "dbStats.sortDesc" : "dbStats.sortAsc") : "dbStats.sortNone",
          { column: label },
        )}
        onClick={() => toggleSort(column)}
      >
        {label}
        <span className={styles.sortIcon}>
          {active && (sort.desc ? <ChevronDownIcon /> : <ChevronUpIcon />)}
        </span>
      </th>
    );
  }

  return (
    <div className={styles.stats}>
      <header className={styles.panelHeader}>
        <h4 className={styles.panelTitle}>
          {t(kind !== "mongo" ? "dbStats.tablesTitle" : "dbStats.collectionsTitle", { database })}
        </h4>
        {/* How much of the database is being looked at, but only while that is not all of it —
            numbers alone, since there is nothing to say about them in any language. */}
        {needle !== "" && (
          <span className={styles.matchCount}>
            {shown.length}/{sorted.length}
          </span>
        )}
        <Input
          size="small"
          className={styles.filter}
          placeholder={searchLabel}
          aria-label={searchLabel}
          value={filter}
          onChange={(e) => search(e.target.value)}
          // Escape empties the box rather than leaving the pane, which is where every other search
          // in the app puts it. Held here while there is something to clear, so it is not taken
          // from whatever else in the workspace answers to it.
          onKeyDown={(e) => {
            if (e.key !== "Escape" || filter === "") return;
            e.preventDefault();
            e.stopPropagation();
            search("");
          }}
        />
        <ActionBar
          actions={[
            {
              key: "reload",
              icon: ReloadIcon,
              label: withReloadShortcut(t("dbStats.reload")),
              disabled: loading,
              busy: loading,
              // Dropping the figures is the reload: the effect reads again the moment there are
              // none held for the database on show.
              onClick: reload,
            },
          ]}
        />
      </header>

      <div
        className={styles.gridWrap}
        ref={scrollRef}
        onScroll={virtual ? view.onScroll : undefined}
      >
        <table
          className={virtual ? `${styles.grid} ${styles.gridRows}` : styles.grid}
          style={virtual ? gridStyle(ROW_HEIGHT, null) : undefined}
        >
          <thead>
            <tr>
              <th className={styles.rowNumber}>#</th>
              {header("name", nameLabel, false)}
              {header("rows", countLabel, true)}
              {header("dataSize", t("dbStats.colDataSize"), true)}
              {header("indexSize", t("dbStats.colIndexSize"), true)}
              {header("avgRecordSize", averageLabel, true)}
            </tr>
          </thead>
          <tbody>
            {/* What gives every column its width while only a few dozen rows are in the table: a
                row of no height holding the widest value of each column, which the browser's own
                layout finds exactly as it would find a real row. Without it the columns would be
                sized by whichever rows happen to be on screen and would shift on every scroll. */}
            {virtual && (
              <tr className={styles.sizerRow} aria-hidden="true">
                <td className={styles.rowNumber}>
                  <div className={styles.sizer}>{sorted.length}</div>
                </td>
                <td>
                  <div className={styles.sizer}>
                    {widest[1].map((value) => (
                      <div key={value} className={styles.name}>
                        {value}
                      </div>
                    ))}
                  </div>
                </td>
                {[2, 3, 4, 5].map((column) => (
                  <td key={column} className={styles.numeric}>
                    <div className={styles.sizer}>
                      {widest[column].map((value) => (
                        <div key={value}>{value}</div>
                      ))}
                    </div>
                  </td>
                ))}
              </tr>
            )}
            {virtual && (
              <tr className={styles.spacer} style={{ height: view.padTop }} aria-hidden="true">
                <td colSpan={6} />
              </tr>
            )}
            {shown.slice(view.first, view.last).map(({ row, position }) => {
              return (
                <tr key={row.name}>
                  {/* Where the table stands in the order, which is not where it stands in what a
                      search left — the rank is what says how it compares to the rest. */}
                  <td className={styles.rowNumber}>{position + 1}</td>
                  <td>
                    <span className={styles.name}>{row.name}</span>
                  </td>
                  <td className={styles.numeric}>{formatCount(row.rows)}</td>
                  {/* The exact byte count is in the tooltip: what the cell shows is rounded to a
                      unit, and two tables can read the same while differing by megabytes. */}
                  <td className={styles.numeric} title={t("dbStats.bytes", { bytes: formatCount(row.dataSize) })}>
                    {formatSize(row.dataSize)}
                  </td>
                  <td className={styles.numeric} title={t("dbStats.bytes", { bytes: formatCount(row.indexSize) })}>
                    {formatSize(row.indexSize)}
                  </td>
                  <td className={styles.numeric} title={t("dbStats.bytes", { bytes: formatCount(row.avgRecordSize) })}>
                    {formatSize(row.avgRecordSize)}
                  </td>
                </tr>
              );
            })}
            {virtual && (
              <tr className={styles.spacer} style={{ height: view.padBottom }} aria-hidden="true">
                <td colSpan={6} />
              </tr>
            )}
          </tbody>
          {shown.length > 0 && (
            <tfoot>
              <tr>
                <td className={styles.rowNumber} />
                {/* Says so when it is adding up a search rather than the database: a figure this
                    much smaller than the one that was there a keystroke ago has to name itself. */}
                <td className={styles.name}>
                  {t(needle === "" ? "dbStats.total" : "dbStats.totalShown")}
                </td>
                <td className={styles.numeric}>{formatCount(totals.rows)}</td>
                <td className={styles.numeric}>{formatSize(totals.dataSize)}</td>
                <td className={styles.numeric}>{formatSize(totals.indexSize)}</td>
                <td className={styles.numeric}>{formatSize(totals.avgRecordSize)}</td>
              </tr>
            </tfoot>
          )}
        </table>
        {/* Only once something has actually been read: `stats` is null while the first read is
            still out, and after one that failed — neither is an empty database. */}
        {!loading && stats !== null && shown.length === 0 && (
          <p className="muted">
            {stats.length === 0
              ? t(kind !== "mongo" ? "dbStats.noTables" : "dbStats.noCollections")
              : t(kind !== "mongo" ? "dbStats.noTablesMatch" : "dbStats.noCollectionsMatch")}
          </p>
        )}
      </div>

      {/* MySQL's row counts are sampled rather than counted, and saying so once under the grid is
          better than a tooltip on every cell that carries one. */}
      {kind !== "mongo" && shown.length > 0 && (
        <p className={styles.note}>{t("dbStats.estimateNote")}</p>
      )}

      {loading && <LoadingOverlay label={t("dbStats.loading")} />}
    </div>
  );
}

export default DatabaseStats;
