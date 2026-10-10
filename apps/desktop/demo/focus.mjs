/**
 * Measuring, while a clip films, what frames each step: the container, where to look inside it,
 * and what has to be seen. Raw — CSS pixels, wall clock; `demo/shots.mjs` makes the camera.
 * The design is docs/specs/2026-10-10-demo-clip-camera-shots-design.md.
 */
import { wallClock } from "./camera.mjs";
import { describeStep } from "./clips.mjs";

/* About two frames of the clip: a sample's time is when it saw a change, so this is how late. */
const INTERVAL_MS = 80;
/* What was added to a container this recently is where to look. */
const APPEARED_MS = 600;

/* Runs in the page once, before filming: remembers each element added, and when. */
function watchAppearances() {
  window.__demoAppeared = [];
  new MutationObserver((records) => {
    const now = performance.now();
    for (const record of records) {
      for (const node of record.addedNodes) if (node.nodeType === 1) window.__demoAppeared.push({ node, at: now });
    }
    window.__demoAppeared = window.__demoAppeared.filter((added) => added.at >= now - 2000);
  }).observe(document.body, { childList: true, subtree: true });
}

/* Runs in the page. Returns { browser, container, point, need } or { missingOverride }. */
function measure({ target, next, override, previous, last, appearedMs }) {
  const rect = (el) => {
    const r = el.getBoundingClientRect();
    return { x: r.left, y: r.top, w: r.width, h: r.height };
  };
  const union = (rects) => {
    const real = rects.filter((r) => r && r.w > 0 && r.h > 0);
    if (real.length === 0) return null;
    const x = Math.min(...real.map((r) => r.x));
    const y = Math.min(...real.map((r) => r.y));
    return { x, y, w: Math.max(...real.map((r) => r.x + r.w)) - x, h: Math.max(...real.map((r) => r.y + r.h)) - y };
  };
  // A block element spans its row whatever it holds; `data-demo-fit="text"` measures its words.
  const textRect = (el) => {
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    const rects = [];
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      if (!node.textContent.trim()) continue;
      const range = document.createRange();
      range.selectNodeContents(node);
      const r = range.getBoundingClientRect();
      rects.push({ x: r.left, y: r.top, w: r.width, h: r.height });
    }
    // Icons are content too: a row's folder, a path's copy button.
    for (const graphic of el.querySelectorAll("svg, img")) rects.push(rect(graphic));
    return union(rects) ?? rect(el);
  };
  const boxOf = (el) => (el.dataset.demoFit === "text" ? textRect(el) : rect(el));
  const blockNamed = (value) => ({
    id: `block:${value}`,
    kind: "block",
    name: value,
    elements: [...document.querySelectorAll(`[data-demo-focus="${CSS.escape(value)}"]`)],
  });
  const dialogContainer = (dialog) => {
    if (!dialog.dataset.demoCid) dialog.dataset.demoCid = String((window.__demoCid = (window.__demoCid ?? 0) + 1));
    return { id: `dialog:${dialog.dataset.demoCid}`, kind: "dialog", name: dialog.getAttribute("aria-label") ?? "", elements: [dialog] };
  };
  const containerOf = (el) => {
    const dialog = el.closest('[role="dialog"]');
    if (dialog) return dialogContainer(dialog);
    const block = el.closest("[data-demo-focus]");
    return block ? blockNamed(block.dataset.demoFocus) : null;
  };
  const byId = (id) => {
    if (id.startsWith("dialog:")) {
      const dialog = document.querySelector(`[data-demo-cid="${id.slice(7)}"]`);
      return dialog ? dialogContainer(dialog) : null;
    }
    const found = blockNamed(id.slice(6));
    return found.elements.length > 0 ? found : null;
  };

  if (document.getElementById("__demo-browser")) return { browser: true, container: null, point: null, need: null };
  if (override === "full") return { browser: false, container: null, point: null, need: null };

  let container;
  let el = null;
  if (override) {
    const elements = [...document.querySelectorAll(override)];
    if (elements.length === 0) return { missingOverride: true };
    const value = elements[0].dataset.demoFocus;
    container =
      value !== undefined && elements.every((e) => e.dataset.demoFocus === value)
        ? blockNamed(value)
        : { id: `focus:${override}`, kind: "block", name: override, elements };
  } else {
    // A target that is gone, or covered by a modal it is not in, has done its work: look ahead.
    const modal = [...document.querySelectorAll('[role="dialog"][aria-modal="true"]')].at(-1) ?? null;
    const usable = (selector) => {
      const found = selector ? document.querySelector(selector) : null;
      return found && (modal === null || modal.contains(found)) ? found : null;
    };
    el = usable(target) ?? usable(next);
    if (el === null) {
      // Nothing to act on yet — a job still running before its Close appears — while what framed
      // the last sample is still on screen: keep it, rather than cut to the whole frame and back.
      const kept = last ? byId(last) : null;
      const visible = kept !== null && (modal === null || kept.elements.some((e) => e === modal || modal.contains(e)));
      if (!visible) return { browser: false, container: null, point: null, need: null };
      container = kept;
    } else {
      // A listbox or menu is a portal: its item is in whatever framed the step before.
      container = el.closest('[role="listbox"], [role="menu"]') && previous ? byId(previous) : containerOf(el);
    }
    if (container === null) return { browser: false, container: null, point: null, need: null };
  }

  const box = union(container.elements.map(boxOf));
  if (box === null) return { browser: false, container: null, point: null, need: null };
  const since = performance.now() - appearedMs;
  const appeared = union(
    (window.__demoAppeared ?? [])
      .filter((added) => added.at >= since && added.node.isConnected && container.elements.some((c) => c.contains(added.node)))
      .map((added) => rect(added.node)),
  );
  const point = appeared ?? (el ? rect(el) : null);
  // What appeared steers where to look, not how close: a plan or a log is tall, and letting it set
  // the narrow zoom would hold a phone's whole shot at the zoom of the tallest thing in it.
  const need = el ? rect(el.closest('label, tr, li, [role="option"]') ?? el) : null;
  const { elements, ...described } = container;
  const framing = { ...described, box };
  // A dialog's title, which a phone's view never cuts.
  if (container.kind === "dialog") {
    const heading = elements[0].querySelector("h1, h2, h3");
    if (heading) framing.title = textRect(heading);
  }
  // A block's own zoom cap.
  const caps = elements.map((e) => Number.parseFloat(e.dataset.demoZoomMax)).filter((cap) => Number.isFinite(cap));
  if (caps.length > 0) framing.zoomMax = Math.max(...caps);
  // The pane a view stays inside — the screen beside the sidebar.
  const pane = elements[0].closest("[data-demo-bounds]");
  if (pane) framing.bounds = rect(pane);
  // The text of the table the container sits in, column by column, which a view's edge never cuts.
  const table = elements[0].closest("table");
  if (table) {
    framing.columns = [...table.querySelectorAll("th, td")]
      .map((cell) => textRect(cell))
      .filter((r) => r.w > 0)
      .map((r) => [r.x, r.x + r.w]);
  }
  return { browser: false, container: framing, point, need };
}

