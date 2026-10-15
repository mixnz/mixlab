import { useEffect, useState } from "react";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import { useSqlApi } from "../../sql/context";
import type {
  SqlColumnSpec,
  SqlIndexSpec,
  SqlSkipIndex,
  SqlSkipIndexSpec,
  SqlStructureColumn,
  SqlTableIndex,
} from "../../types";

/** Which dialog is open and on what: an entry with nothing in it is the "add" form. */
export type ColumnDialogState = { column?: SqlStructureColumn };
export type IndexDialogState = { index?: SqlTableIndex };
export type SkipIndexDialogState = { index?: SqlSkipIndex };
/** What the confirmation is about to drop. */
export type PendingDrop =
  | { kind: "column"; name: string }
  | { kind: "index"; name: string }
  | { kind: "skipIndex"; name: string };

interface Options {
  connectionId: string;
  selectedDb: string;
  selectedTable: string;
  onError: (message: string) => void;
  /** Told that an `ALTER TABLE` has just landed — see `TableStructure`'s prop of the same name. */
  onSchemaChanged: () => void;
}

/** What the Structure tab sends: the dialogs that ask for a change, the confirmation before a
 *  drop, and the `ALTER TABLE` each of them ends in. */
export function useStructureEdits({ connectionId, selectedDb, selectedTable, onError, onSchemaChanged }: Options) {
  const { t } = useTranslation();
  const api = useSqlApi();
  const [saving, setSaving] = useState(false);
  const [columnDialog, setColumnDialog] = useState<ColumnDialogState | null>(null);
  const [indexDialog, setIndexDialog] = useState<IndexDialogState | null>(null);
  const [skipIndexDialog, setSkipIndexDialog] = useState<SkipIndexDialogState | null>(null);
  const [orderByDialogOpen, setOrderByDialogOpen] = useState(false);
  const [orderByRowCount, setOrderByRowCount] = useState<number | null>(null);
  const [pendingDrop, setPendingDrop] = useState<PendingDrop | null>(null);

  // Read only while the dialog asking about it is open — the same reason the collations list
  // (`useTableShape`) is read once and not on every render, but this one is per-table rather than
  // per-connection, so it is keyed on the dialog being open rather than on mount.
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
      // what it does — which is what brings the columns back through `useTableShape`'s read.
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

  return {
    saving,
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
  };
}

export type StructureEdits = ReturnType<typeof useStructureEdits>;
