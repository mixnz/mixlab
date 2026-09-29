import { likeToRegex } from "./like";
import type { Dialect, Translation, Unsupported, Warning } from "./types";

/*
 * The AST shape below is `node-sql-parser`'s, read off the library itself rather than guessed. A
 * few places are not what their names suggest, and each one nearly became a bug:
 *
 * - `NOT (a = 1)` is **not** a `binary_expr` with operator `NOT`. It is a `function` node named
 *   `NOT`, so it has to be recognised *before* the rejection rule sweeps away every scalar
 * function.
 * - PostgreSQL returns `column_ref.column` as `{ expr: { value } }`, MySQL returns a string.
 *   `columnName` reads both.
 * - `astify` returns an object for one statement and an **array** for several.
 * - `limit` is `{ seperator, value: [limit, offset?] }` — `seperator` (misspelt like that in the
 *   library) is `"offset"` when there is a second element.
 */

/** Enough for the traversal here; the real AST is much wider and we do not need the rest. */
interface Node {
  type?: string;
  [key: string]: unknown;
}

const isNode = (value: unknown): value is Node =>
  typeof value === "object" && value !== null && !Array.isArray(value);

/** The column name, readable from both MySQL's string form and PostgreSQL's object form. */
function columnName(ref: Node): string {
  const column = ref.column;
  if (typeof column === "string") return column;
  if (isNode(column) && isNode(column.expr) && typeof column.expr.value === "string") {
    return column.expr.value;
  }
  return "";
}

/** The name of a `function` node, read from the `name.name[0].value` structure. */
function functionName(node: Node): string {
  const name = node.name;
  if (typeof name === "string") return name;
  if (isNode(name) && Array.isArray(name.name)) {
    const first = name.name[0];
    if (isNode(first) && typeof first.value === "string") return first.value.toUpperCase();
  }
  return "";
}

const AGGREGATES = new Set(["COUNT", "SUM", "AVG", "MIN", "MAX"]);

/**
 * Every reason this statement cannot be translated, collected in one pass.
 *
 * Collects them all rather than stopping at the first: someone pasting a statement with both a JOIN
 * and a subquery should see both at once, not fix one and only then find out about the other.
 */
function collectUnsupported(ast: Node, sql: string): Unsupported[] {
  const found: Unsupported[] = [];
  const add = (code: Unsupported["code"], fragment: string) => {
    if (!found.some((u) => u.code === code)) found.push({ code, fragment: fragment || sql });
  };

  if (ast.with) add("cte", "WITH");
  if (ast._next) add("union", String(ast.set_op ?? "UNION").toUpperCase());

  const from = Array.isArray(ast.from) ? ast.from : [];
  if (from.length > 1) {
    const joined = from.find((entry) => isNode(entry) && entry.join);
    add("join", isNode(joined) ? String(joined.join) : "FROM a, b");
  }

  const walk = (value: unknown) => {
    if (Array.isArray(value)) {
      value.forEach(walk);
      return;
    }
    if (!isNode(value)) return;

    // A subquery appears as a nested node with an `ast` key, or as a nested `select` node.
    if (value.ast !== undefined || (value !== ast && value.type === "select")) {
      add("subquery", "SELECT (…)");
      return;
    }

    if (value.type === "case") add("case", "CASE");
    if (value.over) add("window", "OVER");

    if (value.type === "aggr_func") {
      const name = typeof value.name === "string" ? value.name.toUpperCase() : "";
      if (!AGGREGATES.has(name)) add("function", `${name}(…)`);
    }

    // `NOT` in the guise of a scalar function. Go inside it rather than rejecting it.
    if (value.type === "function" && functionName(value) !== "NOT") {
      add("function", `${functionName(value)}(…)`);
      return;
    }

    Object.values(value).forEach(walk);
  };

  walk(ast.columns);
  walk(ast.where);
  walk(ast.having);
  walk(ast.groupby);
  walk(ast.orderby);

  return found;
}

