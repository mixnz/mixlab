export type FormatKind = "json" | "xml" | "sql";

/**
 * Guesses the format from the first non-whitespace character.
 *
 * The Panel **says** what it guessed, just as the Timestamp tool does with units: a silent guess
 * that is wrong leaves the user no way of knowing. `null` means there is nothing to guess from.
 */
export function detectFormat(text: string): FormatKind | null {
  const head = text.trimStart();
  if (head === "") return null;
  const ch = head[0]!;
  if (ch === "<") return "xml";
  if (ch === "{" || ch === "[") return "json";
  return "sql";
}