/** The target a step acts on, for measuring; a pause keeps the one before it. */
function targetOf(step) {
  return step.click ?? step.type ?? step.select ?? step.waitFor ?? null;
}

/** `steps` are the clip's, so a step whose target is gone can look at the next one's. */
export function createSampler(page, steps) {
  const samples = [];
  const problems = [];
  const framed = new Map();
  let current = null;
  let timer = null;
  let stopped = false;
  let last = null;

  function previousContainer(step) {
    let found = null;
    for (const [index, id] of framed) if (index < step) found = id;
    return found;
  }

  async function sample() {
    if (current === null) return;
    const { step, label, target, next, override } = current;
    const before = wallClock();
    const result = await page
      .evaluate(measure, { target, next, override, previous: previousContainer(step), last, appearedMs: APPEARED_MS })
      .catch(() => null);
    const at = (before + wallClock()) / 2;
    // A measurement still running when filming stopped is about a moment that is not on film.
    if (stopped || result === null) return;
    if (result.missingOverride) {
      problems.push(`step ${step}: focus ${override} matches nothing`);
      return;
    }
    if (result.container && !framed.has(step)) framed.set(step, result.container.id);
    last = result.container?.id ?? null;
    samples.push({ at, step, label, ...result });
  }

  return {
    /** Before filming: start remembering what appears in the page. */
    async install() {
      await page.evaluate(watchAppearances);
    },
    /** A step begins: from now on samples belong to it. Measures once before it runs. */
    async enter(index, step) {
      const target = targetOf(step) ?? current?.target ?? null;
      const next = steps.slice(index + 1).map(targetOf).find((selector) => selector !== null) ?? null;
      current = { step: index, label: describeStep(step), target, next, override: step.focus ?? null };
      await sample();
    },
    sample,
    start() {
      // A tick that finds the last one still measuring skips, so measurements never pile up.
      let measuring = false;
      timer = setInterval(() => {
        if (measuring) return;
        measuring = true;
        void sample().finally(() => (measuring = false));
      }, INTERVAL_MS);
    },
    stop() {
      clearInterval(timer);
      current = null;
      stopped = true;
      return { samples: [...samples].sort((a, b) => a.at - b.at), problems: [...problems] };
    },
  };
}
