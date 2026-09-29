/** Every character with a meaning in regex, to escape one by one. */
const SPECIAL = /[.*+?^${}()|[\]\\]/g;

/**
 * An SQL `LIKE` pattern as a Mongo regex pattern.
 *
 * The escaping here is not excess caution: `LIKE 'a.b%'` with the dot left unescaped becomes a
 * completely different query — still running, still giving results, just wrong ones. That is the
 * kind of bug nobody catches by eye.
 *
 * The `^`/`$` anchors are only dropped at whichever end has a `%`: `LIKE 'abc'` in SQL is exact
 * equality, not "contains". Escaping runs **before** `%` and `_` are converted, otherwise the `.*`
 * just produced would be escaped into `\.\*` right after.
 */
export function likeToRegex(pattern: string): string {
  const startsAny = pattern.startsWith("%");
  const endsAny = pattern.length > 1 && pattern.endsWith("%");

  const body = pattern
    .slice(startsAny ? 1 : 0, endsAny ? pattern.length - 1 : undefined)
    .replace(SPECIAL, "\\$&")
    .replace(/%/g, ".*")
    .replace(/_/g, ".");

  // `LIKE '%'` matches everything. So does `.*$`, but the trailing anchor only makes the reader
  // stop and wonder what it is for.
  if (startsAny && body === "") return ".*";

  return `${startsAny ? ".*" : "^"}${body}${endsAny ? ".*" : "$"}`;
}