/** A node's literal value, used directly as a JSON value. */
function literal(node: Node): unknown {
  if (node.type === "null") return null;
  if (node.type === "bool") return node.value;
  return node.value;
}

const OBJECT_ID = /^[0-9a-f]{24}$/i;

/** The context carried through the traversal: the dialect decides whether `LIKE` is
 *  case-sensitive, and warnings are collected here along the way. */
interface Context {
  dialect: Dialect;
  warnings: Warning[];
  /**
   * The name `$group` gave each aggregate function, looked up by {@link aggrKey}.
   *
   * Only present while translating `HAVING`. `HAVING COUNT(*) > 5` talks about **the grouped
   * result**, so its left side is an `aggr_func` rather than a column — without this table the
   * field name comes out empty, and `$match: { "": … }` is a query that runs but matches nothing.
   */
  aggrAlias?: Map<string, string>;
}

/** An aggregate function's signature, enough to recognise that `COUNT(*)` in `HAVING` is the
 *  `COUNT(*)` in `SELECT`. */
function aggrKey(fn: Node): string {
  const name = String(fn.name ?? "").toUpperCase();
  const arg = isNode(fn.args) && isNode(fn.args.expr) ? fn.args.expr : null;
  const inner = !arg || arg.type === "star" ? "*" : columnName(arg);
  return `${name}(${inner})`;
}

function warn(ctx: Context, code: Warning["code"], fragment: string) {
  if (!ctx.warnings.some((w) => w.code === code && w.fragment === fragment)) {
    ctx.warnings.push({ code, fragment });
  }
}

function buildFilter(node: unknown, ctx: Context): unknown {
  if (!isNode(node)) return {};

  if (node.type === "function" && functionName(node) === "NOT") {
    const args = isNode(node.args) && Array.isArray(node.args.value) ? node.args.value : [];
    // Mongo's `$not` only stands inside a field; negating a whole condition is `$nor`.
    return { $nor: args.map((arg) => buildFilter(arg, ctx)) };
  }

  if (node.type !== "binary_expr") return {};

  const operator = String(node.operator ?? "");
  const left = node.left;
  const right = node.right;

  if (operator === "AND") return { $and: [buildFilter(left, ctx), buildFilter(right, ctx)] };
  if (operator === "OR") return { $or: [buildFilter(left, ctx), buildFilter(right, ctx)] };

  if (!isNode(left) || !isNode(right)) return {};
  // In `HAVING`, the left side is an aggregate function and its name is the one `$group` just
  // gave it.
  const field =
    left.type === "aggr_func" ? (ctx.aggrAlias?.get(aggrKey(left)) ?? "") : columnName(left);

  if (operator === "LIKE" || operator === "NOT LIKE" || operator === "ILIKE") {
    // MySQL is case-insensitive under the default collation; PostgreSQL is not, and `ILIKE` is how
    // it says "do not distinguish". This is where the dialect picker has a real effect.
    const insensitive = ctx.dialect === "mysql" || operator === "ILIKE";
    const expr = {
      $regex: likeToRegex(String(right.value ?? "")),
      ...(insensitive ? { $options: "i" } : {}),
    };
    return { [field]: operator === "NOT LIKE" ? { $not: expr } : expr };
  }

  if (operator === "IS") warn(ctx, "isNull", field);

  if (operator === "=" && right.type === "single_quote_string") {
    const value = String(right.value ?? "");
    if (field === "_id" && OBJECT_ID.test(value)) warn(ctx, "objectId", value);
    else if (/^\d+$/.test(value)) warn(ctx, "type", field);
  }

  switch (operator) {
    // The short form, no `$eq`: that is what people write by hand, and it reads better.
    case "=":
      return { [field]: literal(right) };
    case "!=":
    case "<>":
      return { [field]: { $ne: literal(right) } };
    case ">":
      return { [field]: { $gt: literal(right) } };
    case ">=":
      return { [field]: { $gte: literal(right) } };
    case "<":
      return { [field]: { $lt: literal(right) } };
    case "<=":
      return { [field]: { $lte: literal(right) } };
    case "IS":
      return { [field]: null };
    case "IS NOT":
      return { [field]: { $ne: null } };
    case "IN":
    case "NOT IN": {
      const values = Array.isArray(right.value) ? right.value.filter(isNode).map(literal) : [];
      return { [field]: operator === "IN" ? { $in: values } : { $nin: values } };
    }
    case "BETWEEN": {
      const pair = Array.isArray(right.value) ? right.value.filter(isNode).map(literal) : [];
      return { [field]: { $gte: pair[0], $lte: pair[1] } };
    }
    default:
      return {};
  }
}

