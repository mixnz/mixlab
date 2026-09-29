import { formatBytes, formatCpu } from "../../metricsState";

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const MIB = 1024 * 1024;

/**
 * A quantity that knows how to build its own vertical scale and write its own labels.
 *
 * The chart does not know whether it is drawing percentages or bytes, and should not: the two round
 * on different bases (10 and 2) and read differently. This is the only place that keeps that pair
 * right.
 */
export interface Unit {
  /** The top of the vertical axis for a given measured maximum — always a readable mark, and never
   *  below the quantity's floor. */
  niceMax(peak: number): number;
  /** The label of one grid line. Short — it sits beside the chart, not inside the tooltip. */
  tick(value: number): string;
  /** The full value in the tooltip, at exactly the precision the daemon sends. */
  value(value: number): string;
}

/**
 * Rounds `value` up to the nearest `steps` mark on base `base`.
 *
 * `steps` are mantissas in `[1, base)`, ascending, and the last entry must be `base` itself — a
 * value just below the top step still needs room to climb.
 */
function niceCeil(value: number, base: number, steps: readonly number[]): number {
  if (value <= 0) return steps[0]! * Math.pow(base, 0);
  const exponent = Math.floor(Math.log(value) / Math.log(base));
  const magnitude = Math.pow(base, exponent);
  const mantissa = value / magnitude;
  const step = steps.find((candidate) => candidate >= mantissa - 1e-9) ?? steps[steps.length - 1]!;
  return step * magnitude;
}

/**
 * **A scale with a floor, not a free scale.** An idle daemon at 0.03% with a scale running to
 * 0.05% draws measurement noise as mountains; the floor keeps a flat line looking flat. Since T190c
 * the chart draws a percentage of the **whole machine**, like Task Manager, so the floor is 5% of
 * the machine rather than a quarter of a core: small enough that a service using a few percent
 * still shows its shape clearly, large enough that noise does not become mountains.
 */
const CPU_FLOOR = 5;

/** The same reason on the byte side: below 64 MB what you see is allocation noise, not usage. */
const RSS_FLOOR = 64 * MIB;

/** Base 10, dense enough that `250` is not rounded up to `500`. */
const PERCENT_STEPS = [1, 1.5, 2, 2.5, 3, 4, 5, 6, 8, 10];

/** Base 2 — byte marks must be ones `formatBytes` writes neatly: `320 MB`, not `312.5 MB`. */
const BYTE_STEPS = [1, 1.25, 1.5, 2];

export const CPU_UNIT: Unit = {
  niceMax: (peak) => Math.max(CPU_FLOOR, niceCeil(peak, 10, PERCENT_STEPS)),
  // Two decimal places, then drop trailing zeros: `25`, `2.5`, `125` — not the tooltip's
  // `25.0000%`.
  tick: (value) => `${Number(value.toFixed(2))}%`,
  // The value is already a percentage of the whole machine (Metrics.tsx divides it beforehand), so
  // the denominator here is 1 (T190c).
  value: (share) => formatCpu(share, 1),
};

export const RSS_UNIT: Unit = {
  niceMax: (peak) => Math.max(RSS_FLOOR, niceCeil(peak, 2, BYTE_STEPS)),
  tick: formatBytes,
  value: formatBytes,
};

/** The allowed window widths, ascending. */
const WINDOWS = [HOUR, 3 * HOUR, 6 * HOUR, 12 * HOUR, 24 * HOUR];

/**
 * The start of the horizontal axis window: the smallest step that still holds all the data, at
 * most the whole retention period.
 *
 * **The window fits the data, but does not equal it.** Both extremes are wrong. Stretching exactly
 * the measured range across the full width was the first version's mistake: 40 minutes looked
 * exactly like 24 hours, with no label to correct it. Fixing it at 24 hours, on the other hand,
 * gives a freshly started home with half an hour of figures a streak 2% of the width — true, but
 * unreadable. The next step up is where both are right: the time labels say exactly where we are,
 * the unmeasured part still shows as unmeasured, and the data is still big enough to see.
 *
 * `earliest` is the earliest minute with a row, or `null` when there is no row yet.
 */
export function windowStart(earliest: number | null, to: number, retention: number): number {
  if (earliest === null) return to - retention;
  const span = to - earliest;
  const fitted = WINDOWS.find((window) => window >= span) ?? span;
  return to - Math.max(span, Math.min(fitted, retention));
}

/** The allowed time steps, ascending. Every step under an hour divides an hour, every step of an
 *  hour or more divides a day — the condition for aligning to round marks of the local calendar. */
const INTERVALS = [
  MINUTE,
  2 * MINUTE,
  5 * MINUTE,
  10 * MINUTE,
  15 * MINUTE,
  30 * MINUTE,
  HOUR,
  2 * HOUR,
  3 * HOUR,
  6 * HOUR,
  12 * HOUR,
  24 * HOUR,
];

/**
 * The first mark from `time` onwards, aligned to round hours/minutes of the **local calendar**.
 *
 * Aligned through `Date`'s fields rather than arithmetic on the epoch value: a time zone offset by
 * half an hour (India, Nepal) makes `Math.ceil(time / interval) * interval` land on 10:30, 11:30 —
 * the right spacing, the wrong marks.
 */
function alignUp(time: number, interval: number): number {
  const at = new Date(time);
  if (at.getSeconds() !== 0 || at.getMilliseconds() !== 0) {
    at.setSeconds(0, 0);
    at.setMinutes(at.getMinutes() + 1);
  }
  if (interval < HOUR) {
    const step = interval / MINUTE;
    at.setMinutes(Math.ceil(at.getMinutes() / step) * step);
  } else {
    at.setMinutes(0);
    const step = interval / HOUR;
    at.setHours(Math.ceil(at.getHours() / step) * step);
  }
  return at.getTime();
}

/** The next mark, also through `Date`'s fields: a daylight-saving day is not 24 hours long. */
function nextTick(time: number, interval: number): number {
  const at = new Date(time);
  if (interval < HOUR) at.setMinutes(at.getMinutes() + interval / MINUTE);
  else at.setHours(at.getHours() + interval / HOUR);
  return at.getTime();
}

/**
 * The round time marks within `[from, to]`, at most `max` of them.
 *
 * The step chosen is the smallest one that still fits `max` — a 24-hour window gives 6-hour marks,
 * a 20-minute window gives 5-minute marks. A short window must not leave the axis empty: data that
 * has only run for half an hour still has to say which half hour it is in.
 */
export function timeTicks(from: number, to: number, max: number): number[] {
  if (to <= from || max < 1) return [];
  const span = to - from;
  // `max` marks enclose `max - 1` intervals: a window exactly four steps long has five marks, not
  // four.
  const interval =
    INTERVALS.find((candidate) => span / candidate <= max - 1) ?? INTERVALS[INTERVALS.length - 1]!;

  const ticks: number[] = [];
  let at = alignUp(from, interval);
  while (at <= to && ticks.length < max) {
    ticks.push(at);
    const after = nextTick(at, interval);
    if (after <= at) break;
    at = after;
  }
  return ticks;
}
