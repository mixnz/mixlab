/**
 * Turning recorded frames into the clip the website plays. Pure apart from `findFfmpeg`'s default
 * probe, so it is unit-tested without ffmpeg; `demo/clip.mjs` runs what these return.
 */
import { spawnSync } from "node:child_process";

/** The website shows the clip at most 1040 CSS pixels wide: twice that is sharp, more is waste. */
export const VIDEO_SIZE = { width: 2080, height: 1300 };
const SCALE_FILTER = `scale=${VIDEO_SIZE.width}:${VIDEO_SIZE.height}:flags=lanczos`;

/** Bytes. Over `warn` is printed; over `fail` fails the clip. */
export const BUDGET = { warn: 1_000_000, fail: 1_500_000 };

/** Frames a second in the clip. */
export const FPS = 24;

/**
 * Which recorded frame each tick of the clip shows: the last one that had arrived by then. The
 * screencast only sends a frame when something repaints, so recorded frames are unevenly spaced;
 * the clip is not, and it lasts until recording stopped, the last frame held to the end.
 */
export function resample(frames, end, fps = FPS) {
  if (frames.length === 0) throw new Error("no frames to encode");
  const start = frames[0].t;
  // Enough ticks to reach the end; the slack keeps 4.4999… (floating point for 4.5) from losing one.
  const ticks = Math.max(1, Math.ceil((end - start) * fps - 1e-9));
  const shown = [];
  let index = 0;
  for (let tick = 0; tick < ticks; tick++) {
    const time = start + tick / fps;
    // A hair of slack, so a frame that arrived exactly on a tick counts as arrived.
    while (index + 1 < frames.length && frames[index + 1].t <= time + 1e-9) index++;
    shown.push(index);
  }
  return shown;
}

/** ffmpeg reading PNG frames, one per tick, from stdin. */
export function videoArgs(outPath) {
  return [
    ...["-y", "-loglevel", "error"],
    ...["-f", "image2pipe", "-c:v", "png", "-framerate", String(FPS), "-i", "-"],
    ...["-vf", SCALE_FILTER],
    ...["-c:v", "libx264", "-preset", "veryslow", "-crf", "26", "-tune", "stillimage"],
    ...["-pix_fmt", "yuv420p", "-movflags", "+faststart", "-an"],
    outPath,
  ];
}

/** Bytes a WebP poster should stay under; over it is printed, never a failure. */
export const POSTER_BUDGET = 150_000;

/** The poster as WebP: the website shows it first, so it is the page's largest paint. */
export function posterWebpArgs(inPath, outPath) {
  return ["-y", "-loglevel", "error", "-i", inPath, "-vf", SCALE_FILTER, "-c:v", "libwebp", "-quality", "80", outPath];
}

export function posterArgs(inPath, outPath) {
  return ["-y", "-loglevel", "error", "-i", inPath, "-vf", SCALE_FILTER, outPath];
}

export function budgetVerdict(bytes) {
  if (bytes > BUDGET.fail) return "fail";
  if (bytes > BUDGET.warn) return "warn";
  return "ok";
}

export function formatMB(bytes) {
  return `${(bytes / 1_000_000).toFixed(2)} MB`;
}

const runs = (cmd) => spawnSync(cmd, ["-version"], { stdio: "ignore" }).status === 0;

/** `FFMPEG` when it runs, else `ffmpeg` on PATH when it runs, else null. */
export function findFfmpeg(env = process.env, works = runs) {
  for (const candidate of [env.FFMPEG, "ffmpeg"]) {
    if (candidate && works(candidate)) return candidate;
  }
  return null;
}

export const FFMPEG_MISSING =
  "ffmpeg is needed to encode the clips (Playwright's own only writes blurry WebM). Install it:\n\n" +
  "  Windows  winget install Gyan.FFmpeg\n" +
  "  macOS    brew install ffmpeg\n" +
  "  Linux    sudo apt install ffmpeg\n\n" +
  "or point FFMPEG at the binary. `npm run clips -- --check` runs without it.\n";