/** `null` for `SELECT *` — with no projection, `find` takes no second argument. */
function buildProjection(columns: unknown): Record<string, unknown> | null {
  if (!Array.isArray(columns)) return null;

  const projection: Record<string, unknown> = {};
  let named = false;

  for (const entry of columns) {
    if (!isNode(entry) || !isNode(entry.expr)) continue;
    if (entry.expr.type === "column_ref" && columnName(entry.expr) === "*") return null;
    if (entry.expr.type !== "column_ref") continue;

    const name = columnName(entry.expr);
    const alias = typeof entry.as === "string" ? entry.as : null;
    projection[alias ?? name] = alias ? `$${name}` : 1;
    named = true;
  }

  if (!named) return null;
  // `_id` comes along by default in Mongo, while `SELECT name` does not mean "name and _id".
  if (!("_id" in projection)) projection._id = 0;
  return projection;
}

function buildSort(orderby: unknown): Record<string, number> | null {
  if (!Array.isArray(orderby) || orderby.length === 0) return null;

  const sort: Record<string, number> = {};
  for (const entry of orderby) {
    if (!isNode(entry) || !isNode(entry.expr)) continue;
    sort[columnName(entry.expr)] = String(entry.type).toUpperCase() === "DESC" ? -1 : 1;
  }
  return Object.keys(sort).length > 0 ? sort : null;
}

/** `[limit, skip]`, each `null` when the statement does not mention it. */
function buildLimit(limit: unknown): [number | null, number | null] {
  if (!isNode(limit) || !Array.isArray(limit.value) || limit.value.length === 0) return [null, null];
  const values = limit.value.filter(isNode).map((node) => Number(node.value));
  const hasOffset = String(limit.seperator ?? "") === "offset" && values.length > 1;
  return [values[0] ?? null, hasOffset ? values[1] : null];
}

/** A column's `aggr_func` node, if that column is an aggregate function. */
function aggregateOf(entry: unknown): Node | null {
  if (!isNode(entry) || !isNode(entry.expr)) return null;
  return entry.expr.type === "aggr_func" ? entry.expr : null;
}

/** Whether this statement needs a pipeline, or `find()` is enough. */
function needsPipeline(ast: Node): boolean {
  if (ast.groupby || ast.having) return true;
  if (typeof ast.distinct === "string" && ast.distinct.toUpperCase() === "DISTINCT") return true;
  return Array.isArray(ast.columns) && ast.columns.some((entry) => aggregateOf(entry) !== null);
}

/** The `$group` expression for an aggregate function. */
function accumulator(fn: Node): unknown {
  const name = String(fn.name ?? "").toUpperCase();
  const arg = isNode(fn.args) && isNode(fn.args.expr) ? fn.args.expr : null;

  if (name === "COUNT") {
    if (!arg || arg.type === "star") return { $sum: 1 };
    // SQL's `COUNT(col)` skips NULLs. Translating it to `$sum: 1` is the kind of bug that runs
    // smoothly and gives the wrong number.
    return { $sum: { $cond: [{ $eq: [`$${columnName(arg)}`, null] }, 0, 1] } };
  }

  const field = arg ? `$${columnName(arg)}` : null;
  switch (name) {
    case "SUM":
      return { $sum: field };
    case "AVG":
      return { $avg: field };
    case "MIN":
      return { $min: field };
    case "MAX":
      return { $max: field };
    default:
      return { $sum: 1 };
  }
}

