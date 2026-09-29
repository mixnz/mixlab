/**
 * Generates `INSERT` statements from an array of objects.
 *
 * The output is for **reading and pasting by hand**, not a replacement for parameterised queries —
 * the tool does not know where the data came from. The Panel says so in a line under the result
 * field.
 */

export type SqlDialect = "mysql" | "postgres";

export interface InsertOptions {
  table: string;
  dialect: SqlDialect;
  /** One statement with many `VALUES` rows, instead of one statement per row. */
  multiRow: boolean;
}

function quoteIdent(name: string, dialect: SqlDialect): string {
  return dialect === "mysql" ? `\`${name.replace(/`/g, "``")}\`` : `"${name.replace(/"/g, '""')}"`;
}

/**
 * Wraps a string as an SQL literal.
 *
 * **The two dialects escape differently, and this is a mistake that runs smoothly.** MySQL treats
 * `\` as an escape character in strings (by default, with `NO_BACKSLASH_ESCAPES` off), so it has to
 * be doubled — skip that and `C:\new\table` lands in the DB as a newline and a tab. PostgreSQL with
 * `standard_conforming_strings` on (the default since 9.1) does not, and doubling there writes an
 * extra `\` into the data.
 */
function quoteText(value: string, dialect: SqlDialect): string {
  const escaped = dialect === "mysql" ? value.replace(/\\/g, "\\\\") : value;
  return `'${escaped.replace(/'/g, "''")}'`;
}

function literal(value: unknown, dialect: SqlDialect): string {
  if (value === null || value === undefined) return "NULL";
  if (typeof value === "number") return Number.isFinite(value) ? String(value) : "NULL";
  if (typeof value === "boolean") return value ? "TRUE" : "FALSE";
  if (typeof value === "object") return quoteText(JSON.stringify(value), dialect);
  return quoteText(String(value), dialect);
}

export function toInsert(rows: Record<string, unknown>[], options: InsertOptions): string {
  if (rows.length === 0) return "";

  const columns: string[] = [];
  for (const row of rows) {
    for (const key of Object.keys(row)) {
      if (!columns.includes(key)) columns.push(key);
    }
  }

  const head = `INSERT INTO ${quoteIdent(options.table, options.dialect)} (${columns
    .map((name) => quoteIdent(name, options.dialect))
    .join(", ")})`;
  const tuples = rows.map(
    (row) => `(${columns.map((name) => literal(row[name], options.dialect)).join(", ")})`,
  );

  if (options.multiRow) {
    return `${head} VALUES\n${tuples.map((tuple) => `  ${tuple}`).join(",\n")};`;
  }
  return tuples.map((tuple) => `${head} VALUES ${tuple};`).join("\n");
}
