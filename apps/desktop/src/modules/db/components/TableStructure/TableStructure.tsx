import { useEffect, useMemo, useRef, useState } from "react";
import { fileInto } from "../../../../core/paneCache";
import { gridStyle, useVirtualRows, widestValues } from "../../../../core/virtualRows";
import ActionBar from "../../../../components/ActionBar";
import ColumnDialog from "../ColumnDialog";
import ConfirmDialog from "../../../../components/ConfirmDialog";
import IndexDialog, { indexKind } from "../IndexDialog";
import SkipIndexDialog from "../SkipIndexDialog";
import OrderByDialog from "../OrderByDialog";
import Input from "../../../../components/Input";
import LoadingOverlay from "../../../../components/LoadingOverlay";
import { PencilIcon, PlusIcon, ReloadIcon, TrashIcon } from "../../../../icons";
import { useTranslation } from "../../../../i18n";
import { errorMessage } from "../../../../core/errors";
import { useReloadShortcut, withReloadShortcut } from "../../../../core/reload";
import { useSqlApi, useSqlDialect } from "../../sql/context";
import type { SqlEditing } from "../../sql/dialect";
import type {
  SqlCollation,
  SqlColumnSpec,
  SqlIndexSpec,
  SqlSkipIndex,
  SqlSkipIndexSpec,
  SqlStructureColumn,
  SqlTableIndex,
  SqlTableStructure,
} from "../../types";
import styles from "./TableStructure.module.css";
import { cacheKey } from "../../schemaTokens";

/** What the Kind column says an index is — the same reading of it the edit dialog opens on. */
const INDEX_KIND_LABEL = {
  primary: "structure.kindPrimary",
  unique: "structure.kindUnique",
  fulltext: "structure.kindFulltext",
  spatial: "structure.kindSpatial",
  index: "structure.kindIndex",
} as const;

/** How the index is stored, when that is a choice the index had. Read against the methods this
 * engine offers rather than a fixed pair, so PostgreSQL's `GIN`/`GiST`/`BRIN`/`SP-GiST` are named
 * here as well — the edit dialog fills its picker from the same list. `FULLTEXT`/`SPATIAL` name a
 * kind rather than a structure, and the Kind column already says so. */
function indexMethod(index: SqlTableIndex, methods: readonly string[]): string {
  const type = index.indexType.toUpperCase();
  return methods.includes(type) ? type : "";
}

/** What marks a default the server evaluates rather than stores. `f(x)` rather than a pair of
 * letters like the badges beside a name: those abbreviate a phrase, and this says what the value
 * beside it is. */
const EXPRESSION_BADGE = "f(x)";

/** Whether a column's default is one the server works out for each new row rather than a value it
 * keeps — and whether that is a distinction this engine draws at all. */
function isExpressionDefault(column: SqlStructureColumn, offers: SqlEditing): boolean {
  return offers.markExpressionDefaults && column.defaultIsExpression && column.defaultValue !== null;
}

/** An index over an expression rather than over columns. Its expression is not read here, so such
 * an index cannot be rebuilt from what the grid knows — only dropped. */
function isFunctional(index: SqlTableIndex): boolean {
  return index.columns.some((column) => column.name === null);
}

/** Where holding every row in the DOM stops being the cheap thing to do. Most tables have fewer
 *  columns than this and nothing here applies to them; a wide table has hundreds, and then the
 *  whole grid is laid out again every time the tab is opened — this panel is unmounted when the
 *  header leaves it, so that is every visit rather than only the first. */
const VIRTUAL_FROM = 60;

/** How tall a row of these grids is — stated, never measured; see `virtualRows.ts` for why that
 *  distinction is what makes a window of rows sound. 29px is what a row here comes to: a 20px
 *  line, 4px of padding above and below, and the 1px rule. Was 33px on a 24px line. */
const ROW_HEIGHT = 29;

/** The engines a sorting-key rebuild is allowed to run on — the same four
 *  `clickhouse_ddl::ENGINES` already whitelists for creating a new table (D2 of the main ClickHouse
 *  DDL design doc). `EXCHANGE TABLES`/`CREATE`/`DROP` with no `ON CLUSTER` only ever reach one
 *  replica of a `Replicated*` engine, which this has never been verified against — see the index DDL
 *  design doc's D11. */
const CLICKHOUSE_REBUILD_ENGINES = [
  "MergeTree",
  "ReplacingMergeTree",
  "SummingMergeTree",
  "AggregatingMergeTree",
];

