/**
 * `npm run clips` — the promotional clips, from sample data, as MP4 plus start and end posters.
 *
 *   npm run clips                             every clip, dark and light
 *   npm run clips -- --clip new-site --theme dark
 *   npm run clips -- --check                  film every clip, encode nothing (no ffmpeg needed)
 *
 * The design is docs/specs/2026-10-05-demo-clips-design.md.
 */
import { execFile } from "node:child_process";
import { mkdir, mkdtemp, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { promisify } from "node:util";
import { parseClipArgs } from "./args.mjs";
import { CLIPS, describeStep } from "./clips.mjs";
import {
  BUDGET,
  FFMPEG_MISSING,
  budgetVerdict,
  findFfmpeg,
  formatMB,
  posterArgs,
} from "./encode.mjs";
import { writeVideo } from "./ffmpeg.mjs";
import { installCursor, removeCursor, runStep, startRecording } from "./recorder.mjs";
import {
  OUT,
  collectProblems,
  firstLine,
  launch,
  openScene,
  printReport,
  startServer,
  waitReady,
  warmUp,
} from "./rig.mjs";

const CLIPS_OUT = join(OUT, "clips");
/* A beat of stillness before the first action and after each one, so nothing on film is abrupt.
   Steps do not wait for the scene to go quiet: Playwright already waits for what a step acts on,
   and on film the quiet check is dead air — seconds of a frozen window per step. */
const LEAD_IN_MS = 600;
const SETTLE_MS = 400;

const run = promisify(execFile);

async function encode(ffmpeg, clip, name, recording, report) {
  const work = await mkdtemp(join(tmpdir(), "mixlab-clip-"));
  try {
    const { frames, end } = recording;
    const video = join(CLIPS_OUT, `${name}.mp4`);
    await writeVideo(ffmpeg, frames, end, video);
    const first = join(work, "first.png");
    const last = join(work, "last.png");
    await writeFile(first, frames[0].data);
    await writeFile(last, frames.at(-1).data);
    await run(ffmpeg, posterArgs(first, join(CLIPS_OUT, `${name}-start.png`)));
    await run(ffmpeg, posterArgs(last, join(CLIPS_OUT, `${name}-end.png`)));
    // Already at 2×, and sized by the element rather than the window: written as taken.
    if (recording.still) await writeFile(join(CLIPS_OUT, `${name}-${clip.still.suffix}.png`), recording.still);
    const { size } = await stat(video);
    const verdict = budgetVerdict(size);
    if (verdict === "fail") {
      report.problems.push(`failed     ${formatMB(size)} is over the ${formatMB(BUDGET.fail)} budget`);
    } else {
      report.notes.push(`${formatMB(size)}${verdict === "warn" ? `, over ${formatMB(BUDGET.warn)}` : ""}`);
    }
  } finally {
    await rm(work, { recursive: true, force: true });
  }
}

async function runClip(browser, baseUrl, clip, theme, options, ffmpeg) {
  const { name, report, context, page, network, counter } = await openScene(browser, baseUrl, clip, theme, options, {
    reducedMotion: "no-preference",
    fixtures: clip.fixtures ?? {},
    colorScheme: theme,
  });
  let recording = null;
  try {
    try {
      await page.goto(`${baseUrl}/demo/demo.html`);
      await waitReady(page, network);
      if (clip.setup) {
        await clip.setup({ page });
        await waitReady(page, network);
      }
      await installCursor(page);
      const recorder = await startRecording(page);
      await page.waitForTimeout(LEAD_IN_MS);
      for (const [index, step] of clip.steps.entries()) {
        try {
          await runStep(page, step);
          if (!("pause" in step) && !("waitFor" in step)) await page.waitForTimeout(SETTLE_MS);
        } catch (error) {
          throw new Error(`step ${index}, ${describeStep(step)}: ${firstLine(error)}`);
        }
      }
      const stopped = await recorder.stop();
      // After the film stops: taking an element's picture repaints, and the screencast would keep it.
      if (clip.still) await removeCursor(page);
      const still = clip.still ? await page.locator(clip.still.selector).screenshot() : null;
      recording = { ...stopped, still };
    } catch (error) {
      report.problems.push(`failed     ${firstLine(error)}`);
    }

    await collectProblems(page, report, counter);

    if (report.problems.length === 0) {
      const frames = recording?.frames ?? [];
      if (frames.length < 2) {
        report.problems.push("failed     the screencast recorded no movement");
      } else if (frames[0].data.equals(frames.at(-1).data)) {
        report.problems.push("failed     the last frame is the first: the clip did nothing");
      }
    }
    if (report.problems.length === 0 && !options.check) {
      try {
        await encode(ffmpeg, clip, name, recording, report);
      } catch (error) {
        report.problems.push(`failed     encoding: ${firstLine(error)}`);
      }
    }
  } finally {
    await context.close();
  }
  return report;
}

async function main() {
  let options;
  try {
    options = parseClipArgs(process.argv.slice(2), CLIPS.map((clip) => clip.id));
  } catch (error) {
    console.error(firstLine(error));
    process.exit(2);
  }

  let ffmpeg = null;
  if (!options.check) {
    ffmpeg = findFfmpeg();
    if (ffmpeg === null) {
      console.error(FFMPEG_MISSING);
      process.exit(2);
    }
    if (options.full) await rm(CLIPS_OUT, { recursive: true, force: true });
    await mkdir(CLIPS_OUT, { recursive: true });
  }

  const { server, baseUrl } = await startServer();
  const reports = [];
  let browser;
  try {
    browser = await launch();
    await warmUp(browser, baseUrl);
    for (const clip of CLIPS.filter((c) => options.clips.includes(c.id))) {
      for (const theme of options.themes) {
        const report = await runClip(browser, baseUrl, clip, theme, options, ffmpeg);
        printReport(report);
        reports.push(report);
      }
    }
  } finally {
    await browser?.close();
    await server.close();
  }

  const failed = reports.filter((report) => report.problems.length > 0);
  console.log(`\n${reports.length - failed.length}/${reports.length} clips ${options.check ? "filmed" : "encoded"}`);
  if (failed.length > 0) {
    console.log(`failed: ${failed.map((report) => report.name).join(", ")}`);
    console.log("See docs/standards/desktop/demo-screenshots.md for what each line means.");
    process.exitCode = 1;
  } else if (!options.check) {
    console.log(`clips in ${CLIPS_OUT}`);
  }
}

await main();
