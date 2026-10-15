import type { DbKind } from "./types";

/** One mark this tab's state calls for. Turned into a `TabBadge` — an icon and a translated label —
 *  by `tab/useTabBadges.tsx`, which is the only place with a `t` to hand. */
export type DbBadgeMark = { type: "kind"; kind: DbKind } | { type: "readOnly" };

/**
 * Which marks the tab bar should be showing for this tab.
 *
 * `kind` is `undefined` until there is a connection, and that gates both marks: before then the
 * logo belongs to the row in the sidebar, and the form on screen may be for another connection
 * entirely. The engine comes first — which server a tab is on is what you are looking for when
 * five of them are open, and the shape answers it before the name does, the name being truncated
 * to a word or two by then anyway.
 */
export function dbBadgeMarks(kind: DbKind | undefined, readOnly: boolean): DbBadgeMark[] {
  if (!kind) return [];
  const marks: DbBadgeMark[] = [{ type: "kind", kind }];
  if (readOnly) marks.push({ type: "readOnly" });
  return marks;
}