/** Which dialog is open and on what: an entry with nothing in it is the "add" form. */
type ColumnDialogState = { column?: SqlStructureColumn };
type IndexDialogState = { index?: SqlTableIndex };
type SkipIndexDialogState = { index?: SqlSkipIndex };
/** What the confirmation is about to drop. */
type PendingDrop =
  | { kind: "column"; name: string }
  | { kind: "index"; name: string }
  | { kind: "skipIndex"; name: string };

/** One table's shape as it was last read, and which shape of the database it was read from — see
 * {@link Props.schemaToken}. */
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

interface Props {
  /** Whether this is what the user is actually looking at — the Structure tab, in the connection
   *  tab the tab bar is showing. This stays mounted behind both, so it is what says when the shape
   *  is worth reading, when a read the user cannot see would be wasted, and which of the panes
   *  mounted at once `Ctrl+R` belongs to. */
  active: boolean;
  connectionId: string;
  selectedDb: string;
  selectedTable: string;
  onError: (message: string) => void;
  /** Where the shape read for each table is kept between visits — see {@link StructureCache}. */
  structureCache: StructureCache;
  /** Which shape of this database the cache is allowed to speak for. Moved by the workspace
   *  whenever the app changes that shape — a table created, renamed or dropped, a column altered, a
   *  dump restored — so that columns read from the shape before are read again rather than shown.
   *  A name is not a promise: a table dropped and made again under the same name would otherwise
   *  open on the columns of the one it replaced. */
  schemaToken: number;
  /** Told that an `ALTER TABLE` has just landed. The panel's own cache is not the only thing now
   *  out of date — the Data tab's rows are about columns that have moved, and the Query tab
   *  completes from a copy of the shape — so this is the workspace's to answer, not this panel's. */
  onSchemaChanged: () => void;
  /** The saved connection is marked as one nothing is written to. The columns and indexes are
   *  still read and shown; every `ALTER TABLE` this panel can send is what goes. */
  readOnly?: boolean;
}

/**
 * The table's shape: its columns above, its indexes below, each with its own add/edit/drop.
 *
 * Every change is one `ALTER TABLE` sent by itself and followed by a reload, rather than a batch
 * of pending edits applied together — a rejected ALTER then names the one thing that was wrong
 * with it, and what the grid shows afterwards is what the server actually has.
 *
 * Read once and kept: the workspace leaves this mounted while another tab is up, so moving between
 * the tabs shows the shape already read rather than asking for it again. It is only re-read when
 * the table changes under it, when an `ALTER` this panel sent means the server no longer matches,
 * or when the reload asks; a table selected while the tab was hidden waits until it is looked at
 * again before costing anything.
 */
