/**
 * The table the content panes are on, and the one held above the sidebar's list.
 *
 * `pinned` is put there by following a foreign key out of the grid. The list itself is left exactly
 * as it was — the search box especially, which is very often what the user was in the middle of
 * when a key took them somewhere else, and clearing it would lose the search they will want back.
 * The table they were sent to is instead shown on its own row at the top, in reach whatever the list
 * underneath is filtered down to, until they pick another table — which is the moment they no
 * longer need a way back to this one.
 */
export interface TableSelection {
  table: string | null;
  pinned: string | null;
}

export const NO_TABLE: TableSelection = { table: null, pinned: null };

export type SelectionAction =
  /** A table chosen from the sidebar — either row of it. Choosing anything other than the pinned
   *  table takes the pin down: it was a way back from a key that has now been followed away from. */
  | { type: "select"; table: string }
  /** A table named somewhere other than the sidebar, such as `Ctrl+Click` in a script. */
  | { type: "open"; table: string }
  /** A foreign key followed out of the grid. */
  | { type: "follow"; table: string }
  /** Made from here, and selected so the columns it still needs are one tab away. */
  | { type: "created"; table: string }
  /** The pinned row follows a rename rather than being dropped: it is the same table, and it is
   *  still the way back to where the user came from. */
  | { type: "renamed"; from: string; to: string }
  | { type: "dropped"; table: string }
  /** The list as read again: a pinned table it no longer holds has nothing left to pin above it. */
  | { type: "listed"; tables: string[] }
  /** Another database, or none. */
  | { type: "cleared" };

export function tableSelection(selection: TableSelection, action: SelectionAction): TableSelection {
  const { table, pinned } = selection;
  switch (action.type) {
    case "select":
      return { table: action.table, pinned: action.table === pinned ? pinned : null };
    case "open":
      return { table: action.table, pinned: null };
    case "follow":
      return { table: action.table, pinned: action.table };
    case "created":
      return { ...selection, table: action.table };
    case "renamed":
      if (table !== action.from && pinned !== action.from) return selection;
      return {
        table: table === action.from ? action.to : table,
        pinned: pinned === action.from ? action.to : pinned,
      };
    case "dropped":
      return {
        table: table === action.table ? null : table,
        pinned: pinned === action.table ? null : pinned,
      };
    case "listed":
      return pinned === null || action.tables.includes(pinned) ? selection : { ...selection, pinned: null };
    case "cleared":
      return NO_TABLE;
  }
}
