import type { ChangeEvent, KeyboardEvent, MouseEvent, RefObject } from "react";
import {
  cellPreview,
  type useVirtualColumns,
  type useVirtualRows,
} from "../../../../core/virtualRows";
import type { SqlColumnMeta } from "../../types";
import { displayValue, isMultilineType } from "./cells";
import type { EditingCell, PendingRow } from "./useCellEditing";
import styles from "./SqlTable.module.css";

interface Props {
  rows: Record<string, unknown>[];
  columns: string[];
  columnMeta: Record<string, SqlColumnMeta>;
  /** Whether the rows are windowed, and the window — see `useGridLayout`. */
  virtual: boolean;
  view: ReturnType<typeof useVirtualRows>;
  /** Whether the columns are windowed, the window, and the columns it draws. */
  columnsVirtual: boolean;
  colView: ReturnType<typeof useVirtualColumns>;
  drawnColumns: string[];
  selectedRows: Set<number>;
  editingCell: EditingCell | null;
  editValue: string;
  editInputRef: RefObject<HTMLInputElement | HTMLTextAreaElement | null>;
  columnWidths: Record<string, number>;
  pendingRowRef: RefObject<PendingRow | null>;
  handleEditChange: (e: ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => void;
  handleEditKeyDown: (e: KeyboardEvent<HTMLInputElement | HTMLTextAreaElement>) => void;
  handleInputBlur: (rowIndex: number, col: string) => void;
  handleCellMouseDown: (e: MouseEvent<HTMLTableCellElement>, rowIndex: number, col: string) => void;
  handleCellContextMenu: (
    e: MouseEvent<HTMLTableCellElement>,
    rowIndex: number,
    col: string,
  ) => void;
}

/** The grid's rows: the drawn window of them, the cell open for editing, and the cells being
 *  read. */
function GridBody({
  rows,
  columns,
  columnMeta,
  virtual,
  view,
  columnsVirtual,
  colView,
  drawnColumns,
  selectedRows,
  editingCell,
  editValue,
  editInputRef,
  columnWidths,
  pendingRowRef,
  handleEditChange,
  handleEditKeyDown,
  handleInputBlur,
  handleCellMouseDown,
  handleCellContextMenu,
}: Props) {
  return (
    <tbody>
      {/* The rows outside the window, as the height they would have taken. Two elements
          instead of hundreds, and the scrollbar is the length it would have been. Always
          both, even at zero height: a spacer that disappeared at the end of the page would
          hand `tr:last-child` to a real row and change the table's height at exactly the
          point a scroll is trying to settle. */}
      {virtual && (
        <tr className={styles.spacer} style={{ height: view.padTop }} aria-hidden="true">
          <td colSpan={columns.length} />
        </tr>
      )}
      {rows.slice(view.first, view.last).map((row, offset) => {
        // The index into the page, not into what is drawn. Every other part of this
        // component — the selection, the staged edit, the delete keys — is indexed the same
        // way, so windowing the rows changes none of it.
        const i = view.first + offset;
        return (
          <tr
            key={i}
            className={selectedRows.has(i) ? styles.rowSelected : undefined}
            aria-selected={selectedRows.has(i)}
          >
            {/* The columns outside the window, as one cell spanning them. It needs no
                width of its own: a fixed layout takes every column's from the colgroup
                `SqlTable` draws, which holds all of them whether or not a cell was built for
                them, so the space left here is the space those columns were measured to want. */}
            {columnsVirtual && colView.first > 0 && (
              <td colSpan={colView.first} aria-hidden="true" />
            )}
            {drawnColumns.map((c, at) => {
              // Where this cell sits in the whole table rather than in what was drawn —
              // see `aria-colcount` on the table in `SqlTable`. One-based, as the attribute counts.
              const colIndex = colView.first + at + 1;
              const isEditing = editingCell?.rowIndex === i && editingCell.col === c;
              if (isEditing) {
                const multiline = isMultilineType(columnMeta[c]?.dataType);
                const cellWidth = columnWidths[c];
                return (
                  <td
                    key={c}
                    aria-colindex={colIndex}
                    className={styles.cellEditing}
                    style={cellWidth ? { width: cellWidth } : undefined}
                  >
                    {multiline ? (
                      <textarea
                        ref={editInputRef as React.RefObject<HTMLTextAreaElement>}
                        value={editValue}
                        onChange={handleEditChange}
                        onKeyDown={handleEditKeyDown}
                        onBlur={() => handleInputBlur(i, c)}
                        className={styles.cellTextarea}
                        rows={1}
                        autoComplete="off"
                        autoCorrect="off"
                        autoCapitalize="off"
                        spellCheck={false}
                      />
                    ) : (
                      <input
                        ref={editInputRef as React.RefObject<HTMLInputElement>}
                        type="text"
                        value={editValue}
                        onChange={handleEditChange}
                        onKeyDown={handleEditKeyDown}
                        onBlur={() => handleInputBlur(i, c)}
                        className={styles.cellInput}
                        autoComplete="off"
                        autoCorrect="off"
                        autoCapitalize="off"
                        spellCheck={false}
                      />
                    )}
                  </td>
                );
              }
              const raw = row[c];
              const isNull = raw === null || raw === undefined;
              // What the cell draws, not the value: see `cellPreview`.
              const value = cellPreview(displayValue(raw));
              const isDirty =
                pendingRowRef.current?.rowIndex === i &&
                Object.prototype.hasOwnProperty.call(pendingRowRef.current.changes, c);
              const cellClassName = [isNull && styles.cellNull, isDirty && styles.cellDirty]
                .filter(Boolean)
                .join(" ");
              return (
                <td
                  key={c}
                  aria-colindex={colIndex}
                  title={value}
                  className={cellClassName || undefined}
                  onMouseDown={(e) => handleCellMouseDown(e, i, c)}
                  // Only on a cell being read. The cell open for editing is the branch
                  // above and deliberately has none: right-clicking inside its input is
                  // the webview's own cut/copy/paste, which nothing here replaces.
                  onContextMenu={(e) => handleCellContextMenu(e, i, c)}
                >
                  {value}
                </td>
              );
            })}
            {columnsVirtual && colView.last < columns.length && (
              <td colSpan={columns.length - colView.last} aria-hidden="true" />
            )}
          </tr>
        );
      })}
      {virtual && (
        <tr className={styles.spacer} style={{ height: view.padBottom }} aria-hidden="true">
          <td colSpan={columns.length} />
        </tr>
      )}
    </tbody>
  );
}

export default GridBody;
