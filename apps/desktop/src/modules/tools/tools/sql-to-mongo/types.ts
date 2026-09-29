export type Dialect = "mysql" | "postgresql";

/** A clause that cannot be translated. It **replaces** the output; it does not come with it. */
export interface Unsupported {
  code:
    | "join"
    | "subquery"
    | "union"
    | "cte"
    | "window"
    | "dml"
    | "case"
    | "function"
    | "multi"
    | "parse";
  /** The SQL fragment responsible, so the Panel can point at the right place instead of just
   *  saying "not supported". */
  fragment: string;
}

/** A place that can be translated but where Mongo's semantics do not match SQL's. It **comes
 *  with** the output. */
export interface Warning {
  code: "isNull" | "type" | "objectId" | "starWithGroupBy";
  /** The field or SQL fragment the warning is about. */
  fragment: string;
}

/**
 * The result of one translation.
 *
 * Two branches, not one object with both `output` and `unsupported`: **never output a partial
 * result**. A query that has lost its `HAVING` looks exactly like a correct query, and someone will
 * run it in production.
 *
 * `Unsupported` and `Warning` both carry a `fragment` but live in two different branches, and that
 * is deliberate: the first *replaces* the output, the second *comes with* the output. Putting both
 * in one list is the first step towards one day outputting a partial result.
 */
export type Translation =
  | { ok: true; output: string; warnings: Warning[] }
  | { ok: false; unsupported: Unsupported[] };
