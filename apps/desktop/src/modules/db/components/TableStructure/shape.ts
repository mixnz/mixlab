import type { SqlEditing } from "../../sql/dialect";
import type { SqlStructureColumn, SqlTableIndex } from "../../types";

/** What the Kind column says an index is — the same reading of it the edit dialog opens on. */
export const INDEX_KIND_LABEL = {
  primary: "structure.kindPrimary",
  unique: "structure.kindUnique",
  fulltext: "structure.kindFulltext",
  spatial: "structure.kindSpatial",
  index: "structure.kindIndex",
} as const;

/** How the index is stored, when that is a choice the index had. Read against the methods this
 * engine offers rather than a fixed pair, so PostgreSQL's `GIN`/`GiST`/`BRIN`/`SP-GiST` are named
 * here as well — the edit dialog fills its picker from the same list. `FULLTEXT`/`SPATIAL` name a
 * kind rather than a structure, and the Kind column already says so. */
export function indexMethod(index: SqlTableIndex, methods: readonly string[]): string {
  const type = index.indexType.toUpperCase();
  return methods.includes(type) ? type : "";
}

/** What marks a default the server evaluates rather than stores. `f(x)` rather than a pair of
 * letters like the badges beside a name: those abbreviate a phrase, and this says what the value
 * beside it is. */
export const EXPRESSION_BADGE = "f(x)";

/** Whether a column's default is one the server works out for each new row rather than a value it
 * keeps — and whether that is a distinction this engine draws at all. */
export function isExpressionDefault(column: SqlStructureColumn, offers: SqlEditing): boolean {
  return offers.markExpressionDefaults && column.defaultIsExpression && column.defaultValue !== null;
}

/** An index over an expression rather than over columns. Its expression is not read here, so such
 * an index cannot be rebuilt from what the grid knows — only dropped. */
export function isFunctional(index: SqlTableIndex): boolean {
  return index.columns.some((column) => column.name === null);
}

/** Where holding every row in the DOM stops being the cheap thing to do. Most tables have fewer
 *  columns than this and nothing here applies to them; a wide table has hundreds, and then the
 *  whole grid is laid out again every time the tab is opened — this panel is unmounted when the
 *  header leaves it, so that is every visit rather than only the first. */
export const VIRTUAL_FROM = 60;

/** How tall a row of these grids is — stated, never measured; see `virtualRows.ts` for why that
 *  distinction is what makes a window of rows sound. 29px is what a row here comes to: a 20px
 *  line, 4px of padding above and below, and the 1px rule. Was 33px on a 24px line. */
export const ROW_HEIGHT = 29;

/** The engines a sorting-key rebuild is allowed to run on — the same four
 *  `clickhouse_ddl::ENGINES` already whitelists for creating a new table (D2 of the main ClickHouse
 *  DDL design doc). `EXCHANGE TABLES`/`CREATE`/`DROP` with no `ON CLUSTER` only ever reach one
 *  replica of a `Replicated*` engine, which this has never been verified against — see the index DDL
 *  design doc's D11. */
export const CLICKHOUSE_REBUILD_ENGINES = [
  "MergeTree",
  "ReplacingMergeTree",
  "SummingMergeTree",
  "AggregatingMergeTree",
];
