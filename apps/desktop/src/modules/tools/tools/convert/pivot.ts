import { parseCsvRows, rowsToObjects, toCsv } from "../shared/csv";
import { toInsert, type SqlDialect } from "./insert";

/**
 * The Convert tool's pivot: every input format becomes a JS value, and every output format is
 * produced from that value. Three readers plus four writers, not twelve cross-translating
 * functions.
 *
 * The price is losing whatever the pivot cannot carry — YAML comments, and the precision of JSON
 * numbers. The latter is reported through `warnings`; the Format tool avoids it entirely by not
 * going through here.
 */

export type ReadFormat = "json" | "yaml" | "csv";
export type WriteFormat = "json" | "yaml" | "csv" | "insert";

export interface ConvertOptions {
  delimiter: string;
  header: boolean;
  table: string;
  dialect: SqlDialect;
  multiRow: boolean;
}

export type ConvertFailure =
  | { reason: "empty" | "same" | "needsRows" }
  | { reason: "parse"; detail: string };

export type ConvertResult =
  | { ok: true; output: string; warnings: "precision"[] }
  | { ok: false; failure: ConvertFailure };

/** Integers of 16 digits or more do not survive `JSON.parse`. */
const LONG_INTEGER = /(^|[^\w.])-?\d{16,}([^\d.]|$)/;

/** CSV and INSERT need an array of objects whose every value is a single cell. */
function isFlatObjectArray(value: unknown): value is Record<string, unknown>[] {
  return (
    Array.isArray(value) &&
    value.length > 0 &&
    value.every(
      (row) =>
        typeof row === "object" &&
        row !== null &&
        !Array.isArray(row) &&
        Object.values(row as Record<string, unknown>).every(
          (cell) => cell === null || typeof cell !== "object",
        ),
    )
  );
}

async function read(text: string, from: ReadFormat, options: ConvertOptions): Promise<unknown> {
  if (from === "json") return JSON.parse(text);
  if (from === "yaml") {
    // Loaded on first use, not when the tab opens — the same way `node-sql-parser` is loaded in
    // stage 2.
    const yaml = await import("js-yaml");
    return yaml.load(text);
  }
  const rows = parseCsvRows(text, options.delimiter);
  return options.header ? rowsToObjects(rows) : rows;
}

async function write(value: unknown, to: WriteFormat, options: ConvertOptions): Promise<string> {
  if (to === "json") return JSON.stringify(value, null, 2);
  if (to === "yaml") {
    const yaml = await import("js-yaml");
    return yaml.dump(value, { noRefs: true, lineWidth: -1 });
  }
  const rows = value as Record<string, unknown>[];
  if (to === "csv") return toCsv(rows, options.delimiter, options.header);
  return toInsert(rows, {
    table: options.table,
    dialect: options.dialect,
    multiRow: options.multiRow,
  });
}

export async function convertData(
  text: string,
  from: ReadFormat,
  to: WriteFormat,
  options: ConvertOptions,
): Promise<ConvertResult> {
  if (text.trim() === "") return { ok: false, failure: { reason: "empty" } };
  if ((from as string) === (to as string)) return { ok: false, failure: { reason: "same" } };

  let value: unknown;
  try {
    value = await read(text, from, options);
  } catch (error) {
    return {
      ok: false,
      failure: { reason: "parse", detail: error instanceof Error ? error.message : String(error) },
    };
  }

  // No partial output: a CSV table missing its nested columns looks exactly like a correct one.
  if ((to === "csv" || to === "insert") && !isFlatObjectArray(value)) {
    return { ok: false, failure: { reason: "needsRows" } };
  }

  const warnings: "precision"[] = from === "json" && LONG_INTEGER.test(text) ? ["precision"] : [];
  return { ok: true, output: await write(value, to, options), warnings };
}
