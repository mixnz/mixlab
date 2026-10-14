import { useCallback, useRef, useState } from "react";
import { hasPrimaryModifier } from "../../../../core/platform";

/** Which rows of the page are selected, and the ways a list changes that. */
export default function useRowSelection(rows: Record<string, unknown>[]) {
  // Selected rows are held as indices into the current page: selection is a
  // property of what is on screen, and every path that replaces the rows
  // (paging, reloading, switching table) clears it.
  const [selectedRows, setSelectedRows] = useState<Set<number>>(new Set());
  // Where a shift-click range starts. A ref rather than state because it is
  // only ever read from inside an event handler.
  const anchorRowRef = useRef<number | null>(null);

  // Stable, because the component's refetch effect calls it and lists it.
  const clearSelection = useCallback(() => {
    setSelectedRows(new Set());
    anchorRowRef.current = null;
  }, []);

  /** Every row on the page, with the range anchored at the first. */
  function selectAllRows() {
    setSelectedRows(new Set(rows.map((_, i) => i)));
    anchorRowRef.current = 0;
  }

  /** Just the one row, anchored there: what a right-click outside the selection makes of it. */
  function selectOnly(rowIndex: number) {
    setSelectedRows(new Set([rowIndex]));
    anchorRowRef.current = rowIndex;
  }

  /** Extends, toggles or replaces the selection the way a list does: plain click selects the one
   * row, the platform's own modifier toggles it, shift takes everything between the anchor and it. */
  function selectRow(rowIndex: number, e: React.MouseEvent) {
    const toggle = hasPrimaryModifier(e);
    const anchor = anchorRowRef.current;
    if (e.shiftKey && anchor !== null) {
      const from = Math.min(anchor, rowIndex);
      const to = Math.max(anchor, rowIndex);
      setSelectedRows((prev) => {
        const next = toggle ? new Set(prev) : new Set<number>();
        for (let i = from; i <= to; i++) next.add(i);
        return next;
      });
      return;
    }
    anchorRowRef.current = rowIndex;
    if (toggle) {
      setSelectedRows((prev) => {
        const next = new Set(prev);
        if (!next.delete(rowIndex)) next.add(rowIndex);
        return next;
      });
      return;
    }
    setSelectedRows(new Set([rowIndex]));
  }

  /** The selected rows themselves, in the order they sit on screen — `selectedRows` holds their
   * indices, and a Set keeps no order of its own. */
  function selectedRowsInOrder(): Record<string, unknown>[] {
    return [...selectedRows]
      .sort((a, b) => a - b)
      .map((i) => rows[i])
      .filter((row): row is Record<string, unknown> => row !== undefined);
  }

  return { selectedRows, clearSelection, selectRow, selectAllRows, selectOnly, selectedRowsInOrder };
}
