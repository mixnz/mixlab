/**
 * Rows of the grid as text somebody can paste somewhere else: the SQL that would put them back
 * into a table, and the TSV, CSV and JSON a spreadsheet or an editor reads.
 *
 * Kept out of the component and free of React so the escaping — the part that is easy to get
 * quietly wrong and impossible to see wrong on screen — can be tested on its own.
 */

// Renamed on the way in because this file exports a `csvText` and a `jsonText` of its own — same
// names, and rows of a different shape.
import {
  csvText as csvGridText,
  jsonText as jsonGridText,
  tsvText,
} from "../../../../core/gridText";

/** A name as it goes into a statement: backticks, with any backtick inside doubled. That is MySQL's
 *  own way of writing a name that holds one, and a table or column can hold one. */
export function quoteIdentifier(name: string): string {
  return `\`${name.replace(/`/g, "``")}\``;
}

/**
 * The characters that cannot stand for themselves inside a quoted string, and what MySQL reads in
 * their place. `\Z` is Ctrl+Z, which Windows takes as end-of-file when a script is piped into a
 * client — so a value holding one would cut the script short there.
 *
 * These are MySQL's default escapes. A server running under `NO_BACKSLASH_ESCAPES` reads a
 * backslash as an ordinary character and would take such a value back in changed; that mode is one
 * this app never sets, and the statements here are written to be pasted into MixLab's own Query tab
 * or a `mysql` client, where the default is what applies.
 */
const ESCAPED: Record<string, string> = {
  "\\": "\\\\",
  "'": "\\'",
  "\n": "\\n",
  "\r": "\\r",
  "\0": "\\0",
  "\x1a": "\\Z",
};

function quoteString(text: string): string {
  return `'${text.replace(/[\\'\n\r\0\x1a]/g, (ch) => ESCAPED[ch])}'`;
}

/**
 * One cell as the literal that would write it back into the column it came from.
 *
 * Numbers and booleans go in bare — quoted they would still land in a numeric column, but the
 * statement is meant to be read as well as run. Anything the driver handed over as an object came
 * out of a JSON column, so it goes back as its JSON text. A number that is not finite has no literal
 * at all in SQL and becomes NULL rather than the word `NaN`, which would be read as a column name.
 */
export function sqlLiteral(value: unknown): string {
  if (value === null || value === undefined) return "NULL";
  if (typeof value === "number") return Number.isFinite(value) ? String(value) : "NULL";
  if (typeof value === "bigint") return String(value);
  if (typeof value === "boolean") return value ? "1" : "0";
  if (typeof value === "object") return quoteString(JSON.stringify(value));
  return quoteString(String(value));
}

/**
 * One cell of a binary column as the literal that would write the same bytes back.
 *
 * A binary column arrives base64-encoded — bytes have no other way through JSON — so what sits in
 * the row is an encoding of the value rather than the value. Quoted as it stands it would insert
 * the encoding itself: a `BINARY(16)` UUID would go back in as the first sixteen ASCII characters
 * of its own base64, wrong in a way nothing on screen or in the statement says. `FROM_BASE64` is
 * what undoes the encoding, on the server, where the bytes are wanted.
 *
 * Anything that is not a string never went through that encoding and is written as it always was.
 */
function binaryLiteral(value: unknown): string {
  if (typeof value !== "string") return sqlLiteral(value);
  return `FROM_BASE64(${quoteString(value)})`;
}

/**
 * How many rows one statement carries before the next one begins.
 *
 * A batched insert is one round trip and one pass over the index, which is the whole reason the
 * rows go into a single `VALUES` rather than a statement each. Past a few hundred rows that
 * stops being the trade it was: the statement is a single transaction the server holds open, one
 * bad row rolls the whole batch back, and what comes back on failure names a row number in a list
 * nobody wants to count through. A hundred is small enough to read and re-run, large enough that
 * the round trips are no longer what the copy costs.
 */
const ROWS_PER_STATEMENT = 100;

