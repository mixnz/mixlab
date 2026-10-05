/**
 * What makes a clip look like a person using MixLab: a cursor that glides to what it is about to
 * press, typing at a readable pace, and every repaint kept with the moment it happened.
 */
import { SCALE, VIEWPORT } from "./rig.mjs";

const CURSOR_ID = "__demo-cursor";
const GLIDE_MS = 450;
const TYPE_DELAY_MS = 70;
const OPTION_PAUSE_MS = 350;
/* Longest a `waitFor` step waits for the app, e.g. a job to finish. */
const WAIT_FOR_MS = 20_000;
/* Where the arrow's tip sits inside its 24×24 box: the point that is actually aimed. */
const TIP = { x: 4, y: 2 };

/** Adds the cursor above everything. Added after the scene is quiet, so readiness never waits on it. */
export async function installCursor(page) {
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
    { id: CURSOR_ID, glide: GLIDE_MS, start: { x: VIEWPORT.width / 2, y: VIEWPORT.height * 0.6 } },
  );
}

/** Takes the cursor away, for a picture that is not of a person using MixLab. */
export async function removeCursor(page) {
  await page.evaluate((id) => document.getElementById(id)?.remove(), CURSOR_ID);
}

async function glideTo(page, locator) {
  await locator.scrollIntoViewIfNeeded();
  const box = await locator.boundingBox();
  if (box === null) throw new Error("the element is not visible");
  const x = box.x + box.width / 2;
  const y = box.y + box.height / 2;
  await page.evaluate(
    ({ id, x, y }) => {
      document.getElementById(id).style.transform = `translate(${x}px, ${y}px)`;
    },
    { id: CURSOR_ID, x: x - TIP.x, y: y - TIP.y },
  );
  // The real pointer follows, so hover states show on film the way they would for a person — during
  // the glide, not after it: moved first, its steps alone doubled every glide.
  await Promise.all([page.mouse.move(x, y, { steps: 6 }), page.waitForTimeout(GLIDE_MS + 50)]);
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

export async function runStep(page, step) {
  if ("pause" in step) {
    await page.waitForTimeout(step.pause);
    return;
  }
  if ("waitFor" in step) {
    await page.locator(step.waitFor).waitFor({ state: "visible", timeout: WAIT_FOR_MS });
    return;
  }
  const target = page.locator(step.click ?? step.type ?? step.select);
  await glideTo(page, target);
  await press(page);
  await target.click();
  if ("type" in step) {
    await target.pressSequentially(step.text, { delay: TYPE_DELAY_MS });
  } else if ("select" in step) {
    await page.waitForTimeout(OPTION_PAUSE_MS);
    const option = page.getByRole("option", { name: step.option, exact: true });
    await glideTo(page, option);
    await press(page);
    await option.click();
  }
}

/** Starts a CDP screencast; `stop()` returns every frame with its timestamp, and when it stopped. */
export async function startRecording(page) {
  const session = await page.context().newCDPSession(page);
  const frames = [];
  let lastArrived = 0;
  session.on("Page.screencastFrame", ({ data, metadata, sessionId }) => {
    frames.push({ data: Buffer.from(data, "base64"), t: metadata.timestamp });
    lastArrived = performance.now();
    session.send("Page.screencastFrameAck", { sessionId }).catch(() => {});
  });
  await session.send("Page.startScreencast", {
    format: "png",
    maxWidth: VIEWPORT.width * SCALE,
    maxHeight: VIEWPORT.height * SCALE,
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
