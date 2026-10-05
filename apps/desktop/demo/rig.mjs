/**
 * What `npm run screenshots` and `npm run clips` share: the Vite server, a Chromium context seeded
 * for one scene, waiting for the scene to go quiet, and the report each run prints.
 *
 * The designs are docs/specs/2026-09-17-marketing-screenshots-design.md and
 * docs/specs/2026-10-05-demo-clips-design.md.
 */
import { createServer as createNetServer } from "node:net";
import { dirname, join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import { createServer } from "vite";
import { USER_AGENTS, storageFor } from "./args.mjs";
import { createQuietDetector } from "./readiness.mjs";
import { CONSTANTS } from "./scenes.mjs";

export const DESKTOP = join(dirname(fileURLToPath(import.meta.url)), "..");
export const OUT = join(DESKTOP, "screenshots", "out");
export const VIEWPORT = { width: 1440, height: 900 };
export const SCALE = 2;
const READY_TIMEOUT_MS = 20_000;
const ACT_TIMEOUT_MS = 10_000;

/** A port the OS says is free right now — never a fixed one. */
function freePort() {
  return new Promise((resolve, reject) => {
    const server = createNetServer();
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
  });
}

/* Runs in the page before any of its scripts, on every navigation of the top frame. `fixtures` is
   what a clip asks of the fixtures — `demo/fixtures/options.ts` reads it. */
function initScene({ storage, fixtures }) {
  if (window.top !== window) return;
  localStorage.clear();
  for (const [key, value] of Object.entries(storage)) localStorage.setItem(key, value);
  window.__demoFixtures = fixtures;
  window.__demoMutations = 0;
  new MutationObserver((records) => {
    window.__demoMutations += records.length;
  }).observe(document, { subtree: true, childList: true, attributes: true, characterData: true });
}

/**
 * Requests the page has started and not finished.
 *
 * Counted as activity alongside IPC because a cold Vite server can take seconds to transform a
 * module a screen imports lazily, and a page waiting on one makes no IPC call and no DOM change.
 */
function trackNetwork(page) {
  const network = { started: 0, open: 0 };
  page.on("request", () => {
    network.started++;
    network.open++;
  });
  const settle = () => network.open--;
  page.on("requestfinished", settle);
  page.on("requestfailed", settle);
  return network;
}

export async function waitReady(page, network) {
  const deadline = Date.now() + READY_TIMEOUT_MS;
  const isQuiet = createQuietDetector();
  while (Date.now() < deadline) {
    const sample = await page.evaluate(() => ({
      inFlight: window.__demo?.inFlight ?? 1,
      calls: window.__demo?.calls ?? 0,
      mutations: window.__demoMutations ?? 0,
      mounted: (document.getElementById("root")?.childElementCount ?? 0) > 0,
    }));
    const activity = {
      ...sample,
      inFlight: sample.inFlight + network.open,
      calls: sample.calls + network.started,
    };
    if (isQuiet(activity)) {
      await page.evaluate(async () => {
        await document.fonts.ready;
        await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      });
      return;
    }
    await delay(100);
  }
  const pending = await page.evaluate(() => window.__demo?.pending ?? []);
  throw new Error(
    `not ready after ${READY_TIMEOUT_MS} ms (IPC in flight: ${pending.join(", ") || "nothing"}; ` +
      `requests open: ${network.open})`,
  );
}

export function firstLine(error) {
  return String(error?.message ?? error).split("\n")[0];
}

export async function startServer() {
  const port = await freePort();
  const server = await createServer({
    root: DESKTOP,
    configFile: join(DESKTOP, "vite.config.ts"),
    logLevel: "warn",
    clearScreen: false,
    server: { host: "127.0.0.1", port, strictPort: true, hmr: false },
    optimizeDeps: { entries: ["index.html", "demo/demo.html", "demo/frame/frame.html"] },
  });
  await server.listen();
  return { server, baseUrl: `http://127.0.0.1:${port}` };
}

/** A fresh context for one scene or clip: seeded storage, fixed clock, and a report that listens. */
export async function openScene(
  browser,
  baseUrl,
  scene,
  theme,
  options,
  { reducedMotion = "reduce", fixtures = {}, colorScheme } = {},
) {
  const name = `${scene.id}-${theme}`;
  const report = { name, problems: [], consoleErrors: [], notes: [] };
  const context = await browser.newContext({
    viewport: VIEWPORT,
    deviceScaleFactor: SCALE,
    userAgent: USER_AGENTS[options.platform],
    reducedMotion,
    // Only a clip sets it: what a page inside the window (a browser's site) shows follows it.
    ...(colorScheme ? { colorScheme } : {}),
    locale: "en-US",
    timezoneId: "UTC",
  });
  await context.clock.setFixedTime(new Date(CONSTANTS.now));
  await context.addInitScript(initScene, { storage: storageFor(scene, theme), fixtures });
  const page = await context.newPage();
  page.setDefaultTimeout(ACT_TIMEOUT_MS);
  const network = trackNetwork(page);
  const counter = { navigations: 0 };
  page.on("framenavigated", (frame) => {
    if (frame === page.mainFrame()) counter.navigations++;
  });
  page.on("pageerror", (error) => report.problems.push(`pageerror  ${firstLine(error)}`));
  page.on("console", (message) => {
    if (message.type() === "error") report.consoleErrors.push(message.text().split("\n")[0]);
  });
  return { name, report, context, page, network, counter };
}

/** What the page's probe says went wrong: unanswered commands, app errors, a reload. */
export async function collectProblems(page, report, counter) {
  const probe = await page
    .evaluate(() => ({ unmocked: window.__demo?.unmocked ?? [], errors: window.__demo?.errors ?? [] }))
    .catch(() => ({ unmocked: [], errors: [] }));
  for (const call of probe.unmocked) report.problems.push(`unmocked   ${call.cmd} ${call.args}`);
  for (const message of probe.errors) report.problems.push(`app error  ${message.split("\n")[0]}`);
  // What the probe says belongs to the last document only; a reload would have hidden the rest.
  if (counter.navigations > 1) report.problems.push("failed     the page reloaded during the scene");
}

export function printReport(report) {
  console.log(`${report.problems.length === 0 ? "ok  " : "FAIL"}  ${report.name}`);
  for (const problem of report.problems) console.log(`        ${problem}`);
  for (const note of report.notes) console.log(`        note       ${note}`);
  for (const line of report.consoleErrors) console.log(`        console    ${line}`);
}

export async function launch() {
  try {
    return await chromium.launch();
  } catch (error) {
    if (String(error?.message).includes("Executable doesn't exist")) {
      console.error("Chromium for Playwright is not installed. Run:\n\n  npx playwright install chromium\n");
      process.exit(2);
    }
    throw error;
  }
}

/* Vite optimises dependencies on the first request and may reload the page when it finds more;
   paying for that here keeps it out of the first scene. */
export async function warmUp(browser, baseUrl) {
  const context = await browser.newContext();
  try {
    const page = await context.newPage();
    await page.goto(`${baseUrl}/demo/demo.html`, { waitUntil: "networkidle" });
    await page.goto(`${baseUrl}/demo/frame/frame.html`, { waitUntil: "networkidle" });
  } finally {
    await context.close();
  }
}
