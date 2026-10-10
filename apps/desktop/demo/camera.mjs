/**
 * The arithmetic of a filmed clip's clock, fractions and frame, for the website's camera. Pure
 * apart from `wallClock`; the camera itself — shots, zooms, pans — is `demo/shots.mjs`.
 * The designs are docs/specs/2026-10-10-demo-clip-camera-design.md and
 * docs/specs/2026-10-10-demo-clip-camera-shots-design.md.
 */

/** The machine's clock in seconds, the one the screencast stamps its frames with. */
export function wallClock() {
  // `Date.now`, the system clock Chromium reads too — not `performance.timeOrigin + now()`, which
  // runs on a monotonic clock and drifts from it while the system clock is being slewed.
  return Date.now() / 1000;
}

const round = (value) => Math.round(value * 1000) / 1000;
const clamp = (value, low, high) => Math.min(high, Math.max(low, value));

/** A wall-clock time on the encoded video, which starts at the first frame. */
export function toVideoTime(at, start, duration) {
  return clamp(at - start, 0, duration);
}

/** Seconds two processes reading one machine's clock may disagree by: a frame can seem that early. */
const CLOCK_SLACK = 0.005;

/**
 * Whether the screencast's timestamps are on this clock: every frame arrives after it was swapped —
 * to within `CLOCK_SLACK` — and within a second.
 */
export function clockCheck(frames) {
  const delays = frames.map((frame) => frame.arrival - frame.t).sort((a, b) => a - b);
  const ok = delays.length > 0 && delays[0] > -CLOCK_SLACK && delays.at(-1) < 1;
  return { ok, least: round(delays[0] ?? 0), median: round(delays[delays.length >> 1] ?? 0), worst: round(delays.at(-1) ?? 0) };
}

/** A rectangle in CSS pixels as fractions of the frame, clipped to it. */
export function normalize(rect, viewport) {
  const left = clamp(rect.x / viewport.width, 0, 1);
  const top = clamp(rect.y / viewport.height, 0, 1);
  const right = clamp((rect.x + rect.w) / viewport.width, 0, 1);
  const bottom = clamp((rect.y + rect.h) / viewport.height, 0, 1);
  return { x: round(left), y: round(top), w: round(right - left), h: round(bottom - top) };
}

/** One time a second for the contact sheet, from 0, never at or past the end. */
export function sheetTimes(duration) {
  const times = [];
  for (let t = 0; t === 0 || t < duration - 1e-9; t++) times.push(t);
  return times;
}

/** A box (Playwright's shape) wholly inside the viewport. Half a pixel of slack for rounding. */
export function inFrame(box, viewport) {
  return (
    box.x >= -0.5 &&
    box.y >= -0.5 &&
    box.x + box.width <= viewport.width + 0.5 &&
    box.y + box.height <= viewport.height + 0.5
  );
}