/** The names of the `GROUP BY` columns. */
function groupKeys(groupby: unknown): string[] {
  if (!isNode(groupby) || !Array.isArray(groupby.columns)) return [];
  return groupby.columns.filter(isNode).map(columnName).filter(Boolean);
}

/**
 * The stages of an aggregation pipeline.
 *
 * The order is fixed, and the two `$match`es in two different positions **are** exactly where
 * `WHERE` differs from `HAVING`: `WHERE` filters before grouping, `HAVING` filters the grouped
 * result. Put them in the wrong place and the query still runs and still gives numbers — just
 * different numbers.
 */
function buildPipeline(ast: Node, ctx: Context): unknown[] {
  const stages: unknown[] = [];

  if (ast.where) stages.push({ $match: buildFilter(ast.where, ctx) });

  const keys = groupKeys(ast.groupby);
  const columns = Array.isArray(ast.columns) ? ast.columns : [];
  const distinct = typeof ast.distinct === "string" && ast.distinct.toUpperCase() === "DISTINCT";

  /* The grouping key: the GROUP BY columns, or — with DISTINCT — the selected columns themselves.
     An aggregate function without GROUP BY groups the whole table, and the key is `null`. */
  const plainColumns = columns
    .filter((entry) => aggregateOf(entry) === null)
    .map((entry) => (isNode(entry) && isNode(entry.expr) ? columnName(entry.expr) : ""))
    .filter((name) => name !== "" && name !== "*");

  const keyFields = keys.length > 0 ? keys : distinct ? plainColumns : [];
  const id =
    keyFields.length === 0
      ? null
      : keyFields.length === 1
        ? `$${keyFields[0]}`
        : Object.fromEntries(keyFields.map((name) => [name, `$${name}`]));

  const group: Record<string, unknown> = { _id: id };
  const aggrAlias = new Map<string, string>();
  for (const entry of columns) {
    const fn = aggregateOf(entry);
    if (!fn || !isNode(entry)) continue;
    const alias = typeof entry.as === "string" ? entry.as : String(fn.name ?? "value").toLowerCase();
    group[alias] = accumulator(fn);
    aggrAlias.set(aggrKey(fn), alias);
  }

  /* An aggregate function appearing only in `HAVING` still has to be grouped, otherwise there is
     nothing to filter. It goes into `$group` under a helper name and is dropped by `$project`
     afterwards — the user did not ask for it, so it should not be in the result. */
  const helpers: string[] = [];
  const findAggregates = (value: unknown) => {
    if (Array.isArray(value)) return value.forEach(findAggregates);
    if (!isNode(value)) return;
    if (value.type === "aggr_func") {
      const key = aggrKey(value);
      if (!aggrAlias.has(key)) {
        const name = `_having${helpers.length}`;
        aggrAlias.set(key, name);
        group[name] = accumulator(value);
        helpers.push(name);
      }
      return;
    }
    Object.values(value).forEach(findAggregates);
  };
  if (ast.having) findAggregates(ast.having);

  stages.push({ $group: group });

  if (ast.having) stages.push({ $match: buildFilter(ast.having, { ...ctx, aggrAlias }) });

  /* `$project` brings the grouping key from `_id` back to the column names as in `SELECT` — without
     it the result carries a field named `_id` the SQL statement never mentioned. It is also where
     the helper names above disappear: `$project` lists what is kept, so not naming them is enough
     to drop them. */
  if (keyFields.length > 0 || helpers.length > 0) {
    const project: Record<string, unknown> = { _id: 0 };
    for (const name of keyFields) {
      project[name] = keyFields.length === 1 ? "$_id" : `$_id.${name}`;
    }
    for (const key of Object.keys(group)) {
      if (key !== "_id" && !helpers.includes(key)) project[key] = 1;
    }
    stages.push({ $project: project });
  }

  const sort = buildSort(ast.orderby);
  if (sort) stages.push({ $sort: sort });

  const [limit, skip] = buildLimit(ast.limit);
  if (skip !== null) stages.push({ $skip: skip });
  if (limit !== null) stages.push({ $limit: limit });

  return stages;
}

