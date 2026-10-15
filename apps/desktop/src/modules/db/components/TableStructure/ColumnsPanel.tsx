import { useEffect, useMemo, useRef, useState } from "react";
import ActionBar from "../../../../components/ActionBar";
import Input from "../../../../components/Input";
import { gridStyle, useVirtualRows, widestValues } from "../../../../core/virtualRows";
import { useTranslation } from "../../../../i18n";
import { PencilIcon, PlusIcon, ReloadIcon, TrashIcon } from "../../../../icons";
import { withReloadShortcut } from "../../../../core/reload";
import type { SqlEditing } from "../../sql/dialect";
import type { SqlStructureColumn } from "../../types";
import { EXPRESSION_BADGE, ROW_HEIGHT, VIRTUAL_FROM, isExpressionDefault } from "./shape";
import styles from "./TableStructure.module.css";

interface Props {
  columns: SqlStructureColumn[];
  /** The table on screen, as the cache keys it: a new one empties the filter. */
  tableKey: string;
  loading: boolean;
  busy: boolean;
  noWrites: boolean;
  noWritesHint: string | undefined;
  offers: SqlEditing;
  onReload: () => void;
  onAdd: () => void;
  onEdit: (column: SqlStructureColumn) => void;
  onDrop: (name: string) => void;
}

/** The columns grid: its filter, its window of rows once the table is wide, and a row per column
 *  with its badges and its edit and drop buttons. */
export default function ColumnsPanel({
  columns,
  tableKey,
  loading,
  busy,
  noWrites,
  noWritesHint,
  offers,
  onReload,
  onAdd,
  onEdit,
  onDrop,
}: Props) {
  const { t } = useTranslation();
  /** What the columns grid is narrowed to, as typed. A wide table is the one nobody can read down,
   *  and it is also the one where the column being looked for has a name already in mind. */
  const [columnFilter, setColumnFilter] = useState("");

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
  const columnsVirtual = shownColumns.length >= VIRTUAL_FROM;
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

  return (
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
              onClick: onReload,
            },
            {
              key: "add",
              icon: PlusIcon,
              label: t("structure.addColumn"),
              disabled: noWrites,
              disabledHint: noWritesHint,
              onClick: () => onAdd(),
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
                        onClick={() => onEdit(column)}
                      >
                        <PencilIcon size={14} />
                      </button>
                      <button
                        type="button"
                        className={`${styles.iconButton} ${styles.danger}`}
                        disabled={noWrites}
                        title={noWritesHint ?? t("structure.dropColumn")}
                        aria-label={t("structure.dropColumn")}
                        onClick={() => onDrop(column.name)}
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
  );
}