/**
 * How long one statement is allowed to get, whatever the row count says.
 *
 * A row can be a megabyte of JSON or a blob written out as text, and a hundred of those is a
 * statement the server hangs up on: `max_allowed_packet` is 4MB on MySQL 5.7 and 64MB on 8, and
 * what a refusal reads as is "MySQL server has gone away" rather than anything about size. Half a
 * megabyte clears the lower of the two several times over.
 */
const MAX_STATEMENT_LENGTH = 512 * 1024;

/**
 * The `INSERT` statements that would put `rows` into `table`: one statement holding every row, one
 * row per line, split into further statements only where the batch grows past what is comfortable
 * to run — {@link ROWS_PER_STATEMENT} rows, or {@link MAX_STATEMENT_LENGTH} of text.
 *
 * A line per row rather than one long line: what is copied here is nearly always read before it is
 * run, and a row picked out of forty is then a line to delete rather than a bracket to find.
 *
 * `omit` leaves a column out of both the name list and the values. That is how the second of the
 * two copies works: without the AUTO_INCREMENT column the server numbers each row itself, so the
 * statement inserts new rows rather than colliding with the ones they were copied from.
 *
 * `binary` names the columns whose values reached here base64-encoded, and which therefore go back
 * through {@link binaryLiteral} rather than being quoted as the text they look like.
 */
export function insertStatements(
  table: string,
  columns: string[],
  rows: Record<string, unknown>[],
  omit: string | null = null,
  binary: ReadonlySet<string> = new Set(),
): string {
  const used = omit === null ? columns : columns.filter((c) => c !== omit);
  const head = `INSERT INTO ${quoteIdentifier(table)} (${used.map(quoteIdentifier).join(", ")}) VALUES`;
  const statements: string[] = [];
  let batch: string[] = [];
  let length = head.length;

  function flush() {
    if (batch.length === 0) return;
    statements.push(`${head}\n${batch.join(",\n")};`);
    batch = [];
    length = head.length;
  }

  for (const row of rows) {
    const literals = used.map((c) => (binary.has(c) ? binaryLiteral(row[c]) : sqlLiteral(row[c])));
    const values = `  (${literals.join(", ")})`;
    // The row that would take the statement past either limit opens the next one instead — but
    // never on its own account: a single row longer than the cap still has to go somewhere, and
    // splitting before an empty batch would only produce a statement with no rows in it.
    const full = batch.length >= ROWS_PER_STATEMENT || length + values.length > MAX_STATEMENT_LENGTH;
    if (batch.length > 0 && full) flush();
    batch.push(values);
    // The comma and the newline this row costs once another follows it.
    length += values.length + 2;
  }
  flush();
  // A blank line between them, so where one statement ends and the next begins is visible without
  // reading to the semicolon.
  return statements.join("\n\n");
}

/**
 * The three copy formats, on rows keyed by column name.
 *
 * The work is in `core/gridText.ts`, which speaks positional rows — the form that survives a result
 * naming the same column twice. This grid never has that problem, since it is showing one table and
 * a table's columns are distinct, so the map down is a plain lookup per column.
 */
function positional(columns: string[], rows: Record<string, unknown>[]): unknown[][] {
  return rows.map((row) => columns.map((c) => row[c]));
}

/** `rows` as the tab-separated text a spreadsheet pastes into cells. */
export function spreadsheetText(columns: string[], rows: Record<string, unknown>[]): string {
  return tsvText(columns, positional(columns, rows));
}

/** `rows` as CSV, for saving to a file rather than pasting into an open sheet. */
export function csvText(columns: string[], rows: Record<string, unknown>[]): string {
  return csvGridText(columns, positional(columns, rows));
}

/** `rows` as a JSON array of objects — what goes into a request body or a fixture rather than into
 *  a spreadsheet. */
export function jsonText(columns: string[], rows: Record<string, unknown>[]): string {
  return jsonGridText(columns, positional(columns, rows));
}
