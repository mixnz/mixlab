/**
 * `<clip>-<theme>.camera.png`: one frame a second of the encoded mp4 with the camera's views drawn
 * over it — wide solid, narrow dashed — and the cursor marked, for a person to check by eye.
 * Frames come from the mp4, so a view beside what it should frame is a timing error anyone can see.
 * docs/specs/2026-10-10-demo-clip-camera-shots-design.md.
 */
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { sheetTimes } from "./camera.mjs";
import { GLIDE_MS } from "./recorder.mjs";
import { cursorAt, viewAt } from "./shots.mjs";

const run = promisify(execFile);
const THUMB = { width: 520, height: 325 };
const COLUMNS = 4;

async function frameAt(ffmpeg, video, t) {
  const { stdout } = await run(
    ffmpeg,
    ["-loglevel", "error", "-ss", String(t), "-i", video, "-frames:v", "1", "-vf", `scale=${THUMB.width}:${THUMB.height}`, "-f", "image2pipe", "-c:v", "png", "-"],
    { encoding: "buffer", maxBuffer: 16 * 1024 * 1024 },
  );
  return stdout.toString("base64");
}

/** A view as a box over the thumbnail; the whole frame draws none. */
function frameBox(view, dashed) {
  if (view === "full") return "";
  const size = 100 / view.s;
  return `<div class="box${dashed ? " narrow" : ""}" style="left:${view.x * 100}%;top:${view.y * 100}%;width:${size}%;height:${size}%"></div>`;
}

/** Where the cursor is, as a dot with a ring, so one outside the views shows. */
function cursorDot(cursor) {
  if (cursor === null) return "";
  return `<div class="cursor" style="left:${cursor.x * 100}%;top:${cursor.y * 100}%"></div>`;
}

function cell(png, t, wide, narrow, cursor) {
  const caption = (view) => (view === "full" ? "full" : `×${view.s}`);
  return (
    `<figure><div class="shot"><img src="data:image/png;base64,${png}">${frameBox(wide, false)}${frameBox(narrow, true)}${cursorDot(cursor)}</div>` +
    `<figcaption>${t}s · wide ${caption(wide)} · narrow ${caption(narrow)}</figcaption></figure>`
  );
}

export async function writeContactSheet(browser, ffmpeg, { video, wide, narrow, actions = [], duration, out }) {
  const cells = [];
  for (const t of sheetTimes(duration)) {
    const cursor = cursorAt(actions, t, GLIDE_MS / 1000);
    cells.push(cell(await frameAt(ffmpeg, video, t), t, viewAt(wide, t), viewAt(narrow, t), cursor));
  }
  const html =
    `<style>body{margin:0;background:#111;font:14px system-ui;color:#ddd}` +
    `#sheet{display:grid;grid-template-columns:repeat(${COLUMNS},${THUMB.width}px);gap:12px;padding:12px;width:max-content}` +
    `figure{margin:0}.shot{position:relative;width:${THUMB.width}px;height:${THUMB.height}px;overflow:hidden}` +
    `img{display:block;width:100%;height:100%}` +
    `.box{position:absolute;outline:3px solid #ff3b6b;outline-offset:-3px;box-sizing:border-box}` +
    `.box.narrow{outline:3px dashed #38bdf8;outline-offset:-3px}` +
    `.cursor{position:absolute;width:10px;height:10px;margin:-5px 0 0 -5px;border-radius:50%;background:#facc15;box-shadow:0 0 0 2px #111,0 0 0 4px #facc15}` +
    `figcaption{padding-top:4px}</style><div id="sheet">${cells.join("")}</div>`;
  const context = await browser.newContext({ deviceScaleFactor: 1 });
  try {
    const page = await context.newPage();
    await page.setContent(html, { waitUntil: "load" });
    await page.locator("#sheet").screenshot({ path: out });
  } finally {
    await context.close();
  }
}