function TableStructure({
  active,
  connectionId,
  selectedDb,
  selectedTable,
  onError,
  structureCache,
  schemaToken,
  onSchemaChanged,
  readOnly = false,
}: Props) {
  const { t } = useTranslation();
  const api = useSqlApi();
  const dialect = useSqlDialect();
  const offers = dialect.editing;
  /** Bumped whenever the cache is written to or dropped, since a Map is the same object either way
   *  and nothing would re-render off it on its own. Only the setter is ever read: the count says
   *  nothing, it only says that the cache is worth looking at again. */
  const [, setCacheToken] = useState(0);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [columnDialog, setColumnDialog] = useState<ColumnDialogState | null>(null);
  const [indexDialog, setIndexDialog] = useState<IndexDialogState | null>(null);
  const [skipIndexDialog, setSkipIndexDialog] = useState<SkipIndexDialogState | null>(null);
  const [orderByDialogOpen, setOrderByDialogOpen] = useState(false);
  const [orderByRowCount, setOrderByRowCount] = useState<number | null>(null);
  const [pendingDrop, setPendingDrop] = useState<PendingDrop | null>(null);
  const [collations, setCollations] = useState<SqlCollation[]>([]);
  /** What the columns grid is narrowed to, as typed. A wide table is the one nobody can read down,
   *  and it is also the one where the column being looked for has a name already in mind. */
  const [columnFilter, setColumnFilter] = useState("");

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

  // Memoized because the grids below are, on these: a fresh `[]` each render while nothing is
  // loaded would rebuild them on every render for no change at all.
  const columns = useMemo(() => structure?.columns ?? [], [structure]);
  const indexes = useMemo(() => structure?.indexes ?? [], [structure]);
  const skipIndexes = structure?.skipIndexes ?? [];
  const engine = structure?.engine ?? null;
  const busy = loading || saving;

  // Read only while the dialog asking about it is open — the same reason the collations list above
  // is read once and not on every render, but this one is per-table rather than per-connection, so
  // it is keyed on the dialog being open rather than on mount.
  useEffect(() => {
    if (!orderByDialogOpen) return;
    let cancelled = false;
    api
      .rowCount(connectionId, selectedDb, selectedTable)
      .then((n) => {
        if (!cancelled) setOrderByRowCount(n);
      })
      .catch(() => {
        if (!cancelled) setOrderByRowCount(null);
      });
    return () => {
      cancelled = true;
    };
  }, [orderByDialogOpen, api, connectionId, selectedDb, selectedTable]);

  // Emptied whenever another table is put on screen: a filter left over from the last one would
  // hide most of this table's columns while reading as if the table simply had that few.
  useEffect(() => {
    setColumnFilter("");
  }, [tableKey]);

  /** The columns on screen, each with the place it holds in the table — the `#` column counts
   *  positions in the table rather than in what the filter left, so a column keeps the number it
   *  had whether or not anything is being searched for. */
  const needle = columnFilter.trim().toLowerCase();
  const shownColumns = useMemo(() => {
    const all = columns.map((column, position) => ({ column, position }));
    return needle ? all.filter((e) => e.column.name.toLowerCase().includes(needle)) : all;
  }, [columns, needle]);

  // Each grid holds only the rows on screen once it is worth it. Everything either grid does — the
  // edit and drop buttons, the reload, the dialogs — works off `columns` and `indexes` rather than
  // off what is drawn, so none of it notices.
  const columnScroll = useRef<HTMLDivElement>(null);
  const indexScroll = useRef<HTMLDivElement>(null);
  const columnsVirtual = shownColumns.length >= VIRTUAL_FROM;
  const indexesVirtual = indexes.length >= VIRTUAL_FROM;
  const columnView = useVirtualRows(columnScroll, {
    total: shownColumns.length,
    rowHeight: ROW_HEIGHT,
    enabled: columnsVirtual,
  });

  /** Narrows the columns grid, and puts it back to the top as it does: the matches are a new set of
   *  rows, and the offset the last set was read at says nothing about them. Written straight to the
   *  box rather than left to an effect, so the window of rows — which is worked out from the box —
   *  is already the top of the results in the frame the typing lands in. */
  function filterColumns(value: string) {
    const box = columnScroll.current;
    if (box) box.scrollTop = 0;
    setColumnFilter(value);
  }
  const indexView = useVirtualRows(indexScroll, {
    total: indexes.length,
    rowHeight: ROW_HEIGHT,
    enabled: indexesVirtual,
  });

  /** The widest few values of each column of each grid, for the sizer rows below. Three rather than
   *  one because length is only an approximation of width — all three go in, and the browser picks
   *  between them the same way it picks between real rows. */
  const widestColumn = useMemo(
    () =>
      widestValues(shownColumns, 8, ({ column }, c) =>
        [
          "",
          column.name,
          column.dataType,
          t(column.nullable ? "structure.yes" : "structure.no"),
          column.defaultValue ?? t("structure.none"),
          column.extra || t("structure.none"),
          column.collation ?? t("structure.none"),
          column.comment,
        ][c]
      ),
    [shownColumns, t]
  );
  /** Whether the Default column carries a badge at all: the width it is given has to leave room for
   *  one, and a table whose defaults are all values it stores should not be given the room. */
  const defaultBadge = useMemo(
    () => shownColumns.some(({ column }) => isExpressionDefault(column, offers)),
    [shownColumns, offers]
  );

  /** Whether any column carries badges beside its name, and how many at once — the widest name has
   *  to leave room for them, and a table with none of them should not be given the room. */
  const columnBadges = useMemo(
    () =>
      Math.max(
        0,
        ...shownColumns.map(
          ({ column }) => (column.key === "" ? 0 : 1) + (column.autoIncrement ? 1 : 0)
        )
      ),
    [shownColumns]
  );
  const widestIndex = useMemo(
    () =>
      widestValues(indexes, 5, (index, c) =>
        [
          index.name,
          t(INDEX_KIND_LABEL[indexKind(index)]),
          indexMethod(index, offers.indexMethods) || t("structure.none"),
          index.columns
            .map((column) =>
              column.prefixLength === null
                ? (column.name ?? t("structure.indexExpression"))
                : `${column.name ?? t("structure.indexExpression")}(${column.prefixLength})`
            )
            .join(", "),
          index.comment,
        ][c]
      ),
    [indexes, offers.indexMethods, t]
  );
  /** What every button that would send an `ALTER TABLE` is gated on. The reload beside them is
   *  not: reading is the one thing a read-only connection is for. */
  const noWrites = busy || readOnly;
  /** Why they are greyed out, when it is not simply that the panel is mid-request. */
  const noWritesHint = readOnly ? t("common.readOnlyConnection") : undefined;

  /** Dropping this table's entry is the reload: the effect above reads again the moment it finds
   *  nothing for the table on screen. Only this table's — the others were read from the server just
   *  as truthfully, and dropping them would turn one reload into a re-read of every table visited
   *  so far. */
  function reload() {
    structureCache.delete(tableKey);
    setCacheToken((n) => n + 1);
  }

  // Gated on the same state the button below is: a re-read asked for while one is already out, or
  // over an `ALTER` still running, is one the button would refuse. Not from behind the column or
  // index form either — that is a half-filled definition the keyboard belongs to — nor from behind
  // the drop confirmation, which is a question about the very thing a reload would replace.
  useReloadShortcut(
    active && columnDialog === null && indexDialog === null && pendingDrop === null,
    () => {
      if (busy) return;
      reload();
    }
  );

  /** Runs one change and reloads on success. Errors are left to reject so the dialog that asked
   * for the change can show them and stay open; a drop has no dialog, so it reports its own. */
  async function apply(change: () => Promise<void>) {
    setSaving(true);
    try {
      await change();
      // Not this panel's own reload: an `ALTER TABLE` is a change to the database, and the rows
      // the Data tab is holding, the figures the Statistics tab is holding and the shape the Query
      // tab completes from are all now about a table that no longer looks like that. The workspace
      // is the one place that knows about all of them, and dropping this panel's entry is part of
      // what it does — which is what brings the columns back through the effect above.
      onSchemaChanged();
    } finally {
      setSaving(false);
    }
  }

  async function submitColumn(spec: SqlColumnSpec) {
    const original = columnDialog?.column;
    await apply(() =>
      original
        ? api.modifyColumn(connectionId, selectedDb, selectedTable, original.name, spec)
        : api.addColumn(connectionId, selectedDb, selectedTable, spec),
    );
    setColumnDialog(null);
  }

  async function submitIndex(spec: SqlIndexSpec) {
    const original = indexDialog?.index;
    await apply(() =>
      original
        ? api.modifyIndex(connectionId, selectedDb, selectedTable, original.name, spec)
        : api.addIndex(connectionId, selectedDb, selectedTable, spec),
    );
    setIndexDialog(null);
  }

  async function submitSkipIndex(spec: SqlSkipIndexSpec) {
    const original = skipIndexDialog?.index;
    await apply(() =>
      original
        ? api.modifySkipIndex(connectionId, selectedDb, selectedTable, original.name, spec)
        : api.addSkipIndex(connectionId, selectedDb, selectedTable, spec),
    );
    setSkipIndexDialog(null);
  }

  // Deliberately not routed through `apply`: a successful rebuild that left a temp table behind is
  // not a failure (see `rebuildOrderBy`'s own doc), so this needs to tell that case apart from a
  // real rejection rather than let `apply` treat both the same way.
  async function submitOrderBy(columnsChosen: string[]) {
    setSaving(true);
    try {
      const leftover = await api.rebuildOrderBy(
        connectionId,
        selectedDb,
        selectedTable,
        columnsChosen,
      );
      onSchemaChanged();
      setOrderByDialogOpen(false);
      if (leftover !== null) {
        onError(t("orderByDialog.leftoverWarning", { table: leftover }));
      }
    } finally {
      setSaving(false);
    }
  }

  async function confirmDrop() {
    const target = pendingDrop;
    setPendingDrop(null);
    if (!target) return;
    try {
      await apply(() => {
        if (target.kind === "column") {
          return api.dropColumn(connectionId, selectedDb, selectedTable, target.name);
        }
        if (target.kind === "skipIndex") {
          return api.dropSkipIndex(connectionId, selectedDb, selectedTable, target.name);
        }
        return api.dropIndex(connectionId, selectedDb, selectedTable, target.name);
      });
    } catch (e) {
      onError(errorMessage(t, e));
    }
  }

  return (
    <div className={styles.structure}>
      <section className={styles.panel}>
        <header className={styles.panelHeader}>
          <h4 className={styles.panelTitle}>{t("structure.columnsTitle")}</h4>
          {/* How much of the table is being looked at, but only while that is not all of it —
              numbers alone, since there is nothing to say about them in any language. */}
          {needle !== "" && (
            <span className={styles.matchCount}>
              {shownColumns.length}/{columns.length}
            </span>
          )}
          <Input
            size="small"
            className={styles.filter}
            placeholder={t("structure.filterColumns")}
            aria-label={t("structure.filterColumns")}
            value={columnFilter}
            onChange={(e) => filterColumns(e.target.value)}
            // Escape empties the box rather than leaving the pane, which is where every other
            // search in the app puts it. Held here while there is something to clear, so it is not
            // taken from whatever else in the workspace answers to it.
            onKeyDown={(e) => {
              if (e.key !== "Escape" || columnFilter === "") return;
              e.preventDefault();
              e.stopPropagation();
              filterColumns("");
            }}
          />
          <ActionBar
            actions={[
              {
                key: "reload",
                icon: ReloadIcon,
                label: withReloadShortcut(t("structure.reload")),
                disabled: busy,
                busy: loading,
                onClick: reload,
              },
              {
                key: "add",
                icon: PlusIcon,
                label: t("structure.addColumn"),
                disabled: noWrites,
                disabledHint: noWritesHint,
                onClick: () => setColumnDialog({}),
              },
            ]}
          />
        </header>
        <div
          className={styles.gridWrap}
          ref={columnScroll}
          onScroll={columnsVirtual ? columnView.onScroll : undefined}
        >
          <table
            className={columnsVirtual ? `${styles.grid} ${styles.gridRows}` : styles.grid}
            style={columnsVirtual ? gridStyle(ROW_HEIGHT, null) : undefined}
          >
            <thead>
              <tr>
                <th className={styles.rowNumber}>#</th>
                <th>{t("structure.colName")}</th>
                <th>{t("structure.colType")}</th>
                <th>{t("structure.colNullable")}</th>
                <th>{t("structure.colDefault")}</th>
                <th>{t("structure.colExtra")}</th>
                <th>{t("structure.colCollation")}</th>
                <th>{t("structure.colComment")}</th>
                <th className={styles.rowActions} />
              </tr>
            </thead>
            <tbody>
              {/* What gives every column its width while only a few dozen rows are in the table: a
                  row of no height holding the widest cell of each, drawn the way a real row draws
                  it — badges beside the name, a pair of buttons in the actions column — so the
                  browser's own layout sizes the grid around something that cannot scroll away. */}
              {columnsVirtual && (
                <tr className={styles.sizerRow} aria-hidden="true">
                  <td className={styles.rowNumber}>
                    <div className={styles.sizer}>{columns.length}</div>
                  </td>
                  <td>
                    <div className={styles.sizer}>
                      {widestColumn[1].map((value) => (
                        <div key={value}>
                          <span className={styles.name}>{value}</span>
                          {Array.from({ length: columnBadges }, (_, n) => (
                            <span key={n} className={styles.badge}>
                              PK
                            </span>
                          ))}
                        </div>
                      ))}
                    </div>
                  </td>
                  <td className={styles.mono}>
                    <div className={styles.sizer}>
                      {widestColumn[2].map((value) => (
                        <div key={value}>{value}</div>
                      ))}
                    </div>
                  </td>
                  {[3, 6].map((column) => (
                    <td key={column} className={styles.muted}>
                      <div className={styles.sizer}>
                        {widestColumn[column].map((value) => (
                          <div key={value}>{value}</div>
                        ))}
                      </div>
                    </td>
                  ))}
                  <td className={styles.mono}>
                    <div className={styles.sizer}>
                      {widestColumn[4].map((value) => (
                        <div key={value}>
                          {value}
                          {defaultBadge && (
                            <span className={`${styles.badge} ${styles.badgeExpression}`}>
                              {EXPRESSION_BADGE}
                            </span>
                          )}
                        </div>
                      ))}
                    </div>
                  </td>
                  <td className={styles.muted}>
                    <div className={styles.sizer}>
                      {widestColumn[5].map((value) => (
                        <div key={value}>{value}</div>
                      ))}
                    </div>
                  </td>
                  <td>
                    <div className={styles.sizer}>
                      {widestColumn[7].map((value) => (
                        <div key={value}>{value}</div>
                      ))}
                    </div>
                  </td>
                  <td className={styles.rowActions}>
                    <div className={styles.sizer}>
                      <div className={styles.rowButtons}>
                        <span className={styles.iconButton}>
                          <PencilIcon size={14} />
                        </span>
                        <span className={styles.iconButton}>
                          <TrashIcon size={14} />
                        </span>
                      </div>
                    </div>
                  </td>
                </tr>
              )}
              {columnsVirtual && (
                <tr
                  className={styles.spacer}
                  style={{ height: columnView.padTop }}
                  aria-hidden="true"
                >
                  <td colSpan={9} />
                </tr>
              )}
              {shownColumns.slice(columnView.first, columnView.last).map(({ column, position }) => {
                return (
                  <tr key={column.name}>
                    {/* Where the column sits in the table, which is not where it sits in a filtered
                        list — a search for one name should not renumber it. */}
                    <td className={styles.rowNumber}>{position + 1}</td>
                    <td>
                      <span className={styles.name}>{column.name}</span>
                      {column.key === "PRI" && (
                        <span
                          className={`${styles.badge} ${styles.badgeKey}`}
                          title={t("structure.primaryTooltip")}
                        >
                          PK
                        </span>
                      )}
                      {column.key === "UNI" && (
                        <span className={styles.badge} title={t("structure.uniqueTooltip")}>
                          UQ
                        </span>
                      )}
                      {column.key === "MUL" && (
                        <span className={styles.badge} title={t("structure.indexTooltip")}>
                          IX
                        </span>
                      )}
                      {column.autoIncrement && (
                        <span className={styles.badge} title={t("structure.autoIncrementTooltip")}>
                          AI
                        </span>
                      )}
                    </td>
                    <td className={styles.mono}>{column.dataType}</td>
                    <td className={column.nullable ? undefined : styles.muted}>
                      {t(column.nullable ? "structure.yes" : "structure.no")}
                    </td>
                    <td className={column.defaultValue === null ? styles.muted : styles.mono}>
                      {column.defaultValue ?? t("structure.none")}
                      {/* An expression is what the server runs for each new row, not text it
                          stores: `uuid()` here means a different value every time, and reads
                          exactly like the six characters `uuid()` would without this. */}
                      {isExpressionDefault(column, offers) && (
                        <span
                          className={`${styles.badge} ${styles.badgeExpression}`}
                          title={t("structure.expressionTooltip")}
                        >
                          {EXPRESSION_BADGE}
                        </span>
                      )}
                    </td>
                    <td className={styles.muted} title={column.extra}>
                      {column.extra || t("structure.none")}
                    </td>
                    <td className={styles.muted}>{column.collation ?? t("structure.none")}</td>
                    <td title={column.comment}>{column.comment}</td>
                    <td className={styles.rowActions}>
                      <div className={styles.rowButtons}>
                        <button
                          type="button"
                          className={styles.iconButton}
                          // A generated column's expression is not read here, so re-issuing its
                          // definition would drop the very thing that defines it.
                          disabled={noWrites || column.generated}
                          title={
                            column.generated
                              ? t("structure.generatedTooltip")
                              : (noWritesHint ?? t("structure.editColumn"))
                          }
                          aria-label={t("structure.editColumn")}
                          onClick={() => setColumnDialog({ column })}
                        >
                          <PencilIcon size={14} />
                        </button>
                        <button
                          type="button"
                          className={`${styles.iconButton} ${styles.danger}`}
                          disabled={noWrites}
                          title={noWritesHint ?? t("structure.dropColumn")}
                          aria-label={t("structure.dropColumn")}
                          onClick={() => setPendingDrop({ kind: "column", name: column.name })}
                        >
                          <TrashIcon size={14} />
                        </button>
                      </div>
                    </td>
                  </tr>
                );
              })}
              {columnsVirtual && (
                <tr
                  className={styles.spacer}
                  style={{ height: columnView.padBottom }}
                  aria-hidden="true"
                >
                  <td colSpan={9} />
                </tr>
              )}
            </tbody>
          </table>
          {!loading && shownColumns.length === 0 && (
            <p className="muted">
              {t(columns.length === 0 ? "structure.noColumns" : "structure.noColumnsMatch")}
            </p>
          )}
        </div>
      </section>

      <section className={styles.panel}>
        <header className={styles.panelHeader}>
          <h4 className={styles.panelTitle}>{t("structure.indexesTitle")}</h4>
          <ActionBar
            actions={[
              {
                key: "add",
                icon: PlusIcon,
                label: t("structure.addIndex"),
                disabled: noWrites || columns.length === 0 || offers.indexKinds.length === 0,
                disabledHint: noWritesHint,
                onClick: () => setIndexDialog({}),
              },
            ]}
          />
        </header>
        <div
          className={styles.gridWrap}
          ref={indexScroll}
          onScroll={indexesVirtual ? indexView.onScroll : undefined}
        >
          <table
            className={indexesVirtual ? `${styles.grid} ${styles.gridRows}` : styles.grid}
            style={indexesVirtual ? gridStyle(ROW_HEIGHT, null) : undefined}
          >
            <thead>
              <tr>
                <th>{t("structure.indexName")}</th>
                <th>{t("structure.indexKind")}</th>
                <th>{t("structure.indexMethod")}</th>
                <th>{t("structure.indexColumns")}</th>
                <th>{t("structure.indexComment")}</th>
                <th className={styles.rowActions} />
              </tr>
            </thead>
            <tbody>
              {/* The same sizer as the grid above: the widest cell of each column, drawn the way a
                  real row draws it — the Kind badge included, since a badge is most of that
                  column's width. */}
              {indexesVirtual && (
                <tr className={styles.sizerRow} aria-hidden="true">
                  <td>
                    <div className={styles.sizer}>
                      {widestIndex[0].map((value) => (
                        <div key={value}>
                          <span className={styles.name}>{value}</span>
                        </div>
                      ))}
                    </div>
                  </td>
                  <td>
                    <div className={styles.sizer}>
                      {widestIndex[1].map((value) => (
                        <div key={value}>
                          <span className={`${styles.badge} ${styles.kindBadge}`}>{value}</span>
                        </div>
                      ))}
                    </div>
                  </td>
                  <td className={styles.muted}>
                    <div className={styles.sizer}>
                      {widestIndex[2].map((value) => (
                        <div key={value}>{value}</div>
                      ))}
                    </div>
                  </td>
                  <td className={styles.mono}>
                    <div className={styles.sizer}>
                      {widestIndex[3].map((value) => (
                        <div key={value}>{value}</div>
                      ))}
                    </div>
                  </td>
                  <td>
                    <div className={styles.sizer}>
                      {widestIndex[4].map((value) => (
                        <div key={value}>{value}</div>
                      ))}
                    </div>
                  </td>
                  <td className={styles.rowActions}>
                    <div className={styles.sizer}>
                      <div className={styles.rowButtons}>
                        <span className={styles.iconButton}>
                          <PencilIcon size={14} />
                        </span>
                        <span className={styles.iconButton}>
                          <TrashIcon size={14} />
                        </span>
                      </div>
                    </div>
                  </td>
                </tr>
              )}
              {indexesVirtual && (
                <tr
                  className={styles.spacer}
                  style={{ height: indexView.padTop }}
                  aria-hidden="true"
                >
                  <td colSpan={6} />
                </tr>
              )}
              {indexes.slice(indexView.first, indexView.last).map((index) => {
                const functional = isFunctional(index);
                const isSortingKey =
                  dialect.kind === "clickhouse" && index.indexType === "sorting_key";
                const engineAllowed = CLICKHOUSE_REBUILD_ENGINES.includes(engine ?? "");
                return (
                  <tr key={index.name}>
                    <td>
                      <span className={styles.name}>{index.name}</span>
                    </td>
                    <td>
                      <span
                        className={`${styles.badge} ${styles.kindBadge}${
                          index.primary ? ` ${styles.badgeKey}` : ""
                        }`}
                      >
                        {t(INDEX_KIND_LABEL[indexKind(index)])}
                      </span>
                    </td>
                    <td className={styles.muted}>
                      {indexMethod(index, offers.indexMethods) || t("structure.none")}
                    </td>
                    <td className={styles.mono}>
                      {index.columns
                        .map((column) => {
                          const name = column.name ?? t("structure.indexExpression");
                          return column.prefixLength === null
                            ? name
                            : `${name}(${column.prefixLength})`;
                        })
                        .join(", ")}
                    </td>
                    <td title={index.comment}>{index.comment}</td>
                    <td className={styles.rowActions}>
                      <div className={styles.rowButtons}>
                        <button
                          type="button"
                          className={styles.iconButton}
                          disabled={
                            noWrites || functional || (isSortingKey && !engineAllowed)
                          }
                          title={
                            functional
                              ? t("structure.functionalIndexTooltip")
                              : isSortingKey && !engineAllowed
                                ? t("structure.rebuildEngineNotAllowed", { engine: engine ?? "" })
                                : (noWritesHint ?? t("structure.editIndex"))
                          }
                          aria-label={t("structure.editIndex")}
                          onClick={() =>
                            isSortingKey ? setOrderByDialogOpen(true) : setIndexDialog({ index })
                          }
                        >
                          <PencilIcon size={14} />
                        </button>
                        <button
                          type="button"
                          className={`${styles.iconButton} ${styles.danger}`}
                          disabled={noWrites || isSortingKey}
                          title={noWritesHint ?? t("structure.dropIndex")}
                          aria-label={t("structure.dropIndex")}
                          onClick={() => setPendingDrop({ kind: "index", name: index.name })}
                        >
                          <TrashIcon size={14} />
                        </button>
                      </div>
                    </td>
                  </tr>
                );
              })}
              {indexesVirtual && (
                <tr
                  className={styles.spacer}
                  style={{ height: indexView.padBottom }}
                  aria-hidden="true"
                >
                  <td colSpan={6} />
                </tr>
              )}
            </tbody>
          </table>
          {!loading && indexes.length === 0 && <p className="muted">{t("structure.noIndexes")}</p>}
        </div>
      </section>

      {dialect.kind === "clickhouse" && (
        <section className={styles.panel}>
          <header className={styles.panelHeader}>
            <h4 className={styles.panelTitle}>{t("structure.skipIndexesTitle")}</h4>
            <ActionBar
              actions={[
                {
                  key: "add",
                  icon: PlusIcon,
                  label: t("structure.addSkipIndex"),
                  disabled: noWrites,
                  disabledHint: noWritesHint,
                  onClick: () => setSkipIndexDialog({}),
                },
              ]}
            />
          </header>
          <div className={styles.gridWrap}>
            <table className={styles.grid}>
              <thead>
                <tr>
                  <th>{t("structure.skipIndexName")}</th>
                  <th>{t("structure.skipIndexExpr")}</th>
                  <th>{t("structure.skipIndexType")}</th>
                  <th>{t("structure.skipIndexGranularity")}</th>
                  <th className={styles.rowActions} />
                </tr>
              </thead>
              <tbody>
                {skipIndexes.map((index) => (
                  <tr key={index.name}>
                    <td>
                      <span className={styles.name}>{index.name}</span>
                    </td>
                    <td className={styles.mono}>{index.expr}</td>
                    <td className={styles.mono}>
                      {index.args.length === 0
                        ? index.indexType
                        : `${index.indexType}(${index.args.join(", ")})`}
                    </td>
                    <td className={styles.muted}>{index.granularity}</td>
                    <td className={styles.rowActions}>
                      <div className={styles.rowButtons}>
                        <button
                          type="button"
                          className={styles.iconButton}
                          disabled={noWrites}
                          title={noWritesHint ?? t("structure.editIndex")}
                          aria-label={t("structure.editIndex")}
                          onClick={() => setSkipIndexDialog({ index })}
                        >
                          <PencilIcon size={14} />
                        </button>
                        <button
                          type="button"
                          className={`${styles.iconButton} ${styles.danger}`}
                          disabled={noWrites}
                          title={noWritesHint ?? t("structure.dropIndex")}
                          aria-label={t("structure.dropIndex")}
                          onClick={() =>
                            setPendingDrop({ kind: "skipIndex", name: index.name })
                          }
                        >
                          <TrashIcon size={14} />
                        </button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            {!loading && skipIndexes.length === 0 && (
              <p className="muted">{t("structure.noSkipIndexes")}</p>
            )}
          </div>
        </section>
      )}

      {(loading || saving) && (
        <LoadingOverlay label={saving ? t("structure.saving") : t("structure.loading")} />
      )}

      {columnDialog !== null && (
        <ColumnDialog
          table={selectedTable}
          columns={columns}
          collations={collations}
          column={columnDialog.column}
          onCancel={() => setColumnDialog(null)}
          onSubmit={submitColumn}
        />
      )}

      {indexDialog !== null && (
        <IndexDialog
          table={selectedTable}
          columns={columns.map((c) => c.name)}
          index={indexDialog.index}
          onCancel={() => setIndexDialog(null)}
          onSubmit={submitIndex}
        />
      )}

      {skipIndexDialog !== null && (
        <SkipIndexDialog
          table={selectedTable}
          index={skipIndexDialog.index}
          onCancel={() => setSkipIndexDialog(null)}
          onSubmit={submitSkipIndex}
        />
      )}

      {orderByDialogOpen && (
        <OrderByDialog
          table={selectedTable}
          columns={columns.map((c) => c.name)}
          current={
            indexes
              .find((i) => i.indexType === "sorting_key")
              ?.columns.map((c) => c.name ?? "")
              .filter((n) => n !== "") ?? []
          }
          rowCount={orderByRowCount}
          onCancel={() => setOrderByDialogOpen(false)}
          onSubmit={submitOrderBy}
        />
      )}

      {pendingDrop !== null && (
        <ConfirmDialog
          title={t(
            pendingDrop.kind === "column"
              ? "structure.dropColumnTitle"
              : pendingDrop.kind === "skipIndex"
                ? "structure.dropSkipIndexTitle"
                : "structure.dropIndexTitle",
          )}
          message={
            pendingDrop.kind === "column"
              ? t("structure.dropColumnMessage", { column: pendingDrop.name })
              : pendingDrop.kind === "skipIndex"
                ? t("structure.dropSkipIndexMessage", { index: pendingDrop.name })
                : t("structure.dropIndexMessage", { index: pendingDrop.name })
          }
          confirmLabel={t("common.delete")}
          danger
          onConfirm={() => void confirmDrop()}
          onCancel={() => setPendingDrop(null)}
        />
      )}
    </div>
  );
}

export default TableStructure;
