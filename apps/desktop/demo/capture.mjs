/**
 * `npm run screenshots` — the six scenes, from sample data, as raw and framed PNGs.
 *
 *   npm run screenshots                          every scene, dark and light, macOS
 *   npm run screenshots -- --scene rest --theme dark
 *   npm run screenshots -- --platform windows
 *   npm run screenshots -- --check               render and verify every scene, write nothing
 *
 * The design is docs/specs/2026-09-17-marketing-screenshots-design.md.
 */
import { mkdir, rm } from "node:fs/promises";
import { join } from "node:path";
import { modifierFor, parseArgs } from "./args.mjs";
import {
  OUT,
  SCALE,
  collectProblems,
  firstLine,
  launch,
  openScene,
  printReport,
  startServer,
  waitReady,
  warmUp,
} from "./rig.mjs";
import { CONSTANTS, SCENES } from "./scenes.mjs";

const FRAME_VIEWPORT = { width: 1600, height: 1000 };
const RAW_ROUTE = "/__demo/raw.png";
const FREEZE_CSS =
  "*, *::before, *::after { animation: none !important; transition: none !important; caret-color: transparent !important; }";

/** Composites a raw image into `demo/frame/frame.html` and screenshots that. */
async function frameShot(browser, baseUrl, rawPath, framedPath, scene, theme, platform) {
  const context = await browser.newContext({ viewport: FRAME_VIEWPORT, deviceScaleFactor: SCALE });
  try {
    const page = await context.newPage();
    await page.route(`**${RAW_ROUTE}`, (route) => route.fulfill({ path: rawPath, contentType: "image/png" }));
    const query = new URLSearchParams({
      img: RAW_ROUTE,
      theme,
      platform,
      headline: scene.headline,
      description: scene.description,
    });
    await page.goto(`${baseUrl}/demo/frame/frame.html?${query}`);
    await page.waitForSelector('body[data-ready="true"]');
    await page.screenshot({ path: framedPath });
  } finally {
    await context.close();
  }
}

async function runScene(browser, baseUrl, scene, theme, options) {
  const { name, report, context, page, network, counter } = await openScene(
    browser,
    baseUrl,
    scene,
    theme,
    options,
  );
  try {
    try {
      await page.goto(`${baseUrl}/demo/demo.html`);
      await page.addStyleTag({ content: FREEZE_CSS });
      await waitReady(page, network);
      if (scene.act) {
        await scene.act({ page, modifier: modifierFor(options.platform), constants: CONSTANTS });
        await waitReady(page, network);
      }
    } catch (error) {
      report.problems.push(`failed     ${firstLine(error)}`);
    }

    await collectProblems(page, report, counter);

    if (report.problems.length === 0 && !options.check) {
      const rawPath = join(OUT, "raw", `${name}.png`);
      await page.screenshot({ path: rawPath, animations: "disabled", caret: "hide" });
      await frameShot(browser, baseUrl, rawPath, join(OUT, "framed", `${name}.png`), scene, theme, options.platform);
    }
  } finally {
    await context.close();
  }
  return report;
}

async function main() {
  let options;
  try {
    options = parseArgs(process.argv.slice(2), SCENES.map((scene) => scene.id));
  } catch (error) {
    console.error(firstLine(error));
    process.exit(2);
  }

  if (!options.check) {
    if (options.full) await rm(OUT, { recursive: true, force: true });
    await mkdir(join(OUT, "raw"), { recursive: true });
    await mkdir(join(OUT, "framed"), { recursive: true });
  }

  const { server, baseUrl } = await startServer();
  const reports = [];
  let browser;
  try {
    browser = await launch();
    await warmUp(browser, baseUrl);
    for (const scene of SCENES.filter((s) => options.scenes.includes(s.id))) {
      for (const theme of options.themes) {
        const report = await runScene(browser, baseUrl, scene, theme, options);
        printReport(report);
        reports.push(report);
      }
    }
  } finally {
    await browser?.close();
    await server.close();
  }

  const failed = reports.filter((report) => report.problems.length > 0);
  const verb = options.check ? "render" : "captured";
  console.log(`\n${reports.length - failed.length}/${reports.length} scenes ${verb}`);
  if (failed.length > 0) {
    console.log(`failed: ${failed.map((report) => report.name).join(", ")}`);
    console.log(
      "An `unmocked` line names a command to add to demo/fixtures — see docs/standards/desktop/demo-screenshots.md.",
    );
    process.exitCode = 1;
  } else if (!options.check) {
    console.log(`images in ${OUT}`);
  }
}

await main();
