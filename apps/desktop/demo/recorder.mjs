/**
 * What makes a clip look like a person using MixLab: a cursor that glides to what it is about to
 * press, typing at a readable pace, and every repaint kept with the moment it happened.
 */
import { inFrame, wallClock } from "./camera.mjs";

const CURSOR_ID = "__demo-cursor";
/** How long the cursor takes to reach a target; the camera reads it to place the cursor between. */
export const GLIDE_MS = 450;
const TYPE_DELAY_MS = 70;
const OPTION_PAUSE_MS = 350;
/* Longest a `waitFor` step waits for the app, e.g. a job to finish. */
const WAIT_FOR_MS = 20_000;
/* Where the arrow's tip sits inside its 24×24 box: the point that is actually aimed. */
const TIP = { x: 4, y: 2 };
/* Where each page's cursor points now, in CSS pixels: where its next glide starts. */
const cursors = new WeakMap();

/** Adds the cursor above everything. Added after the scene is quiet, so readiness never waits on it. */
export async function installCursor(page, viewport) {
  cursors.set(page, { x: viewport.width / 2, y: viewport.height * 0.6 });
  await page.evaluate(
    ({ id, glide, start }) => {
      const cursor = document.createElement("div");
      cursor.id = id;
      cursor.innerHTML =
        '<svg width="24" height="24" viewBox="0 0 24 24" aria-hidden="true">' +
        '<path d="M4 2l15 11.5-6.6.9 3.9 7.4-2.9 1.5-3.9-7.5L4 20z" fill="#111" stroke="#fff" ' +
        'stroke-width="1.5" stroke-linejoin="round"/></svg><span></span>';
      Object.assign(cursor.style, {
        position: "fixed",
        left: "0",
        top: "0",
        zIndex: "2147483647",
        pointerEvents: "none",
        transform: `translate(${start.x}px, ${start.y}px)`,
        transition: `transform ${glide}ms cubic-bezier(.45,0,.25,1)`,
      });
      Object.assign(cursor.lastElementChild.style, {
        position: "absolute",
        left: "-10px",
        top: "-12px",
        width: "28px",
        height: "28px",
        borderRadius: "50%",
        background: "rgba(46,144,250,.35)",
        opacity: "0",
      });
      document.body.appendChild(cursor);
    },
    { id: CURSOR_ID, glide: GLIDE_MS, start: { x: viewport.width / 2, y: viewport.height * 0.6 } },
  );
}

/** Takes the cursor away, for a picture that is not of a person using MixLab. */
export async function removeCursor(page) {
  await page.evaluate((id) => document.getElementById(id)?.remove(), CURSOR_ID);
}

/**
 * Glides to `locator` once it is wholly in the frame. Scrolls only when it is not — and says so,
 * since a scroll is on film. Comparing boxes before and after would not do: a dialog still easing
 * in moves everything in it without any scroll.
 */
async function glideTo(page, locator, { viewport, note }, what) {
  let box = await locator.boundingBox();
  if (box === null || !inFrame(box, viewport)) {
    await locator.scrollIntoViewIfNeeded();
    box = await locator.boundingBox();
    if (box !== null) note(`${what} scrolled into view`);
  }
  if (box === null) throw new Error("the element is not visible");
  if (!inFrame(box, viewport)) throw new Error(`${what} is not wholly in the frame`);
  const x = box.x + box.width / 2;
  const y = box.y + box.height / 2;
  const from = cursors.get(page) ?? { x, y };
  cursors.set(page, { x, y });
  await page.evaluate(
    ({ id, x, y }) => {
      document.getElementById(id).style.transform = `translate(${x}px, ${y}px)`;
    },
    { id: CURSOR_ID, x: x - TIP.x, y: y - TIP.y },
  );
  // The real pointer follows, so hover states show on film the way they would for a person — during
  // the glide, not after it: moved first, its steps alone doubled every glide.
  await Promise.all([page.mouse.move(x, y, { steps: 6 }), page.waitForTimeout(GLIDE_MS + 50)]);
  return { from, to: { x, y }, target: { x: box.x, y: box.y, w: box.width, h: box.height } };
}

async function press(page) {
  await page.evaluate((id) => {
    document
      .getElementById(id)
      .lastElementChild.animate(
        [
          { opacity: 0.9, transform: "scale(.4)" },
          { opacity: 0, transform: "scale(1.2)" },
        ],
        { duration: 300, easing: "ease-out" },
      );
  }, CURSOR_ID);
}

/**
 * Runs one step. `frame` is the clip's viewport, where a scroll on film is reported, and — through
 * `onAction` — where every glide and press is told: when it began and ended, from where the cursor
 * flew to where, and the target's box. The website's camera keeps those in view.
 */
export async function runStep(page, step, frame) {
  // An action ends as its press lands — not when the click returns, by which time what the click
  // opened is already on screen and the camera rightly gone to it — or, typing, at the last key.
  const act = async (locator, what, afterPress) => {
    const start = wallClock();
    const glide = await glideTo(page, locator, frame, what);
    await press(page);
    const landed = wallClock();
    const typed = await afterPress();
    frame.onAction?.({ start, end: typed ?? landed, ...glide });
  };
  if ("pause" in step) {
    await page.waitForTimeout(step.pause);
    return;
  }
  if ("waitFor" in step) {
    const target = page.locator(step.waitFor);
    await target.waitFor({ state: "visible", timeout: WAIT_FOR_MS });
    const box = await target.boundingBox();
    if (box === null || !inFrame(box, frame.viewport)) throw new Error("the awaited element is not wholly in the frame");
    return;
  }
  const target = page.locator(step.click ?? step.type ?? step.select);
  await act(target, "the target", async () => {
    await target.click();
    if (!("type" in step)) return null;
    await target.pressSequentially(step.text, { delay: TYPE_DELAY_MS });
    return wallClock();
  });
  if ("select" in step) {
    await page.waitForTimeout(OPTION_PAUSE_MS);
    const option = page.getByRole("option", { name: step.option, exact: true });
    await act(option, "the option", async () => {
      await option.click();
      return null;
    });
  }
}

/** Starts a CDP screencast; `stop()` returns every frame with its timestamp and when it reached
 *  Node, on the same clock (`camera.mjs`'s `clockCheck` holds that), and when it stopped. */
export async function startRecording(page, geometry) {
  const session = await page.context().newCDPSession(page);
  const frames = [];
  let lastArrived = 0;
  session.on("Page.screencastFrame", ({ data, metadata, sessionId }) => {
    frames.push({ data: Buffer.from(data, "base64"), t: metadata.timestamp, arrival: wallClock() });
    lastArrived = performance.now();
    session.send("Page.screencastFrameAck", { sessionId }).catch(() => {});
  });
  await session.send("Page.startScreencast", {
    format: "png",
    maxWidth: geometry.viewport.width * geometry.scale,
    maxHeight: geometry.viewport.height * geometry.scale,
    everyNthFrame: 1,
  });
  return {
    async stop() {
      const stoppedAt = performance.now();
      await session.send("Page.stopScreencast");
      await session.detach();
      const last = frames.at(-1);
      const end = last ? last.t + (stoppedAt - lastArrived) / 1000 : 0;
      return { frames, end };
    },
  };
}
