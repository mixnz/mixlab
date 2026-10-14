import type { RefObject } from "react";
import Tooltip from "../../../../components/Tooltip";
import { ChevronDownIcon, ChevronUpIcon } from "../../../../icons";
import { useTranslation } from "../../../../i18n";
import { IS_MAC } from "../../../../core/platform";
import type { SqlColumnMeta } from "../../types";
import type { Sort } from "./request";
import styles from "./SqlTable.module.css";

interface Props {
  columns: string[];
  columnMeta: Record<string, SqlColumnMeta>;
  sort: Sort | null;
  /** Every header cell, by column — what the grid measures its columns off. */
  thRefs: RefObject<Map<string, HTMLTableCellElement>>;
  toggleSort: (column: string) => Promise<void>;
  setHeaderMenu: (menu: { col: string; x: number; y: number }) => void;
}

/** The grid's header row: each column's name, its FK chip, and its sort chevron. */
function GridHeader({ columns, columnMeta, sort, thRefs, toggleSort, setHeaderMenu }: Props) {
  const { t } = useTranslation();
  return (
    <thead>
      <tr>
        {columns.map((c) => {
          const sorted = sort?.column === c ? sort : null;
          const foreignKey = columnMeta[c]?.foreignKey ?? null;
          return (
            <th
              key={c}
              ref={(el) => {
                if (el) thRefs.current.set(c, el);
                else thRefs.current.delete(c);
              }}
              className={styles.headerCell}
              // `aria-sort` is what tells a screen reader the grid is ordered by this
              // column; the chevron only says it to the eye.
              aria-sort={sorted ? (sorted.desc ? "descending" : "ascending") : "none"}
              // On a Mac the secondary click is also `Ctrl+Click`, which WebKit reports as
              // an ordinary click on top of the `contextmenu` — without this, asking the
              // header for its menu would re-sort the grid underneath it.
              onClick={(e) => {
                if (IS_MAC && e.ctrlKey) return;
                void toggleSort(c);
              }}
              // The header name cannot be selected — `.headerCell` turns that off so that
              // clicking a column repeatedly to cycle its sort does not start highlighting
              // it — so the menu is the only way to take a copy of it.
              onContextMenu={(e) => {
                e.preventDefault();
                setHeaderMenu({ col: c, x: e.clientX, y: e.clientY });
              }}
            >
              {/* Around the name rather than around the whole cell, and not only because a
                  hook cannot be called from this loop: the FK chip beside it has a tooltip
                  of its own, and a cell-wide one would still count as hovered while the
                  pointer sat on the chip — two bubbles at once. Side by side, entering one
                  leaves the other. */}
              <Tooltip
                text={t(
                  sorted
                    ? sorted.desc
                      ? "sqlTable.sortDesc"
                      : "sqlTable.sortAsc"
                    : "sqlTable.sortNone",
                  { column: c },
                )}
              >
                {c}
              </Tooltip>
              {foreignKey && (
                // Its own tooltip, so what the column points at is readable without having
                // to remember the schema. Drawn by the app rather than by `title`, which
                // would have put it in the system's font rather than the one the grid is
                // drawn in.
                <Tooltip
                  text={t("sqlTable.foreignKey", {
                    table: foreignKey.table,
                    column: foreignKey.column,
                  })}
                >
                  <span className={styles.fkBadge}>FK</span>
                </Tooltip>
              )}
              {/* Always rendered, empty when unsorted: the column is measured for the
                  edit input's width, and an indicator that comes and goes would change
                  that width under it. */}
              <span className={styles.sortIcon}>
                {sorted && (sorted.desc ? <ChevronDownIcon /> : <ChevronUpIcon />)}
              </span>
            </th>
          );
        })}
      </tr>
    </thead>
  );
}

export default GridHeader;
