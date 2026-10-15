import ConfirmDialog from "../../../../components/ConfirmDialog";
import { useTranslation } from "../../../../i18n";
import type { SqlCollation, SqlStructureColumn, SqlTableIndex } from "../../types";
import ColumnDialog from "../ColumnDialog";
import IndexDialog from "../IndexDialog";
import OrderByDialog from "../OrderByDialog";
import SkipIndexDialog from "../SkipIndexDialog";
import type { StructureEdits } from "./useStructureEdits";

interface Props {
  /** The table the dialogs are about. */
  table: string;
  columns: SqlStructureColumn[];
  indexes: SqlTableIndex[];
  collations: SqlCollation[];
  edits: StructureEdits;
}

/** Whichever of the Structure tab's dialogs is open, on what it was opened for. */
export default function StructureDialogs({ table: selectedTable, columns, indexes, collations, edits }: Props) {
  const { t } = useTranslation();
  const {
    columnDialog,
    setColumnDialog,
    indexDialog,
    setIndexDialog,
    skipIndexDialog,
    setSkipIndexDialog,
    orderByDialogOpen,
    setOrderByDialogOpen,
    orderByRowCount,
    pendingDrop,
    setPendingDrop,
    submitColumn,
    submitIndex,
    submitSkipIndex,
    submitOrderBy,
    confirmDrop,
  } = edits;
  return (
    <>
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
    </>
  );
}
