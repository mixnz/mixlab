import type { MetricsMinute } from "@mixengine/api";

const MINUTE_MS = 60_000;

/**
 * Splits `minutes` (sorted by ascending time) into contiguous segments.
 *
 * **A missing minute is a segment boundary, not a point joined across** — the
 * `MetricsHistory.minutes` doc comment: a minute with no row is a minute nobody measured (service
 * stopped, machine asleep, daemon being replaced), never a minute that used 0. Drawing a line
 * joining two segments makes up data that was never taken.
 */
export function segmentsFor(minutes: readonly MetricsMinute[]): MetricsMinute[][] {
  const segments: MetricsMinute[][] = [];
  for (const minute of minutes) {
    const current = segments[segments.length - 1];
    const previous = current?.[current.length - 1];
    if (previous !== undefined && minute.minute - previous.minute === MINUTE_MS) {
      current.push(minute);
    } else {
      segments.push([minute]);
    }
  }
  return segments;
}

/**
 * Splits a segment into bands along which `defined` holds throughout.
 *
 * **The same rule as [`segmentsFor`], one level deeper.** A minute with a row may still carry no
 * CPU figure (`cpu_avg: null`, the `MetricsMinute` doc comment) — no reading in that minute got
 * one. Filtering that minute out and joining both sides draws a stretch nobody measured, exactly
 * the mistake [`segmentsFor`] avoids at the missing-minute level.
 */
export function runsOf(
  segment: readonly MetricsMinute[],
  defined: (minute: MetricsMinute) => boolean,
): MetricsMinute[][] {
  const runs: MetricsMinute[][] = [];
  let current: MetricsMinute[] | null = null;
  for (const minute of segment) {
    if (!defined(minute)) {
      current = null;
      continue;
    }
    if (current === null) {
      current = [minute];
      runs.push(current);
    } else {
      current.push(minute);
    }
  }
  return runs;
}

/** A half-open time range `[from, to)`. */
export interface TimeRange {
  from: number;
  to: number;
}

/**
 * The ranges within `[from, to)` that no segment covers — time nobody measured.
 *
 * The rule "a missing minute is a gap" only says the chart must not *join* across it. This says
 * the rest: a gap has to be **visible**, otherwise it cannot be told apart from a flat line. The
 * window spans the whole retention period, so a freshly started home sees right away that most of
 * the chart is time with no data yet, rather than mistaking its 40 minutes for 24 hours.
 *
 * A minute `n` covers `[n, n + 60s)` — it is a minute, not a point.
 */
export function gapsIn(
  segments: readonly (readonly MetricsMinute[])[],
  from: number,
  to: number,
): TimeRange[] {
  const gaps: TimeRange[] = [];
  let cursor = from;
  for (const segment of segments) {
    const first = segment[0];
    const last = segment[segment.length - 1];
    if (first === undefined || last === undefined) continue;
    if (first.minute > cursor) gaps.push({ from: cursor, to: Math.min(first.minute, to) });
    cursor = Math.max(cursor, last.minute + MINUTE_MS);
  }
  if (cursor < to) gaps.push({ from: cursor, to });
  return gaps.filter((gap) => gap.to > gap.from);
}

/**
 * The ranges in which every minute has at least `minimum` readings — the stretches when someone
 * was really watching.
 *
 * **A minute with one reading and a minute with sixty are not the same confidence**
 * (`MetricsMinute::samples` doc comment, and section 2 of the Metrics spec says plainly they must
 * not be drawn alike). This is how to say that without touching the data line itself: `samples: 1`
 * is the *normal* state of a machine where nobody has the Dashboard open, so drawing it faded would
 * fade almost the whole chart — losing exactly what needs to be read.
 *
 * What it makes clear is the **peak band**: at `samples: 1`, `cpu_peak` equals `cpu_avg` because
 * there is only one reading to compare, so the band flattens to nothing by itself. Flat because
 * nobody measured looks exactly like flat because usage really was steady, and nothing in the
 * picture tells the two apart.
 */
export function sampledRanges(
  segments: readonly (readonly MetricsMinute[])[],
  minimum: number,
): TimeRange[] {
  const ranges: TimeRange[] = [];
  for (const segment of segments) {
    let current: TimeRange | null = null;
    for (const minute of segment) {
      if (minute.samples < minimum) {
        current = null;
        continue;
      }
      if (current === null) {
        current = { from: minute.minute, to: minute.minute + MINUTE_MS };
        ranges.push(current);
      } else {
        current.to = minute.minute + MINUTE_MS;
      }
    }
  }
  return ranges;
}

/**
 * The minute nearest to `time`, or `null` if even the nearest minute is further than `tolerance`.
 *
 * `tolerance` is what keeps the crosshair honest: in a region with no data yet, the nearest minute
 * may be hours away, and reading its figures out under the cursor assigns a value to a moment
 * nobody measured.
 */
export function nearestMinute(
  minutes: readonly MetricsMinute[],
  time: number,
  tolerance: number,
): MetricsMinute | null {
  let best: MetricsMinute | null = null;
  let bestDistance = Infinity;
  for (const minute of minutes) {
    const distance = Math.abs(minute.minute - time);
    if (distance < bestDistance) {
      best = minute;
      bestDistance = distance;
    }
  }
  return bestDistance <= tolerance ? best : null;
}
