import { arityLookup, type FilterOperatorSpec, type QueryFilter } from "../workspace/filters";

/** Every operator a filter row can use, in the order the dropdown offers them: comparisons first,
 * then the text matches, then the set/range ones, and the value-less ones last. Each `id` is what
 * the backend matches on (`build_where` in `src-tauri/src/db/mysql.rs`) and also names its label
 * under `sqlTable.op.*` — an operator added here needs an entry in both. */
export const FILTER_OPERATORS = [
  { id: "eq", arity: "one" },
  { id: "ne", arity: "one" },
  { id: "gt", arity: "one" },
  { id: "gte", arity: "one" },
  { id: "lt", arity: "one" },
  { id: "lte", arity: "one" },
  { id: "contains", arity: "one" },
  { id: "notContains", arity: "one" },
  { id: "startsWith", arity: "one" },
  { id: "endsWith", arity: "one" },
  { id: "like", arity: "one" },
  { id: "notLike", arity: "one" },
  { id: "regexp", arity: "one" },
  { id: "notRegexp", arity: "one" },
  { id: "in", arity: "list" },
  { id: "notIn", arity: "list" },
  { id: "between", arity: "pair" },
  { id: "notBetween", arity: "pair" },
  { id: "isNull", arity: "none" },
  { id: "isNotNull", arity: "none" },
  { id: "isEmpty", arity: "none" },
  { id: "isNotEmpty", arity: "none" },
] as const satisfies readonly FilterOperatorSpec[];

export type FilterOperator = (typeof FILTER_OPERATORS)[number]["id"];

export const operatorArity = arityLookup<FilterOperator>(FILTER_OPERATORS);

/** One condition sent to the server, ANDed with the others. */
export type SqlFilter = QueryFilter<FilterOperator>;