const json = (value: unknown) => JSON.stringify(value, null, 2);

function formatFind(
  collection: string,
  filter: unknown,
  projection: Record<string, unknown> | null,
  sort: Record<string, number> | null,
  skip: number | null,
  limit: number | null,
): string {
  let out = projection
    ? `db.${collection}.find(${json(filter)}, ${json(projection)})`
    : `db.${collection}.find(${json(filter)})`;
  if (sort) out += `.sort(${json(sort)})`;
  if (skip !== null) out += `.skip(${skip})`;
  if (limit !== null) out += `.limit(${limit})`;
  return out;
}

/**
 * An SQL statement as a MongoDB query, or as the reasons why not.
 *
 * Async because the parser is loaded with a dynamic `import()`: it is the only heavy thing in the
 * tool, and it only needs to be present when someone actually presses translate.
 */
export async function translate(sql: string, dialect: Dialect): Promise<Translation> {
  const trimmed = sql.trim();
  if (trimmed === "") return { ok: false, unsupported: [{ code: "parse", fragment: "" }] };

  /* A build for one dialect, not the main entry.
     `node-sql-parser` bundles a dozen-odd dialects into one 2.5 MB file; `build/mysql` is 276 kB
     and `build/postgresql` is 308 kB. The dialect is known right here, so there is no reason to
     load the rest — and two separate `import()` branches written like this are how the bundler can
     split them into two chunks. */
  const { Parser } =
    dialect === "mysql"
      ? await import("node-sql-parser/build/mysql")
      : await import("node-sql-parser/build/postgresql");

  let parsed: unknown;
  try {
    parsed = new Parser().astify(trimmed, { database: dialect });
  } catch {
    return { ok: false, unsupported: [{ code: "parse", fragment: trimmed }] };
  }

  if (Array.isArray(parsed)) {
    if (parsed.length > 1) return { ok: false, unsupported: [{ code: "multi", fragment: trimmed }] };
    parsed = parsed[0];
  }
  if (!isNode(parsed)) return { ok: false, unsupported: [{ code: "parse", fragment: trimmed }] };

  const ast = parsed;
  if (ast.type !== "select") {
    return { ok: false, unsupported: [{ code: "dml", fragment: String(ast.type).toUpperCase() }] };
  }

  const unsupported = collectUnsupported(ast, trimmed);
  if (unsupported.length > 0) return { ok: false, unsupported };

  const from = Array.isArray(ast.from) ? ast.from : [];
  const first = from[0];
  const collection = isNode(first) && typeof first.table === "string" ? first.table : "collection";

  const ctx: Context = { dialect, warnings: [] };

  const selectsStar =
    Array.isArray(ast.columns) &&
    ast.columns.some(
      (entry) => isNode(entry) && isNode(entry.expr) && columnName(entry.expr) === "*",
    );
  if (selectsStar && ast.groupby) warn(ctx, "starWithGroupBy", "*");

  if (needsPipeline(ast)) {
    return {
      ok: true,
      output: `db.${collection}.aggregate(${json(buildPipeline(ast, ctx))})`,
      warnings: ctx.warnings,
    };
  }

  const [limit, skip] = buildLimit(ast.limit);

  return {
    ok: true,
    output: formatFind(
      collection,
      buildFilter(ast.where, ctx),
      buildProjection(ast.columns),
      buildSort(ast.orderby),
      skip,
      limit,
    ),
    warnings: ctx.warnings,
  };
}
