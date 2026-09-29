/**
 * Compares two pieces of text line by line.
 *
 * LCS is O(n×m) in both time and memory, and memory is where it hurts: a 5000×5000 table of 4-byte
 * cells is 95 MB for one click. So **trim the identical head and tail first**, only then run LCS on
 * the middle, and cap the middle. Ten differing lines between two 50-thousand-line files leave a
 * table of only 10×10.
 */

export interface DiffLine {
  kind: "same" | "add" | "remove";
  /** The line number on each side, or `null` if the line does not exist on that side. */
  leftNo: number | null;
  rightNo: number | null;
  text: string;
}

export interface DiffOptions {
  ignoreWhitespace: boolean;
  ignoreCase: boolean;
}

export type DiffResult =
  | { ok: true; lines: DiffLine[]; added: number; removed: number }
  | { ok: false; reason: "tooLarge" };

/** 2000×2000 cells of 4 bytes is 16 MB — acceptable for one click. */
const MAX_MIDDLE = 2000;

function keyOf(line: string, options: DiffOptions): string {
  const text = options.ignoreWhitespace ? line.replace(/\s+/g, " ").trim() : line;
  return options.ignoreCase ? text.toLowerCase() : text;
}

export function diffLines(left: string, right: string, options: DiffOptions): DiffResult {
  const a = left.split("\n");
  const b = right.split("\n");
  const ka = a.map((line) => keyOf(line, options));
  const kb = b.map((line) => keyOf(line, options));

  let head = 0;
  while (head < a.length && head < b.length && ka[head] === kb[head]) head += 1;

  let tail = 0;
  while (
    tail < a.length - head &&
    tail < b.length - head &&
    ka[a.length - 1 - tail] === kb[b.length - 1 - tail]
  ) {
    tail += 1;
  }

  const midA = a.length - head - tail;
  const midB = b.length - head - tail;
  if (midA > MAX_MIDDLE || midB > MAX_MIDDLE) return { ok: false, reason: "tooLarge" };

  // The LCS table runs backwards from the end, so cell (i, j) is the common-subsequence length of
  // the two tails.
  const width = midB + 1;
  const table = new Uint32Array((midA + 1) * width);
  const at = (i: number, j: number): number => i * width + j;
  for (let i = midA - 1; i >= 0; i -= 1) {
    for (let j = midB - 1; j >= 0; j -= 1) {
      table[at(i, j)] =
        ka[head + i] === kb[head + j]
          ? table[at(i + 1, j + 1)]! + 1
          : Math.max(table[at(i + 1, j)]!, table[at(i, j + 1)]!);
    }
  }

  const lines: DiffLine[] = [];
  let added = 0;
  let removed = 0;

  for (let k = 0; k < head; k += 1) {
    lines.push({ kind: "same", leftNo: k + 1, rightNo: k + 1, text: a[k]! });
  }

  let i = 0;
  let j = 0;
  while (i < midA && j < midB) {
    if (ka[head + i] === kb[head + j]) {
      lines.push({ kind: "same", leftNo: head + i + 1, rightNo: head + j + 1, text: a[head + i]! });
      i += 1;
      j += 1;
    } else if (table[at(i + 1, j)]! >= table[at(i, j + 1)]!) {
      lines.push({ kind: "remove", leftNo: head + i + 1, rightNo: null, text: a[head + i]! });
      removed += 1;
      i += 1;
    } else {
      lines.push({ kind: "add", leftNo: null, rightNo: head + j + 1, text: b[head + j]! });
      added += 1;
      j += 1;
    }
  }
  while (i < midA) {
    lines.push({ kind: "remove", leftNo: head + i + 1, rightNo: null, text: a[head + i]! });
    removed += 1;
    i += 1;
  }
  while (j < midB) {
    lines.push({ kind: "add", leftNo: null, rightNo: head + j + 1, text: b[head + j]! });
    added += 1;
    j += 1;
  }

  for (let k = 0; k < tail; k += 1) {
    const li = a.length - tail + k;
    const ri = b.length - tail + k;
    lines.push({ kind: "same", leftNo: li + 1, rightNo: ri + 1, text: a[li]! });
  }

  return { ok: true, lines, added, removed };
}

export interface DiffSegment {
  text: string;
  changed: boolean;
}

/**
 * Below this threshold the shared head/tail is just a coincidence between two unrelated lines (e.g.
 * two completely different SQL statements that both start with `SELECT` and end with `;`) —
 * highlighting only the middle then misleads more than highlighting the whole line as before, so
 * return `null` for the caller to render the whole line.
 */
const MIN_SEGMENT_SIMILARITY = 0.3;

function segmentKey(ch: string, ignoreCase: boolean): string {
  return ignoreCase ? ch.toLowerCase() : ch;
}

/**
 * Highlights just the differing part between a removed line and its matching added line, using the
 * same identical-head/tail trimming trick as above but at the character level (code points, through
 * `Array.from`, so a two-code-unit Unicode character is not split).
 *
 * Ignores `ignoreWhitespace`: mapping positions back after collapsing whitespace is not worth the
 * effort, so when that option is on it returns `null` straight away — the caller renders the whole
 * line.
 */
