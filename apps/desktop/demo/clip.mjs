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
import { mkdir, mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { promisify } from "node:util";
import { parseClipArgs } from "./args.mjs";
import { CLIPS, describeStep, geometryOf } from "./clips.mjs";
import {
  BUDGET,
  FFMPEG_MISSING,
  FPS,
  VIDEO_SIZE,
  budgetVerdict,
  findFfmpeg,
  formatMB,
  POSTER_BUDGET,
  posterArgs,
  posterWebpArgs,
  resample,
} from "./encode.mjs";
import { clockCheck, inFrame } from "./camera.mjs";
import { writeContactSheet } from "./contactSheet.mjs";
import { writeVideo } from "./ffmpeg.mjs";
import { createSampler } from "./focus.mjs";
import { cameraFile, cameraTracks, normalizeAction, normalizeSample, trackStats } from "./shots.mjs";
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

/** The camera for a recording: its samples on the mp4's time — which starts at the first frame and
 *  lasts its ticks — as shots and views. */
function cameraOf(recording, geometry) {
  const duration = resample(recording.frames, recording.end).length / FPS;
  const start = recording.frames[0].t;
  return cameraFrom({ samples: recording.samples, actions: recording.actions, viewport: geometry.viewport, start, duration });
}

/** A camera from raw samples and actions — what filming measured, or what `--camera-only` read. */
function cameraFrom({ samples, actions, viewport, start, duration }) {
  const at = { viewport, start, duration };
  const normalized = actions.map((raw) => normalizeAction(raw, at));
  return {
    duration,
    start,
    actions: normalized,
    ...cameraTracks(samples.map((raw) => normalizeSample(raw, at)), duration, normalized),
  };
}

/** One line per step for a track: whether every action of it kept its target and cursor in view. */
function stepLine(checks) {
  const byStep = new Map();
  for (const check of checks) byStep.set(check.step, (byStep.get(check.step) ?? true) && check.ok);
  return [...byStep].map(([step, ok]) => `${step} ${ok ? "yes" : "NO"}`).join(", ");
}

/**
 * What the run says about a camera — its moves, every step's target in view or not, the pans it
 * made for them — and whether it may be written: a target out of view in either track, or more
 * zooms than the clip allows, fails it.
 */
function judgeCamera(clip, camera, report) {
  const wide = trackStats(camera.wide);
  const narrow = trackStats(camera.narrow);
  report.notes.push(
    `camera: ${camera.shots} shots; wide ${wide.zooms} zooms, ${wide.pans} pans; narrow ${narrow.zooms} zooms, ${narrow.pans} pans`,
  );
  for (const track of ["wide", "narrow"]) {
    report.notes.push(`${track} target in view, by step: ${stepLine(camera.checks[track]) || "no actions"}`);
    const pans = camera[track].filter((entry) => entry.label.startsWith("pan: ")).map((entry) => `${entry.t} ${entry.label.slice(5)}`);
    if (pans.length > 0) report.notes.push(`${track} pans: ${pans.join("; ")}`);
    for (const check of camera.checks[track].filter((c) => !c.ok)) {
      report.problems.push(`failed     step ${check.step} (${check.label}): its target or cursor leaves the ${track} view`);
      // Where it was and what was in view, so the failure can be read without filming again.
      const fmt = (view) => (view === "full" ? "full" : `${view.x},${view.y} ×${view.s}`);
      for (const action of camera.actions.filter((a) => a.step === check.step)) {
        const views = camera[track].filter((e, i, all) => (e.t <= action.start && (all[i + 1]?.t ?? Infinity) > action.start) || (e.t > action.start && e.t <= action.end));
        const r = action.reach;
        report.problems.push(
          `           ${action.start.toFixed(2)}–${action.end.toFixed(2)} s, reach ${r.x},${r.y} ${r.w}×${r.h}; views ${views.map((e) => `${e.t}: ${fmt(e.view)}`).join(" | ")}`,
        );
      }
    }
  }
  for (const note of camera.notes) report.notes.push(note);
  if (clip.maxZoomChanges !== undefined && wide.zooms > clip.maxZoomChanges) {
    report.problems.push(`failed     the camera zooms ${wide.zooms} times, more than ${clip.maxZoomChanges}`);
    const fmt = (view) => (view === "full" ? "full" : `×${view.s}`);
    report.problems.push(`           wide: ${camera.wide.map((e) => `${e.t} ${fmt(e.view)} ${e.label}`).join(" | ")}`);
  }
}

/** The camera file, the raw measurements it came from — so it can be recomputed — and its contact sheet. */
async function writeCamera(browser, ffmpeg, { clip, name, camera, samples, actions, viewport }) {
  const file = cameraFile({ clip: clip.id, duration: camera.duration, frame: VIDEO_SIZE, wide: camera.wide, narrow: camera.narrow });
  await writeFile(join(CLIPS_OUT, `${name}.camera.json`), `${JSON.stringify(file, null, 2)}\n`);
  const raw = { version: 2, clip: clip.id, duration: camera.duration, start: camera.start, viewport, samples, actions };
  await writeFile(join(CLIPS_OUT, `${name}.samples.json`), `${JSON.stringify(raw)}\n`);
  await writeContactSheet(browser, ffmpeg, {
    video: join(CLIPS_OUT, `${name}.mp4`),
    wide: camera.wide,
    narrow: camera.narrow,
    actions: camera.actions,
    duration: camera.duration,
    out: join(CLIPS_OUT, `${name}.camera.png`),
  });
}

async function encode(browser, ffmpeg, clip, name, recording, report, geometry) {
  const work = await mkdtemp(join(tmpdir(), "mixlab-clip-"));
  try {
    const { frames, end } = recording;
    const video = join(CLIPS_OUT, `${name}.mp4`);
    await writeVideo(ffmpeg, frames, end, video);
    await writeCamera(browser, ffmpeg, {
      clip,
      name,
      camera: recording.camera,
      samples: recording.samples,
      actions: recording.actions,
      viewport: geometry.viewport,
    });
    const first = join(work, "first.png");
    const last = join(work, "last.png");
    await writeFile(first, frames[0].data);
    await writeFile(last, frames.at(-1).data);
    await run(ffmpeg, posterArgs(first, join(CLIPS_OUT, `${name}-start.png`)));
    await run(ffmpeg, posterArgs(last, join(CLIPS_OUT, `${name}-end.png`)));
    for (const [frame, which] of [[first, "start"], [last, "end"]]) {
      const webp = join(CLIPS_OUT, `${name}-${which}.webp`);
      await run(ffmpeg, posterWebpArgs(frame, webp));
      const bytes = (await stat(webp)).size;
      if (bytes > POSTER_BUDGET) report.notes.push(`${which} poster ${formatMB(bytes)}, over ${formatMB(POSTER_BUDGET)}`);
    }
    // Already at the device scale, and sized by the element rather than the window: written as taken.
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
  const geometry = geometryOf(clip);
  const { name, report, context, page, network, counter } = await openScene(browser, baseUrl, clip, theme, options, {
    reducedMotion: "no-preference",
    fixtures: clip.fixtures ?? {},
    colorScheme: theme,
    geometry,
  });
  let recording = null;
  let sampler = null;
  const actions = [];
  try {
    try {
      await page.goto(`${baseUrl}/demo/demo.html`);
      await waitReady(page, network);
      if (clip.setup) {
        await clip.setup({ page });
        await waitReady(page, network);
      }
      await installCursor(page, geometry.viewport);
      const recorder = await startRecording(page, geometry);
      sampler = createSampler(page, clip.steps);
      await sampler.install();
      await page.waitForTimeout(LEAD_IN_MS);
      // From the first step on: the lead-in is the whole frame, and the camera says nothing of it.
      sampler.start();
      for (const [index, step] of clip.steps.entries()) {
        try {
          await sampler.enter(index, step);
          await runStep(page, step, {
            viewport: geometry.viewport,
            note: (text) => report.notes.push(`step ${index} ${text}`),
            onAction: (action) => actions.push({ ...action, step: index, label: describeStep(step) }),
          });
          if (!("pause" in step) && !("waitFor" in step)) await page.waitForTimeout(SETTLE_MS);
          await sampler.sample();
        } catch (error) {
          throw new Error(`step ${index}, ${describeStep(step)}: ${firstLine(error)}`);
        }
      }
      // What the clip says its last frame shows has to be there, whole, at the smaller window too.
      for (const selector of clip.endsShowing ?? []) {
        // Counted first: `boundingBox` waits for an element that is not there rather than saying so.
        const shown = page.locator(selector);
        const box = (await shown.count()) === 1 ? await shown.boundingBox() : null;
        if (box === null || !inFrame(box, geometry.viewport)) {
          throw new Error(`the last frame does not show ${selector} wholly`);
        }
      }
      const { samples, problems } = sampler.stop();
      const stopped = await recorder.stop();
      for (const problem of problems) report.problems.push(`failed     ${problem}`);
      // After the film stops: taking an element's picture repaints, and the screencast would keep it.
      if (clip.still) await removeCursor(page);
      const still = clip.still ? await page.locator(clip.still.selector).screenshot() : null;
      recording = { ...stopped, still, samples, actions };
    } catch (error) {
      sampler?.stop();
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
    if (report.problems.length === 0) {
      // The camera's times assume the screencast stamps frames with this machine's clock.
      const clock = clockCheck(recording.frames);
      if (!clock.ok) {
        report.problems.push(
          `failed     the screencast's clock is not this machine's (frames arrived ${clock.least} to ${clock.worst} s after their timestamps)`,
        );
      } else {
        report.notes.push(`frames reached node ${Math.round(clock.median * 1000)} ms after their timestamp (median)`);
      }
    }
    if (report.problems.length === 0) {
      recording.camera = cameraOf(recording, geometry);
      report.camera = { wide: recording.camera.wide, narrow: recording.camera.narrow, shots: recording.camera.shots };
      judgeCamera(clip, recording.camera, report);
    }
    if (report.problems.length === 0 && !options.check) {
      try {
        await encode(browser, ffmpeg, clip, name, recording, report, geometry);
      } catch (error) {
        report.problems.push(`failed     encoding: ${firstLine(error)}`);
      }
    }
  } finally {
    await context.close();
  }
  return report;
}

/** Both themes film one flow, so their cameras should cut the same; a run of one theme compares nothing. */
function noteShotCounts(clip, clipReports) {
  const filmed = clipReports.filter((r) => r.camera);
  if (filmed.length === 2 && filmed[0].camera.shots !== filmed[1].camera.shots) {
    console.log(`        note       ${clip.id}: light and dark have ${filmed[0].camera.shots} and ${filmed[1].camera.shots} shots`);
  }
}

/** A camera again from what an earlier run measured, against the mp4 that run encoded. */
async function recomputeCamera(browser, ffmpeg, clip, theme) {
  const name = `${clip.id}-${theme}`;
  const report = { name, problems: [], consoleErrors: [], notes: [] };
  let saved;
  try {
    saved = JSON.parse(await readFile(join(CLIPS_OUT, `${name}.samples.json`), "utf8"));
  } catch {
    report.problems.push(`failed     no ${name}.samples.json: film the clip first (npm run clips -- --clip ${clip.id})`);
    return report;
  }
  if (!Array.isArray(saved.actions)) {
    report.problems.push(`failed     ${name}.samples.json has no actions: film the clip again (npm run clips -- --clip ${clip.id})`);
    return report;
  }
  const camera = cameraFrom(saved);
  report.camera = { wide: camera.wide, narrow: camera.narrow, shots: camera.shots };
  judgeCamera(clip, camera, report);
  if (report.problems.length === 0) {
    await writeCamera(browser, ffmpeg, { clip, name, camera, samples: saved.samples, actions: saved.actions, viewport: saved.viewport });
  }
  return report;
}

/** `--camera-only`: every selected clip's camera from its saved samples. Films nothing, starts no Vite. */
async function recomputeCameras(options) {
  if (options.check) {
    console.error("--camera-only writes files; it has no --check");
    process.exit(2);
  }
  const ffmpeg = findFfmpeg();
  if (ffmpeg === null) {
    console.error(FFMPEG_MISSING);
    process.exit(2);
  }
  const reports = [];
  const browser = await launch();
  try {
    for (const clip of CLIPS.filter((c) => options.clips.includes(c.id))) {
      const clipReports = [];
      for (const theme of options.themes) {
        const report = await recomputeCamera(browser, ffmpeg, clip, theme);
        printReport(report);
        reports.push(report);
        clipReports.push(report);
      }
      noteShotCounts(clip, clipReports);
    }
  } finally {
    await browser.close();
  }
  const failed = reports.filter((report) => report.problems.length > 0);
  console.log(`\n${reports.length - failed.length}/${reports.length} cameras recomputed`);
  if (failed.length > 0) process.exitCode = 1;
}

async function main() {
  let options;
  try {
    options = parseClipArgs(process.argv.slice(2), CLIPS.map((clip) => clip.id));
  } catch (error) {
    console.error(firstLine(error));
    process.exit(2);
  }

  // Before anything is cleared: a full run empties the output folder, and these read it.
  if (options.cameraOnly) {
    await recomputeCameras(options);
    return;
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
      const clipReports = [];
      for (const theme of options.themes) {
        const report = await runClip(browser, baseUrl, clip, theme, options, ffmpeg);
        printReport(report);
        reports.push(report);
        clipReports.push(report);
      }
      noteShotCounts(clip, clipReports);
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
