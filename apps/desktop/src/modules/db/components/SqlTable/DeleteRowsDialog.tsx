import ConfirmDialog from "../../../../components/ConfirmDialog";
import Checkbox from "../../../../components/Checkbox";
import { useTranslation } from "../../../../i18n";
import styles from "./SqlTable.module.css";

interface Props {
  selectedRows: Set<number>;
  total: number;
  autoIncrementColumn: string | null;
  /** Whether the two options are offered — see where `SqlTable` works them out. */
  canDeleteWholeTable: boolean;
  canResetAutoIncrement: boolean;
  deleteWholeTable: boolean;
  setDeleteWholeTable: (on: boolean) => void;
  resetAutoIncrement: boolean;
  setResetAutoIncrement: (on: boolean) => void;
  confirmDelete: () => Promise<void>;
  setConfirmingDelete: (on: boolean) => void;
  focusGrid: () => void;
}

/** Asking before the selected rows are deleted, with the two things a delete can also do. */
function DeleteRowsDialog({
  selectedRows,
  total,
  autoIncrementColumn,
  canDeleteWholeTable,
  canResetAutoIncrement,
  deleteWholeTable,
  setDeleteWholeTable,
  resetAutoIncrement,
  setResetAutoIncrement,
  confirmDelete,
  setConfirmingDelete,
  focusGrid,
}: Props) {
  const { t } = useTranslation();
  return (
    <ConfirmDialog
      title={t("sqlTable.deleteRowsTitle")}
      message={t("sqlTable.deleteRowsMessage", { n: selectedRows.size })}
      confirmLabel={t("common.delete")}
      danger
      onConfirm={() => void confirmDelete()}
      onCancel={() => {
        setConfirmingDelete(false);
        focusGrid();
      }}
    >
      {(canDeleteWholeTable || canResetAutoIncrement) && (
        <div className={styles.deleteOptions}>
          {canDeleteWholeTable && (
            <Checkbox
              className={styles.deleteOption}
              label={t("sqlTable.deleteAllRowsOption", { total })}
              checked={deleteWholeTable}
              onChange={(e) => {
                setDeleteWholeTable(e.target.checked);
                // The reset only exists because of this option on a multi-page table;
                // taking it back takes its follow-up with it.
                if (!e.target.checked) setResetAutoIncrement(false);
              }}
            />
          )}
          {canResetAutoIncrement && (
            <Checkbox
              className={styles.deleteOption}
              label={t("sqlTable.resetAutoIncrementOption", { column: autoIncrementColumn ?? "" })}
              checked={resetAutoIncrement}
              onChange={(e) => setResetAutoIncrement(e.target.checked)}
            />
          )}
        </div>
      )}
    </ConfirmDialog>
  );
}

export default DeleteRowsDialog;
