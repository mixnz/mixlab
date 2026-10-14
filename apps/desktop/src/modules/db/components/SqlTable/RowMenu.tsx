import ContextMenu from "../../../../components/ContextMenu";
import { useTranslation } from "../../../../i18n";
import { withReloadShortcut } from "../../../../core/reload";
import { displayValue } from "./cells";
import { csvText, insertStatements, jsonText, spreadsheetText } from "./rowText";

export /** Which cell the right-click menu was opened on, and where the pointer was when it was. Null
 *  while the menu is closed. */
interface CellMenu {
  rowIndex: number;
  col: string;
  x: number;
  y: number;
}

interface Props {
  menu: CellMenu;
  /** The row the menu was opened on. */
  menuRow: Record<string, unknown>;
  /** The rows its row entries act on — the selection, or the row itself. */
  menuRows: Record<string, unknown>[];
  /** Where the cell under the menu points, when it is a foreign key holding a value. */
  menuForeignKey: { table: string; column: string; value: string } | null;
  columns: string[];
  selectedTable: string;
  insertableColumns: string[];
  binaryColumns: Set<string>;
  autoIncrementColumn: string | null;
  loading: boolean;
  deleting: boolean;
  closeMenu: () => void;
  setExpandedCell: (cell: { rowIndex: number; col: string }) => void;
  copyToClipboard: (text: string) => void;
  openReferenced: (fk: { table: string; column: string; value: string }) => void;
  reload: () => Promise<void>;
}

/** The right-click menu of a cell being read: open it, copy it, follow it, copy its rows. */
function RowMenu({
  menu,
  menuRow,
  menuRows,
  menuForeignKey,
  columns,
  selectedTable,
  insertableColumns,
  binaryColumns,
  autoIncrementColumn,
  loading,
  deleting,
  closeMenu,
  setExpandedCell,
  copyToClipboard,
  openReferenced,
  reload,
}: Props) {
  const { t } = useTranslation();
  return (
    <ContextMenu x={menu.x} y={menu.y} onClose={closeMenu}>
      <button
        type="button"
        onClick={() => {
          setExpandedCell({ rowIndex: menu.rowIndex, col: menu.col });
          closeMenu();
        }}
      >
        {t("sqlTable.expandCell")}
      </button>
      <button type="button" onClick={() => copyToClipboard(displayValue(menuRow[menu.col]))}>
        {t("sqlTable.copyCellValue")}
      </button>
      {menuForeignKey !== null && (
        <button type="button" onClick={() => openReferenced(menuForeignKey)}>
          {t("sqlTable.openReferencedRow", { table: menuForeignKey.table })}
        </button>
      )}
      <div className="context-menu-separator" />
      <button
        type="button"
        onClick={() =>
          copyToClipboard(
            insertStatements(selectedTable, insertableColumns, menuRows, null, binaryColumns),
          )
        }
      >
        {menuRows.length === 1
          ? t("sqlTable.copyInsert")
          : t("sqlTable.copyInsertRows", { n: menuRows.length })}
      </button>
      {/* Only a table MySQL numbers itself has a column an insert can sensibly leave out.
          Everywhere else every column is a value somebody has to provide, and an INSERT short
          of one would simply be refused — so the entry is not offered rather than offered and
          broken. */}
      {autoIncrementColumn !== null && (
        <button
          type="button"
          onClick={() =>
            copyToClipboard(
              insertStatements(
                selectedTable,
                insertableColumns,
                menuRows,
                autoIncrementColumn,
                binaryColumns,
              ),
            )
          }
        >
          {menuRows.length === 1
            ? t("sqlTable.copyInsertWithout", { column: autoIncrementColumn })
            : t("sqlTable.copyInsertRowsWithout", {
                n: menuRows.length,
                column: autoIncrementColumn,
              })}
        </button>
      )}
      <button type="button" onClick={() => copyToClipboard(spreadsheetText(columns, menuRows))}>
        {menuRows.length === 1
          ? t("sqlTable.copyAsTsv")
          : t("sqlTable.copyRowsAsTsv", { n: menuRows.length })}
      </button>
      <button type="button" onClick={() => copyToClipboard(csvText(columns, menuRows))}>
        {menuRows.length === 1
          ? t("sqlTable.copyAsCsv")
          : t("sqlTable.copyRowsAsCsv", { n: menuRows.length })}
      </button>
      <button type="button" onClick={() => copyToClipboard(jsonText(columns, menuRows))}>
        {menuRows.length === 1
          ? t("sqlTable.copyAsJson")
          : t("sqlTable.copyRowsAsJson", { n: menuRows.length })}
      </button>
      <div className="context-menu-separator" />
      <button
        type="button"
        disabled={loading || deleting}
        onClick={() => {
          closeMenu();
          void reload();
        }}
      >
        {withReloadShortcut(t("sqlTable.reloadRows"))}
      </button>
    </ContextMenu>
  );
}

export default RowMenu;
