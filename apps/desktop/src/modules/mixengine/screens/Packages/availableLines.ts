/**
 * The "not installed" list, one group per version line — T193a, D3 and the design's MixLab section.
 *
 * Display only: which line a release is in and which one is newest are the daemon's answers
 * (`line`, `newest_in_line`), and nothing here compares versions. A daemon from before T193 sends
 * neither, and then every row is a group of its own, which is the list as it was.
 */

export interface LinedRow {
  version: string;
  line?: string | null;
  newest_in_line?: boolean | null;
}

export interface LineGroup<R> {
  /** `kind@line`, or `kind@version` for a row with no line. */
  key: string;
  /** The row drawn for the line: its newest release. */
  head: R;
  /** The line's other releases, drawn only when `open`. */
  others: R[];
  /** Expanded by the person, or because the search matched only an older release. */
  open: boolean;
}

export function groupByLine<R extends LinedRow>(
  rows: readonly R[],
  nameOf: (row: R) => string,
  matches: (row: R) => boolean,
  opened: ReadonlySet<string>,
): LineGroup<R>[] {
  const byKey = new Map<string, R[]>();
  for (const row of rows) {
    const key = `${nameOf(row)}@${row.line ?? row.version}`;
    const members = byKey.get(key);
    if (members === undefined) byKey.set(key, [row]);
    else members.push(row);
  }

  const groups: LineGroup<R>[] = [];
  for (const [key, members] of byKey) {
    if (!members.some(matches)) continue;

    const head = members.find((row) => row.newest_in_line === true) ?? members[0];
    const others = members.filter((row) => row !== head);
    const matchedOnlyAnOlder = !matches(head) && others.some(matches);

    groups.push({ key, head, others, open: opened.has(key) || matchedOnlyAnOlder });
  }
  return groups;
}
