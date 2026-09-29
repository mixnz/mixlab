/**
 * Masks sensitive data in a set of flat objects, to share data samples without exposing PII.
 *
 * This is simple masking for exporting sample data — **not** anonymisation up to security/GDPR
 * standards. Everything runs in this process and is sent nowhere; `hash` is a non-cryptographic
 * hash (FNV-1a), reversible if the set of original values is small (e.g. a specific phone number) —
 * enough to keep things consistent across rows/tables, not enough to hide a truly secret value.
 */

export type MaskKind = "none" | "redact" | "partial" | "hash";

export const MASK_KINDS: MaskKind[] = ["none", "redact", "partial", "hash"];

/** Only decides *how* `partial` formats — not a category shown separately for the user to pick;
 *  the user only picks a `MaskKind` per field. */
export type Shape = "email" | "phone" | "name" | "card" | "idNumber" | "dob" | "address" | "generic";

export interface FieldMaskSpec {
  name: string;
  shape: Shape;
  kind: MaskKind;
}

/** Guesses the shape from the column name. A column matching nothing falls into `generic`, and
 *  that shape defaults to kind `none` — silently skipping an unknown column is safer than wrongly
 *  masking a column like `id`/`created_at`. */
export function detectShape(fieldName: string): Shape {
  const key = fieldName.toLowerCase();
  if (/email/.test(key)) return "email";
  if (/phone|sdt|dienthoai|mobile/.test(key)) return "phone";
  if (/card|the|ccnum|creditcard/.test(key)) return "card";
  if (/cmnd|cccd|idnumber|id_number|ssn|passport/.test(key)) return "idNumber";
  if (/ngay.?sinh|dob|birth/.test(key)) return "dob";
  if (/address|diachi/.test(key)) return "address";
  if (/name|ten|hoten|ho_ten/.test(key)) return "name";
  return "generic";
}

export function defaultKindForShape(shape: Shape): MaskKind {
  switch (shape) {
    case "email":
    case "phone":
    case "name":
    case "card":
    case "idNumber":
      return "partial";
    case "dob":
    case "address":
      return "redact";
    case "generic":
      return "none";
  }
}

/** Columns in order of first appearance, merged across every row — a first row missing a column
 *  does not hide it. */
export function detectFieldSpecs(rows: Record<string, unknown>[]): FieldMaskSpec[] {
  const columns: string[] = [];
  for (const row of rows) {
    for (const key of Object.keys(row)) {
      if (!columns.includes(key)) columns.push(key);
    }
  }
  return columns.map((name) => {
    const shape = detectShape(name);
    return { name, shape, kind: defaultKindForShape(shape) };
  });
}

function fnv1a(text: string): string {
  let hash = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    hash ^= text.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return (hash >>> 0).toString(16).padStart(8, "0");
}

/** Keeps the last N digits and replaces what comes before with asterisks — used for
 *  phone/card/idNumber, which differ only in how many digits are kept. */
function maskDigitsKeepingLast(text: string, keep: number): string {
  const digits = text.replace(/\D/g, "");
  if (digits.length <= keep) return "*".repeat(text.length);
  return "*".repeat(digits.length - keep) + digits.slice(-keep);
}

function maskGeneric(text: string): string {
  if (text.length <= 2) return "*".repeat(text.length);
  return text.slice(0, 1) + "*".repeat(text.length - 2) + text.slice(-1);
}

function maskPartial(text: string, shape: Shape): string {
  switch (shape) {
    case "email": {
      const at = text.indexOf("@");
      if (at <= 0) return maskGeneric(text);
      const local = text.slice(0, at);
      const domain = text.slice(at);
      const stars = "*".repeat(Math.max(local.length - 1, 3));
      return `${local.slice(0, 1)}${stars}${domain}`;
    }
    case "phone":
      return maskDigitsKeepingLast(text, 2);
    case "card":
    case "idNumber":
      return maskDigitsKeepingLast(text, 4);
    case "name":
      return text
        .trim()
        .split(/\s+/)
        .map((part) => `${part.slice(0, 1)}***`)
        .join(" ");
    case "dob":
    case "address":
    case "generic":
      return maskGeneric(text);
  }
}

/**
 * Applies a `MaskKind` to a value. An empty value (`null`/`undefined`/`""`) has nothing to hide, so
 * it passes through intact whatever the `kind` — masking a cell that was already empty only makes
 * it look as if there were data there.
 *
 * `none` is the only kind that keeps the original type; the other three always return a string,
 * even when the input is a number or a boolean — a phone number may be stored as a number, and its
 * masked result is no longer a number.
 */
export function maskValue(value: unknown, kind: MaskKind, shape: Shape): unknown {
  if (kind === "none" || value === null || value === undefined || value === "") return value;
  const text = String(value);
  switch (kind) {
    case "redact":
      return "***";
    case "hash":
      return `h_${fnv1a(text)}`;
    case "partial":
      return maskPartial(text, shape);
  }
}

export function maskRows(
  rows: Record<string, unknown>[],
  specs: FieldMaskSpec[],
): Record<string, unknown>[] {
  const bySpec = new Map(specs.map((spec) => [spec.name, spec]));
  return rows.map((row) => {
    const out: Record<string, unknown> = {};
    for (const [key, value] of Object.entries(row)) {
      const spec = bySpec.get(key);
      out[key] = spec ? maskValue(value, spec.kind, spec.shape) : value;
    }
    return out;
  });
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isFlat(row: Record<string, unknown>): boolean {
  return Object.values(row).every((value) => !isRecord(value) && !Array.isArray(value));
}

/**
 * Normalises parsed JSON into an array of flat objects, or `null` if it cannot be read — even when
 * only one field is nested. Silently skipping nested fields instead of reporting an error would
 * make the user think every column had been considered for masking, when some were never looked
 * at.
 */
export function parseFlatRows(parsed: unknown): Record<string, unknown>[] | null {
  const rows = Array.isArray(parsed) ? parsed : isRecord(parsed) ? [parsed] : null;
  if (rows === null || rows.length === 0) return null;
  if (!rows.every(isRecord)) return null;
  const records = rows as Record<string, unknown>[];
  return records.every(isFlat) ? records : null;
}
