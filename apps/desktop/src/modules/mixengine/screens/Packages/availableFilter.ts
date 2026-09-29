/**
 * Filters the "not installed" table of Packages — display only; no RPC takes this search
 * (`runtime.list_available`/`package.list_available` return the whole list; see `Languages.tsx`).
 *
 * Each word in the search has to match somewhere, rather than the whole search matching in one
 * run: typing `php 8.3` still finds `php 8.3.14` even though the two pieces sit in different
 * fields, and the order they are typed in does not decide the result.
 */
export function matchesAvailable(fields: string[], query: string): boolean {
  const words = query.toLowerCase().split(/\s+/).filter((word) => word !== "");
  if (words.length === 0) return true;
  const haystack = fields.join(" ").toLowerCase();
  return words.every((word) => haystack.includes(word));
}
