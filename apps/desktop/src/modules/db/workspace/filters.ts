/** The parts of a filter bar that don't care which database is underneath: how many values an
 * operator reads out of one text box, how that box is split, and what a row of the bar holds
 * while it is being edited. The operator sets themselves are per-database — see
 * `src/mysql/filters.ts` and `src/mongo/filters.ts`. */

/** How many values an operator reads out of the one text box the filter row gives it. */
export type FilterArity =
  /** None — `IS NULL` and its kind stand on their own, and the box is left disabled. */
  | "none"
  /** One, taken as typed. */
  | "one"
  /** A comma-separated list, for `IN` / `NOT IN`. */
  | "list"
  /** Two bounds, `min,max`, for `BETWEEN` / `NOT BETWEEN`. */
  | "pair";

/** One entry of a database's operator list: the id the backend matches on, and how its value is
 * read. The label lives in the translations, keyed by the same id. */
export interface FilterOperatorSpec {
  id: string;
  arity: FilterArity;
}

/** One row of the bar while it is being edited — a condition plus the two things only the UI
 * cares about: whether it is switched on, and what tells it apart from an identical row. */
export interface FilterRow<Op extends string = string> {
  /** Identity for React and for the edit handlers, since two rows may otherwise be equal and a
   * row's position changes as the ones above it are removed. */
  id: number;
  /** The checkbox at the head of the row: off leaves the condition written down but unapplied. */
  enabled: boolean;
  column: string;
  operator: Op;
  value: string;
}

/** One condition sent to the server. They are ANDed together; the value is always text, and the
 * operator is what says how to read it. */
export interface QueryFilter<Op extends string = string> {
  column: string;
  operator: Op;
  value: string;
}

let nextRowId = 1;

/** Looks an operator's arity up in a database's own list. Built once per list rather than
 * searched each time, since every render of the bar asks for every row's operator. */
export function arityLookup<Op extends string>(
  operators: readonly FilterOperatorSpec[],
): (operator: Op) => FilterArity {
  const table = new Map(operators.map((op) => [op.id, op.arity]));
  return (operator) => table.get(operator) ?? "one";
}

/**
 * Splits an `IN`/`BETWEEN` value into its items — `1,2,3` or `abc,xyz`, each item trimmed.
 *
 * An item may be wrapped in single or double quotes, which is how a value that itself holds a
 * comma (or spaces that matter) gets through: the quotes come off and what was inside them is
 * taken as-is. Empty items are dropped, so a trailing comma costs nothing — but a quoted `''`
 * stays, being the only way to put an empty string in a list.
 *
 * This mirrors `split_list` in `src-tauri/src/db/filters.rs`, which is what actually builds the
 * conditions; the two have to agree on how many items a value holds or a row would be sent as
 * complete and come back matching something else.
 */
export function splitFilterList(raw: string): string[] {
  const items: string[] = [];
  let current = "";
  let quote: string | null = null;
  let quoted = false;

  function flush() {
    const item = quoted ? current : current.trim();
    if (quoted || item !== "") items.push(item);
    current = "";
    quoted = false;
  }

  for (const ch of raw) {
    if (quote !== null) {
      if (ch === quote) quote = null;
      else current += ch;
      continue;
    }
    if (ch === "'" || ch === '"') {
      // Whitespace between the comma and the opening quote is padding around the item rather than
      // part of it — a quoted item is not trimmed afterwards, so `'a', 'b'` would otherwise ask
      // for a value beginning with a space.
      if (current.trim() === "") current = "";
      quote = ch;
      quoted = true;
      continue;
    }
    if (ch === ",") {
      flush();
      continue;
    }
    // The same padding on the other side of the closing quote.
    if (quoted && ch.trim() === "") continue;
    current += ch;
  }
  flush();
  return items;
}

/** Whether a row has everything its operator needs to be worth sending. A row that hasn't been
 * filled in yet is left out of the query entirely rather than matched literally — the bar opens
 * with an empty `id =` row, and that must not mean "the rows whose id is the empty string". */
export function isFilterComplete(arity: FilterArity, value: string): boolean {
  switch (arity) {
    case "none":
      return true;
    case "list":
      return splitFilterList(value).length > 0;
    case "pair":
      return splitFilterList(value).length >= 2;
    default:
      return value !== "";
  }
}

/** The column a row starts on: the id column when there is one — it is what a lookup is nearly
 * always by, and Mongo's `_id` counts — and otherwise the first column, so the row is never left
 * pointing at nothing. */
function startingColumn(columns: string[]): string {
  return columns.find((c) => c.toLowerCase() === "id" || c.toLowerCase() === "_id") ?? columns[0] ?? "";
}

/** One row of the bar for a condition that is already known — following a foreign key writes the
 * whole of it, column, operator and value together, rather than leaving anything to be typed. */
export function filterRowFor<Op extends string>(column: string, operator: Op, value: string): FilterRow<Op> {
  return { id: nextRowId++, enabled: true, column, operator, value };
}

export function createFilterRow<Op extends string>(columns: string[], operator: Op): FilterRow<Op> {
  return filterRowFor(startingColumn(columns), operator, "");
}

/** What the bar holds when a table is first opened: an empty `id =` row, ready for the lookup
 * that is about to be typed into it. A table with no id column starts with no rows at all —
 * there is no column to guess at, and an arbitrary one would only be in the way. */
export function initialFilterRows<Op extends string>(columns: string[], operator: Op): FilterRow<Op>[] {
  const hasId = columns.some((c) => c.toLowerCase() === "id" || c.toLowerCase() === "_id");
  return hasId ? [createFilterRow(columns, operator)] : [];
}

/** The conditions that actually reach the query: the rows that are switched on and filled in.
 * A row whose operator still wants a value is dropped rather than sent — see
 * {@link isFilterComplete}. */
export function toQueryFilters<Op extends string>(
  rows: FilterRow<Op>[],
  arityOf: (operator: Op) => FilterArity,
): QueryFilter<Op>[] {
  return rows
    .filter((row) => row.enabled && row.column !== "" && isFilterComplete(arityOf(row.operator), row.value))
    .map(({ column, operator, value }) => ({ column, operator, value }));
}
