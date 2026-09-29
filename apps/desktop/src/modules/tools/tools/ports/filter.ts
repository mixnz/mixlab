import type { ListeningPort } from "./api";

/**
 * Whether a row matches the filter box.
 *
 * Matches on **the port number or the process name**: half the time the question is "what is
 * holding 3000", the other half is "where are those node processes running", and a filter box that
 * answers both saves having to choose.
 *
 * Process names compare case-insensitively — people type `node`, not `Node.exe`. Port numbers
 * compare as substrings, so typing `80` finds `80`, `8080` and `3080`; that is handy when you do
 * not remember the exact port.
 */
export function matchesFilter(row: ListeningPort, needle: string): boolean {
  const text = needle.trim().toLowerCase();
  if (text === "") return true;
  if (String(row.port).includes(text)) return true;
  return row.process !== null && row.process.toLowerCase().includes(text);
}
