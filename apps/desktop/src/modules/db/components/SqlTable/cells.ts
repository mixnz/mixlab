import type { Sort } from "./request";

/** The header click cycle: unsorted → descending → ascending → unsorted. */
export function nextSort(current: Sort | null, column: string): Sort | null {
  if (current?.column !== column) return { column, desc: true };
  return current.desc ? { column, desc: false } : null;
}

export function normalizeCellValue(raw: unknown): string | null {
  if (raw === null || raw === undefined) return null;
  return typeof raw === "object" ? JSON.stringify(raw) : String(raw);
}

export function displayValue(raw: unknown): string {
  return normalizeCellValue(raw) ?? "NULL";
}

export function isMultilineType(type: string | undefined): boolean {
  if (!type) return false;
  const t = type.toLowerCase();
  return t.includes("text") || t.includes("json") || t.includes("blob");
}
