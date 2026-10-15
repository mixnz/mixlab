import { arityLookup, type FilterOperatorSpec, type QueryFilter } from "../workspace/filters";

/**
 * Every operator a document filter can use, in the order the dropdown offers them. Each `id` is
 * what the backend matches on (`build_filter` in `src-tauri/src/db/mongo.rs`) and also names its
 * label under `noSqlTable.op.*` — an operator added here needs an entry in both.
 *
 * Mostly the SQL set, with the differences Mongo forces: no `LIKE`, since a regex says the same
 * thing and is what the driver actually takes, and `exists` / `notExists` / `type` added, because
 * in a schemaless collection "the field isn't there" and "the field is null" are two different
 * questions and only Mongo can be asked either.
 */
export const MONGO_FILTER_OPERATORS = [
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
  { id: "regexp", arity: "one" },
  { id: "notRegexp", arity: "one" },
  { id: "in", arity: "list" },
  { id: "notIn", arity: "list" },
  { id: "between", arity: "pair" },
  { id: "notBetween", arity: "pair" },
  { id: "type", arity: "one" },
  { id: "exists", arity: "none" },
  { id: "notExists", arity: "none" },
  { id: "isNull", arity: "none" },
  { id: "isNotNull", arity: "none" },
  { id: "isEmpty", arity: "none" },
  { id: "isNotEmpty", arity: "none" },
] as const satisfies readonly FilterOperatorSpec[];

export type MongoFilterOperator = (typeof MONGO_FILTER_OPERATORS)[number]["id"];

export const mongoOperatorArity = arityLookup<MongoFilterOperator>(MONGO_FILTER_OPERATORS);

/** One condition sent to the server, ANDed with the others. `column` is a field path — a dotted
 * one (`address.city`) reaches into a subdocument, the same as it would in the shell. */
export type MongoFilter = QueryFilter<MongoFilterOperator>;

/** Folds the top-level keys of a freshly loaded page into the fields the bar already offers.
 *
 * A collection has no schema to read, so the list is built up from the documents themselves: the
 * first page seeds it, and every page after only ever adds to it. Nothing is taken away — a page
 * that happens not to carry a field says nothing about whether the collection has one, and a list
 * that narrowed itself each time a filter matched fewer documents would take away the very field
 * the user was filtering on.
 *
 * `_id` leads, being what a lookup is nearly always by and the one field every document is
 * guaranteed to have; the rest keep the order they were first met in. The list comes back
 * unchanged, identity included, when the page held nothing new — the select has no reason to
 * rebuild its options for a page that taught it nothing. */
export function mergeDocumentFields(
  known: string[],
  documents: Record<string, unknown>[],
): string[] {
  const fields = new Set<string>(["_id", ...known]);
  const before = fields.size;
  for (const doc of documents) {
    for (const key of Object.keys(doc)) fields.add(key);
  }
  return fields.size === before && before === known.length ? known : [...fields];
}
