import { useCallback, useEffect, useRef, useState } from "react";
import type { Dispatch, SetStateAction } from "react";
import { useTranslation } from "../../../../i18n";
import { errorMessage } from "../../../../core/errors";
import type { SqlApi } from "../../sql/api";
import { cacheKey as tableCacheKey } from "../../schemaTokens";
import { displayValue, normalizeCellValue } from "./cells";
import type { TableCache } from "./request";

export interface EditingCell {
  rowIndex: number;
  col: string;
}

/** The row edited but not yet written: the cells changed, and what they held before. */
export interface PendingRow {
  rowIndex: number;
  changes: Record<string, string | null>;
  original: Record<string, unknown>;
}

interface Options {
  api: SqlApi;
  connectionId: string;
  selectedDb: string;
  selectedTable: string;
  schemaToken: number;
  rows: Record<string, unknown>[];
  setRows: Dispatch<SetStateAction<Record<string, unknown>[]>>;
  columns: string[];
  primaryKey: string[];
  tableCache: TableCache;
  readOnly: boolean;
  onError: (message: string) => void;
}

/** Editing a cell: the one open, the text in it, the row staged behind it, and writing that row to
 *  the server when the edit leaves it. */
export default function useCellEditing({
  api,
  connectionId,
  selectedDb,
  selectedTable,
  schemaToken,
  rows,
  setRows,
  columns,
  primaryKey,
  tableCache,
  readOnly,
  onError,
}: Options) {
  const { t } = useTranslation();
  const [editingCell, setEditingCellState] = useState<EditingCell | null>(null);
  const [editValue, setEditValue] = useState("");
  const editingCellRef = useRef<EditingCell | null>(null);
  const pendingRowRef = useRef<PendingRow | null>(null);
  const editInputRef = useRef<HTMLInputElement | HTMLTextAreaElement | null>(null);
  const currentTableRef = useRef({ db: selectedDb, table: selectedTable });

  // Stable, because the effect below that closes the editor on another table calls it.
  const setEditingCell = useCallback((next: EditingCell | null) => {
    editingCellRef.current = next;
    setEditingCellState(next);
  }, []);

  useEffect(() => {
    currentTableRef.current = { db: selectedDb, table: selectedTable };
  }, [selectedDb, selectedTable]);

  useEffect(() => {
    const el = editInputRef.current;
    if (!editingCell || !el) return;
    el.focus();
    el.select();
  }, [editingCell]);

  useEffect(() => {
    const el = editInputRef.current;
    if (!editingCell || !(el instanceof HTMLTextAreaElement)) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, [editValue, editingCell]);

  // A table swapped for another, or changed under the grid by the app, leaves nothing open to edit
  // and nothing staged to write: the restore that follows puts other rows on screen.
  useEffect(() => {
    setEditingCell(null);
    pendingRowRef.current = null;
  }, [selectedDb, selectedTable, schemaToken, setEditingCell]);

  function commitEditingCell() {
    const cell = editingCellRef.current;
    if (!cell) return;
    const { rowIndex, col } = cell;
    const row = rows[rowIndex];
    if (!row) return;
    const oldNorm = normalizeCellValue(row[col]);
    const newVal = editValue === "NULL" ? null : editValue;
    if (newVal === oldNorm) return;
    const prev = pendingRowRef.current;
    const sameRow = prev && prev.rowIndex === rowIndex;
    const changes = { ...(sameRow ? prev.changes : {}), [col]: newVal };
    const original = { ...(sameRow ? prev.original : {}) };
    if (!(col in original)) original[col] = row[col];
    pendingRowRef.current = { rowIndex, changes, original };
    // Optimistically reflect the edit in read mode immediately — the actual
    // UPDATE is still batched/deferred via pendingRowRef.
    setRows((prevRows) => prevRows.map((r, i) => (i === rowIndex ? { ...r, [col]: newVal } : r)));
  }

  async function flushPendingRow() {
    const pending = pendingRowRef.current;
    pendingRowRef.current = null;
    if (!pending || Object.keys(pending.changes).length === 0) return;
    const row = rows[pending.rowIndex];
    if (!row) return;
    const dbAtFlush = selectedDb;
    const tableAtFlush = selectedTable;
    const keyCols = primaryKey.length > 0 ? primaryKey : columns;
    const key: Record<string, string | null> = {};
    for (const c of keyCols) {
      // Key columns already optimistically edited must use their pre-edit
      // value — `row` reflects the optimistic update, not what's in MySQL yet.
      key[c] = c in pending.original ? normalizeCellValue(pending.original[c]) : normalizeCellValue(row[c]);
    }
    try {
      await api.updateRow(connectionId, dbAtFlush, tableAtFlush, pending.changes, key);
    } catch (e) {
      onError(errorMessage(t, e));
      // The value MySQL refused is on screen and may already be filed away: the grid is put away as
      // it stands the moment the user leaves the table, which is exactly what they do while a write
      // is still out. Nothing here can be rolled back in the cache — the entry was written from a
      // render that had the edit in it — so it goes, and the next visit reads the row as the server
      // has it. Free while the table is still up: the rows are held in state, and the entry is
      // written again from them on the way out.
      tableCache.delete(tableCacheKey(dbAtFlush, tableAtFlush));
      // On screen, only if the grid is still showing the table the write was for. Otherwise there is
      // nothing of this row rendered to put back, and the rows in hand belong to another table.
      if (currentTableRef.current.db !== dbAtFlush || currentTableRef.current.table !== tableAtFlush) {
        return;
      }
      setRows((prevRows) =>
        prevRows.map((r, i) => (i === pending.rowIndex ? { ...r, ...pending.original } : r)),
      );
    }
  }

  async function commitAndExit() {
    commitEditingCell();
    setEditingCell(null);
    await flushPendingRow();
  }

  /**
   * Backs out of the open editor and puts the row back the way the server has it — the text being
   * typed goes, and so do the cells of the same row already staged but not yet written.
   *
   * This is Escape, and it is also what `Ctrl+F` does on its way out of the grid. Leaving a cell is
   * normally taken as agreeing to what is in it, because the user pointed at somewhere else in the
   * table; a shortcut aimed at the filter bar says nothing about the edit, and writing it to the
   * server on the strength of that is a change nobody asked for.
   */
  function cancelEdit() {
    const cell = editingCellRef.current;
    if (!cell) return;
    const pending = pendingRowRef.current;
    if (pending && pending.rowIndex === cell.rowIndex) {
      pendingRowRef.current = null;
      setRows((prevRows) =>
        prevRows.map((r, i) => (i === pending.rowIndex ? { ...r, ...pending.original } : r)),
      );
    }
    setEditingCell(null);
  }

  async function moveEditTo(rowIndex: number, col: string) {
    // The one door into edit mode — a double-click and a Tab both come through here — so this is
    // the one place a read-only connection has to be turned away. Nothing is said about it: a cell
    // that simply does not open reads as a grid for looking at, and the reason is on the buttons
    // below, where someone who wants to change something is already looking.
    if (readOnly) return;
    const leavingRow = editingCellRef.current?.rowIndex;
    commitEditingCell();
    if (leavingRow !== undefined && leavingRow !== rowIndex) {
      await flushPendingRow();
    }
    setEditingCell({ rowIndex, col });
    setEditValue(displayValue(rows[rowIndex]?.[col]));
  }

  function handleInputBlur(rowIndex: number, col: string) {
    const current = editingCellRef.current;
    if (!current || current.rowIndex !== rowIndex || current.col !== col) return;
    void commitAndExit();
  }

  function handleEditChange(e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) {
    setEditValue(e.target.value);
  }

  function handleEditKeyDown(e: React.KeyboardEvent<HTMLInputElement | HTMLTextAreaElement>) {
    const cell = editingCellRef.current;
    if (!cell) return;
    if (e.key === "Escape") {
      e.preventDefault();
      cancelEdit();
      return;
    }
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void commitAndExit();
      return;
    }
    if (e.key === "Tab") {
      e.preventDefault();
      const colIdx = columns.indexOf(cell.col);
      const nextColIdx = colIdx + (e.shiftKey ? -1 : 1);
      if (nextColIdx < 0 || nextColIdx >= columns.length) return;
      void moveEditTo(cell.rowIndex, columns[nextColIdx]);
      return;
    }
    if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      e.preventDefault();
      const nextRow = cell.rowIndex + (e.key === "ArrowUp" ? -1 : 1);
      if (nextRow < 0 || nextRow >= rows.length) return;
      void moveEditTo(nextRow, cell.col);
      return;
    }
    if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      const target = e.currentTarget;
      const atStart = target.selectionStart === 0 && target.selectionEnd === 0;
      const atEnd = target.selectionStart === target.value.length && target.selectionEnd === target.value.length;
      if (e.key === "ArrowLeft" && !atStart) return;
      if (e.key === "ArrowRight" && !atEnd) return;
      const colIdx = columns.indexOf(cell.col);
      const nextColIdx = colIdx + (e.key === "ArrowLeft" ? -1 : 1);
      if (nextColIdx < 0 || nextColIdx >= columns.length) return;
      e.preventDefault();
      void moveEditTo(cell.rowIndex, columns[nextColIdx]);
    }
  }

  return {
    editingCell,
    editValue,
    editingCellRef,
    pendingRowRef,
    editInputRef,
    commitAndExit,
    cancelEdit,
    moveEditTo,
    handleInputBlur,
    handleEditChange,
    handleEditKeyDown,
  };
}
