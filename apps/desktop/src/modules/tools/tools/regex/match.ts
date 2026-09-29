/**
 * Runs a regex and collects the results. A pure function — running it in a Worker lives in
 * `run.ts`.
 *
 * Split exactly here because this is the testable boundary: the match-collecting loop runs in
 * Node, while `Worker` does not exist there.
 */

export interface RegexGroup {
  /** `null` for numbered groups. Named groups are listed separately, with an `index` of `-1`. */
  name: string | null;
  index: number;
  text: string | null;
}

export interface RegexMatch {
  index: number;
  text: string;
  groups: RegexGroup[];
}

export type RegexRun =
  | { ok: true; matches: RegexMatch[]; truncated: boolean; replaced: string | null }
  | { ok: false; message: string };

/** Enough to look at, and keeps the payload sent out of the Worker at a meaningful size. */
const MAX_MATCHES = 500;

function toMatch(found: RegExpExecArray): RegexMatch {
  const groups: RegexGroup[] = [];
  for (let i = 1; i < found.length; i += 1) {
    groups.push({ name: null, index: i, text: found[i] ?? null });
  }
  for (const [name, text] of Object.entries(found.groups ?? {})) {
    groups.push({ name, index: -1, text: text ?? null });
  }
  return { index: found.index, text: found[0]!, groups };
}

export function runRegex(
  pattern: string,
  flags: string,
  subject: string,
  replacement: string,
): RegexRun {
  let re: RegExp;
  try {
    re = new RegExp(pattern, flags);
  } catch (error) {
    // The engine's message already points at the right place; rewriting it would only blur it.
    return { ok: false, message: error instanceof Error ? error.message : String(error) };
  }

  const matches: RegexMatch[] = [];
  let truncated = false;

  if (re.global || re.sticky) {
    re.lastIndex = 0;
    for (;;) {
      const found = re.exec(subject);
      if (!found) break;
      matches.push(toMatch(found));
      // An empty-matching pattern leaves `lastIndex` in place and `exec` returns forever.
      if (found[0] === "") re.lastIndex += 1;
      if (matches.length >= MAX_MATCHES) {
        truncated = true;
        break;
      }
    }
  } else {
    const found = re.exec(subject);
    if (found) matches.push(toMatch(found));
  }

  re.lastIndex = 0;
  const replaced = replacement === "" ? null : subject.replace(re, replacement);
  return { ok: true, matches, truncated, replaced };
}
