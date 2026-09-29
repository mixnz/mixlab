/**
 * CSV per RFC 4180, both directions, hand-written.
 *
 * The hard part is exactly the double quotes: inside a quoted field, separators, newlines and even
 * doubled quotes `""` can all sit. That is why there is no `split(",")` here.
 *
 * Shared by `convert` and `mask` — both need to read/write CSV, and RFC 4180 is tricky enough not
 * to be worth copying a second time.
 */

/** Reads CSV into a grid of strings. **No type guessing** — `007` is `"007"`, not `7`. */
export function parseCsvRows(text: string, delimiter: string): string[][] {
  const rows: string[][] = [];
  let row: string[] = [];
  let field = "";
  let quoted = false;
  let started = false;
  let i = 0;

  const endField = (): void => {
    row.push(field);
    field = "";
  };

  const endRow = (): void => {
    endField();
    rows.push(row);
    row = [];
    started = false;
  };

  while (i < text.length) {
    const ch = text[i]!;
    if (quoted) {
      if (ch === '"') {
        if (text[i + 1] === '"') {
          field += '"';
          i += 2;
          continue;
        }
        quoted = false;
        i += 1;
        continue;
      }
      field += ch;
      i += 1;
      continue;
    }
    if (ch === '"' && field === "") {
      quoted = true;
      started = true;
      i += 1;
    } else if (ch === delimiter) {
      endField();
      started = true;
      i += 1;
    } else if (ch === "\r") {
      i += 1;
    } else if (ch === "\n") {
      endRow();
      i += 1;
    } else {
      field += ch;
      started = true;
      i += 1;
    }
  }

  // No extra row when the file ends with a newline.
  if (started || field !== "" || row.length > 0) endRow();
  return rows;
}

/** The first row is the column names. Entirely empty rows are dropped — they are blank lines, not
 *  empty records. */
export function rowsToObjects(rows: string[][]): Record<string, string>[] {
  const header = rows[0];
  if (!header) return [];
  return rows
    .slice(1)
    .filter((row) => row.some((cell) => cell !== ""))
    .map((row) => {
      const record: Record<string, string> = {};
      header.forEach((name, index) => {
        record[name] = row[index] ?? "";
      });
      return record;
    });
}

export function toCsv(
  values: Record<string, unknown>[],
  delimiter: string,
  header: boolean,
): string {
  const columns: string[] = [];
  for (const row of values) {
    for (const key of Object.keys(row)) {
      if (!columns.includes(key)) columns.push(key);
    }
  }

  const cell = (value: unknown): string => {
    if (value === null || value === undefined) return "";
    const text = typeof value === "object" ? JSON.stringify(value) : String(value);
    const needsQuotes = text.includes(delimiter) || /["\n\r]/.test(text);
    return needsQuotes ? `"${text.replace(/"/g, '""')}"` : text;
  };

  const lines = values.map((row) => columns.map((name) => cell(row[name])).join(delimiter));
  return header ? [columns.map(cell).join(delimiter), ...lines].join("\n") : lines.join("\n");
}
