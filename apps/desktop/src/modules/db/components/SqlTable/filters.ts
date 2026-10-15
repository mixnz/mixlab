import type { FilterRow } from "../../workspace/filters";
import type { FilterOperator, SqlFilter } from "../../sql/filters";

/** One table's filter bar as it was left behind: the rows still being edited, and the conditions
 * that were actually running against the grid. */
export interface RememberedFilters {
  rows: FilterRow<FilterOperator>[];
  applied: SqlFilter[];
  /** Which shape of the database the conditions were written against — see `schemaToken` on
   *  `SqlTable`'s props. */
  schemaToken: number;
}

/** Every table's bar, by the table it belongs to. Held by the workspace rather than here: the grid
 * is unmounted whenever the header leaves the Data tab, and a cache living inside it would go with
 * it — the conditions have to outlive a trip to Structure or Query, not just a trip to another
 * table. */
export type FilterCache = Map<string, RememberedFilters>;

/**
 * The bar remembered for a table, or nothing when there is nothing worth speaking for.
 *
 * Conditions written before the app last changed this table are nothing: they name columns that may
 * since have been renamed or dropped, and the name they are filed under may since have been dropped
 * and given to a different table altogether — put back onto that table, they answer the next read
 * with `Unknown column` rather than with rows. Deleting the entry at the moment of the change is not
 * enough on its own: the grid is still holding the bar in state and files it straight back on the way
 * out, so the check has to be here, where the cache is read.
 */
export function rememberedFilters(
  cache: FilterCache,
  key: string,
  schemaToken: number,
): RememberedFilters | undefined {
  const entry = cache.get(key);
  return entry?.schemaToken === schemaToken ? entry : undefined;
}
