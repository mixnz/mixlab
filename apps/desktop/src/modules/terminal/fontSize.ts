/**
 * The terminal screen's font size.
 *
 * The default is noticeably larger than xterm's default, because this is where people sit reading
 * logs for minutes on end, not a label on a button.
 */
export const DEFAULT_FONT_SIZE = 16;

/* The two ends of the range, and both are real limits rather than pretty numbers: below 8 the text
   becomes unreadable, and above 32 an ordinary window only has about 40 columns — narrower than
   even `git log` assumes. */
export const MIN_FONT_SIZE = 8;
export const MAX_FONT_SIZE = 32;

/**
 * One step bigger or smaller, clamped to the range.
 *
 * Clamped rather than refused: a user holding `Ctrl+-` is saying "smaller still", and the right
 * answer once the bottom is reached is to stay put, not an error.
 */
export function stepFontSize(current: number, delta: number): number {
  const from = Number.isFinite(current) ? Math.round(current) : DEFAULT_FONT_SIZE;
  return Math.min(MAX_FONT_SIZE, Math.max(MIN_FONT_SIZE, from + delta));
}
