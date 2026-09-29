export type TimeUnit = "seconds" | "millis" | "micros";

export interface TimeOutputs {
  isoUtc: string;
  isoLocal: string;
  unixSeconds: string;
  unixMillis: string;
  relative: string;
}

/**
 * The unit of a string of digits, guessed by length.
 *
 * Guessing is necessary — people paste a `bigint` column in here without knowing whether it is
 * seconds or milliseconds — but guessing silently is not: the Panel shows this function's result so
 * the user can see what it guessed.
 */
export function detectUnit(input: string): TimeUnit | null {
  const digits = input.trim();
  if (!/^\d+$/.test(digits)) return null;
  if (digits.length <= 11) return "seconds";
  if (digits.length <= 14) return "millis";
  if (digits.length <= 17) return "micros";
  return null;
}

const TO_MILLIS: Record<TimeUnit, number> = { seconds: 1000, millis: 1, micros: 1 / 1000 };

/** A point in time in milliseconds, from a string of digits or an ISO 8601 string. */
export function toInstant(input: string): number | null {
  const text = input.trim();
  if (text === "") return null;

  const unit = detectUnit(text);
  if (unit) return Math.round(Number(text) * TO_MILLIS[unit]);

  const parsed = Date.parse(text);
  return Number.isNaN(parsed) ? null : parsed;
}

const DIVISIONS: [limit: number, size: number, unit: Intl.RelativeTimeFormatUnit][] = [
  [60_000, 1000, "second"],
  [3_600_000, 60_000, "minute"],
  [86_400_000, 3_600_000, "hour"],
  [2_592_000_000, 86_400_000, "day"],
  [31_536_000_000, 2_592_000_000, "month"],
  [Infinity, 31_536_000_000, "year"],
];

/**
 * `now` is a parameter rather than `Date.now()` inside: that is what makes this function testable,
 * and also what lets the Panel freeze the result while the user is reading it.
 */
export function toOutputs(ms: number, timeZone: string, now: number): TimeOutputs {
  const date = new Date(ms);
  const diff = ms - now;

  let relative = "now";
  if (Math.abs(diff) >= 1000) {
    const [, size, unit] =
      DIVISIONS.find(([limit]) => Math.abs(diff) < limit) ?? DIVISIONS[DIVISIONS.length - 1];
    relative = new Intl.RelativeTimeFormat("en", { numeric: "auto" }).format(
      Math.round(diff / size),
      unit,
    );
  }

  return {
    isoUtc: date.toISOString(),
    // `sv-SE` gives `2026-08-28 07:00:00` — ISO without the T, the only one of the built-in locales
    // that prints a readable form that still sorts correctly.
    isoLocal: new Intl.DateTimeFormat("sv-SE", {
      timeZone,
      dateStyle: "short",
      timeStyle: "medium",
    }).format(date),
    unixSeconds: String(Math.floor(ms / 1000)),
    unixMillis: String(ms),
    relative,
  };
}
