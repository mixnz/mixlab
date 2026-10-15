import { useMemo, useRef } from "react";
import ActionBar from "../../../../components/ActionBar";
import { gridStyle, useVirtualRows, widestValues } from "../../../../core/virtualRows";
import { useTranslation } from "../../../../i18n";
import { PencilIcon, PlusIcon, TrashIcon } from "../../../../icons";
import type { SqlDialect, SqlEditing } from "../../sql/dialect";
import type { SqlTableIndex } from "../../types";
import { indexKind } from "../IndexDialog";
import {
  CLICKHOUSE_REBUILD_ENGINES,
  INDEX_KIND_LABEL,
  ROW_HEIGHT,
  VIRTUAL_FROM,
  indexMethod,
  isFunctional,
} from "./shape";
import styles from "./TableStructure.module.css";

interface Props {
  indexes: SqlTableIndex[];
  /** How many columns the table has: an index needs at least one to be made over. */
  columnCount: number;
  loading: boolean;
  noWrites: boolean;
  noWritesHint: string | undefined;
  offers: SqlEditing;
  dialectKind: SqlDialect["kind"];
  /** ClickHouse's table engine, which decides whether the sorting key can be rebuilt. */
  engine: string | null;
  onAdd: () => void;
  onEdit: (index: SqlTableIndex) => void;
  /** A ClickHouse sorting key is not edited as an index: it is rebuilt with a new ORDER BY. */
  onEditSortingKey: () => void;
  onDrop: (name: string) => void;
}

/** The indexes grid: a row per index with its kind, method and columns, and its edit and drop
 *  buttons — windowed past the same threshold as the columns grid. */
export default function IndexesPanel({
  indexes,
  columnCount,
  loading,
  noWrites,
  noWritesHint,
  offers,
  dialectKind,
  engine,
  onAdd,
  onEdit,
  onEditSortingKey,
  onDrop,
}: Props) {
  const { t } = useTranslation();
  const indexScroll = useRef<HTMLDivElement>(null);
  const indexesVirtual = indexes.length >= VIRTUAL_FROM;
  const indexView = useVirtualRows(indexScroll, {
    total: indexes.length,
    rowHeight: ROW_HEIGHT,
    enabled: indexesVirtual,
  });

  /* The sizer row's values, worked out as the columns grid works out its own. */
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

  return (
    <section className={styles.panel}>
      <header className={styles.panelHeader}>
        <h4 className={styles.panelTitle}>{t("structure.indexesTitle")}</h4>
        <ActionBar
          actions={[
            {
              key: "add",
              icon: PlusIcon,
              label: t("structure.addIndex"),
              disabled: noWrites || columnCount === 0 || offers.indexKinds.length === 0,
              disabledHint: noWritesHint,
              onClick: () => onAdd(),
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
            {/* The same sizer as the columns grid's: the widest cell of each column, drawn the way a
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
                dialectKind === "clickhouse" && index.indexType === "sorting_key";
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
                          isSortingKey ? onEditSortingKey() : onEdit(index)
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
                        onClick={() => onDrop(index.name)}
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
  );
}