export function diffSegments(
  leftText: string,
  rightText: string,
  options: DiffOptions
): { left: DiffSegment[]; right: DiffSegment[] } | null {
  if (options.ignoreWhitespace) return null;

  const a = Array.from(leftText);
  const b = Array.from(rightText);
  const ka = a.map((ch) => segmentKey(ch, options.ignoreCase));
  const kb = b.map((ch) => segmentKey(ch, options.ignoreCase));

  let head = 0;
  while (head < a.length && head < b.length && ka[head] === kb[head]) head += 1;

  const remaining = Math.min(a.length, b.length) - head;
  let tail = 0;
  while (tail < remaining && ka[a.length - 1 - tail] === kb[b.length - 1 - tail]) tail += 1;

  const longest = Math.max(a.length, b.length);
  if (longest === 0 || (head + tail) / longest < MIN_SEGMENT_SIMILARITY) return null;

  const build = (chars: string[]): DiffSegment[] => {
    const segments: DiffSegment[] = [];
    if (head > 0) segments.push({ text: chars.slice(0, head).join(""), changed: false });
    const midEnd = chars.length - tail;
    if (midEnd > head) segments.push({ text: chars.slice(head, midEnd).join(""), changed: true });
    if (tail > 0) segments.push({ text: chars.slice(midEnd).join(""), changed: false });
    return segments;
  };

  return { left: build(a), right: build(b) };
}

interface DiffRun {
  removes: DiffLine[];
  adds: DiffLine[];
}

/**
 * Groups adjacent non-"same" lines into clusters. Because of how ties are broken above (`>=`
 * favours remove), a cluster is always a block of removes followed by a block of adds — but this
 * function does not rely on that order to be correct: it splits by `kind` whether removes and adds
 * interleave or not.
 */
function findRuns(lines: DiffLine[]): DiffRun[] {
  const runs: DiffRun[] = [];
  let i = 0;
  while (i < lines.length) {
    if (lines[i]!.kind === "same") {
      i += 1;
      continue;
    }
    const removes: DiffLine[] = [];
    const adds: DiffLine[] = [];
    while (i < lines.length && lines[i]!.kind !== "same") {
      const line = lines[i]!;
      if (line.kind === "remove") removes.push(line);
      else adds.push(line);
      i += 1;
    }
    runs.push({ removes, adds });
  }
  return runs;
}

/**
 * For each remove/add cluster, pairs by position (the k-th remove with the k-th add) to compute the
 * differing parts — leftovers (when removes and adds are not equal in number) are not paired and
 * keep rendering the whole line.
 *
 * Returns a map from `DiffLine` (by reference, from the same `lines` array passed in) to that
 * line's own segments, used by the Unified view: each line still renders on its own row, and only
 * the differing part inside is highlighted more strongly.
 */
export function computeLineSegments(lines: DiffLine[], options: DiffOptions): Map<DiffLine, DiffSegment[]> {
  const map = new Map<DiffLine, DiffSegment[]>();
  for (const run of findRuns(lines)) {
    const pairCount = Math.min(run.removes.length, run.adds.length);
    for (let k = 0; k < pairCount; k += 1) {
      const removeLine = run.removes[k]!;
      const addLine = run.adds[k]!;
      const result = diffSegments(removeLine.text, addLine.text, options);
      if (result === null) continue;
      map.set(removeLine, result.left);
      map.set(addLine, result.right);
    }
  }
  return map;
}

/**
 * One cell in the Split view. `"blank"` is the side with no corresponding line (the leftover when
 * removes and adds differ in number) — no line number, no text. `kind` tells "same" (no colour)
 * from "remove"/"add" (red/green background), so the renderer does not have to guess by comparing
 * `no`/`text`.
 */
export type SplitCell =
  | { kind: "blank" }
  | { kind: "same" | "remove" | "add"; no: number; text: string; segments: DiffSegment[] | null };

export interface SplitRow {
  left: SplitCell;
  right: SplitCell;
}

const BLANK_CELL: SplitCell = { kind: "blank" };

function cellOf(
  kind: "same" | "remove" | "add",
  no: number,
  text: string,
  segments: DiffSegment[] | null
): SplitCell {
  return { kind, no, text, segments };
}

/**
 * Builds the rows for the Split view (2 side-by-side columns, GitHub-style): a "same" line takes
 * one row on both sides, a pairable remove/add takes one "replaced" row, and a leftover (an odd
 * remove or add) takes a row with only one side — the other side is blank.
 */
export function buildSplitRows(lines: DiffLine[], options: DiffOptions): SplitRow[] {
  const rows: SplitRow[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i]!;
    if (line.kind === "same") {
      rows.push({
        left: cellOf("same", line.leftNo!, line.text, null),
        right: cellOf("same", line.rightNo!, line.text, null),
      });
      i += 1;
      continue;
    }

    const removes: DiffLine[] = [];
    const adds: DiffLine[] = [];
    while (i < lines.length && lines[i]!.kind !== "same") {
      const current = lines[i]!;
      if (current.kind === "remove") removes.push(current);
      else adds.push(current);
      i += 1;
    }

    const pairCount = Math.min(removes.length, adds.length);
    for (let k = 0; k < pairCount; k += 1) {
      const removeLine = removes[k]!;
      const addLine = adds[k]!;
      const result = diffSegments(removeLine.text, addLine.text, options);
      rows.push({
        left: cellOf("remove", removeLine.leftNo!, removeLine.text, result?.left ?? null),
        right: cellOf("add", addLine.rightNo!, addLine.text, result?.right ?? null),
      });
    }
    for (let k = pairCount; k < removes.length; k += 1) {
      rows.push({ left: cellOf("remove", removes[k]!.leftNo!, removes[k]!.text, null), right: BLANK_CELL });
    }
    for (let k = pairCount; k < adds.length; k += 1) {
      rows.push({ left: BLANK_CELL, right: cellOf("add", adds[k]!.rightNo!, adds[k]!.text, null) });
    }
  }
  return rows;
}
