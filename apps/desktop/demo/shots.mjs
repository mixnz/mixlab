/**
 * The website's camera, version 2: shots that hold one zoom and pan inside them, with every action
 * — the target a step touches and the cursor reaching it — in view.
 * Pure; `demo/focus.mjs` and `demo/recorder.mjs` measure, `demo/clip.mjs` writes.
 * The design is docs/specs/2026-10-10-demo-clip-camera-shots-design.md.
 */
import { normalize, toVideoTime } from "./camera.mjs";

/** Seconds a shot, or a pan, has to hold to be kept. */
export const MIN_HOLD = 1.2;
/** Seconds a whole-frame shot between two zoomed ones has to last to be kept. */
export const MIN_FULL_BETWEEN = 1.5;
/** Fraction of the frame left around a container when its zoom is chosen. */
export const VIEW_MARGIN = 0.04;
/** How near an edge of the view, as a share of its height, the point may come before a pan. */
export const EDGE = 0.08;
/** Where a pan puts the point, as a share of the view's height from its top. */
export const AIM = 0.4;
/** Two views closer than this on x and y, at one zoom, are one view. */
export const SAME_VIEW = 0.03;
export const MAX_ZOOM = { wide: 2, narrow: 2 };
/** Share of a container's width a narrow view keeps. */
export const NARROW_WIDTH = 0.9;
/** Space left of a container a narrow view keeps when it is anchored to the container's left. */
export const NARROW_LEFT = 0.02;
/** How far inside a view, as a share of its size, an action's reach has to stay. */
export const REACH_INSET = 0.04;
/** Seconds the website's pan takes: an action's pan lands this long before its cursor moves. */
export const PAN_LEAD = 0.5;

const round = (value) => Math.round(value * 1000) / 1000;
const clamp = (value, low, high) => Math.min(high, Math.max(low, value));
const EPS = 1e-9;
/* Views are written to three decimals; a reach checked against a rounded view gets that much slack. */
const SLACK = 0.002;

function unionPx(rects) {
  const x = Math.min(...rects.map((r) => r.x));
  const y = Math.min(...rects.map((r) => r.y));
  return { x, y, w: Math.max(...rects.map((r) => r.x + r.w)) - x, h: Math.max(...rects.map((r) => r.y + r.h)) - y };
}

/** A raw sample — CSS pixels, wall clock — on the video's time, in fractions of the frame. */
export function normalizeSample(raw, { viewport, start, duration }) {
  const fraction = (rect) => (rect ? normalize(rect, viewport) : null);
  let container = null;
  if (!raw.browser && raw.container) {
    const c = raw.container;
    container = { ...c, box: fraction(c.box) };
    if (c.title) container.title = fraction(c.title);
    if (c.bounds) container.bounds = fraction(c.bounds);
    if (c.columns) container.columns = c.columns.map(([left, right]) => [round(left / viewport.width), round(right / viewport.width)]);
  }
  return {
    t: toVideoTime(raw.at, start, duration),
    step: raw.step,
    label: raw.label,
    browser: raw.browser === true,
    container,
    point: fraction(raw.point),
    need: fraction(raw.need),
  };
}

/** A raw action — CSS pixels, wall clock — on the video's time; its reach holds target and path. */
export function normalizeAction(raw, { viewport, start, duration }) {
  const at = (p) => ({ x: round(p.x / viewport.width), y: round(p.y / viewport.height) });
  const path = { x: Math.min(raw.from.x, raw.to.x), y: Math.min(raw.from.y, raw.to.y), w: Math.abs(raw.to.x - raw.from.x), h: Math.abs(raw.to.y - raw.from.y) };
  return {
    start: toVideoTime(raw.start, start, duration),
    end: toVideoTime(raw.end, start, duration),
    from: at(raw.from),
    to: at(raw.to),
    target: normalize(raw.target, viewport),
    reach: normalize(unionPx([raw.target, path]), viewport),
    step: raw.step,
    label: raw.label,
  };
}

/** Where the cursor is at `t`: along an action's path while it glides, else where the last left it. */
export function cursorAt(actions, t, glide) {
  if (actions.length === 0) return null;
  let position = actions[0].from;
  for (const action of actions) {
    if (t < action.start - EPS) break;
    const along = glide > 0 ? clamp((t - action.start) / glide, 0, 1) : 1;
    position = {
      x: round(action.from.x + (action.to.x - action.from.x) * along),
      y: round(action.from.y + (action.to.y - action.from.y) * along),
    };
  }
  return position;
}

/** Consecutive samples in one container are a shot; samples in none are a whole-frame shot. */
export function splitShots(samples) {
  const shots = [];
  for (const sample of samples) {
    const id = sample.container?.id ?? null;
    const last = shots.at(-1);
    if (last !== undefined && last.id === id) {
      last.samples.push(sample);
      last.browser ||= sample.browser;
      continue;
    }
    shots.push({
      id,
      kind: sample.container?.kind ?? null,
      name: sample.container?.name ?? null,
      browser: sample.browser,
      start: sample.t,
      samples: [sample],
    });
  }
  return shots;
}

const lengthOf = (shots, index, duration) => (index + 1 < shots.length ? shots[index + 1].start : duration) - shots[index].start;

/**
 * Shots short enough to be a flicker folded away, until nothing changes: neighbours on one
 * container merge; a shot under `MIN_HOLD` joins the one before (an opening one becomes the
 * whole-frame lead-in); a whole-frame shot under `MIN_FULL_BETWEEN` between two zoomed ones joins
 * the one before. A folded shot's samples are dropped: they were measured in another container.
 */
export function settleShots(input, duration) {
  const shots = input.map((shot) => ({ ...shot, samples: [...shot.samples] }));
  for (let changed = true; changed; ) {
    changed = false;
    for (let i = 1; i < shots.length && !changed; i++) {
      if (shots[i].id !== shots[i - 1].id) continue;
      shots[i - 1].samples.push(...shots[i].samples);
      shots[i - 1].browser ||= shots[i].browser;
      shots.splice(i, 1);
      changed = true;
    }
    for (let i = 0; i < shots.length && !changed; i++) {
      const length = lengthOf(shots, i, duration);
      const between = i > 0 && i + 1 < shots.length && shots[i - 1].id !== null && shots[i + 1].id !== null;
      const short =
        length < MIN_HOLD - EPS || (shots[i].id === null && between && length < MIN_FULL_BETWEEN - EPS);
      if (!short) continue;
      if (i === 0) {
        if (shots[0].id === null) continue;
        shots[0] = { id: null, kind: null, name: null, browser: false, start: shots[0].start, samples: [] };
      } else {
        shots.splice(i, 1);
      }
      changed = true;
    }
  }
  return shots;
}

/** A zoom no closer than lets every reach fit inside the view's inset. */
function fitReaches(s, actions) {
  let zoom = s;
  for (const action of actions) zoom = Math.min(zoom, (1 - 2 * REACH_INSET) / Math.max(action.reach.w, action.reach.h));
  return Math.max(1, zoom);
}

/** Wide: the container's widest box over the shot decides, up to the block's own cap if it has one. */
export function wideZoom(shot, actions = []) {
  const widest = Math.max(...shot.samples.map((s) => s.container.box.w));
  const caps = shot.samples.map((s) => s.container.zoomMax).filter((cap) => typeof cap === "number" && cap >= 1);
  const cap = caps.length > 0 ? Math.max(...caps) : MAX_ZOOM.wide;
  return fitReaches(clamp(1 / (widest + 2 * VIEW_MARGIN), 1, cap), actions);
}

/** Narrow: the tallest thing that has to be legible, and 90 % of the container's width. */
export function narrowZoom(shot, actions = []) {
  // A sample with nothing to read — a job running, its Close not yet there — says nothing about
  // how close to be; the container's height stands in only when no sample of the shot has a need.
  const needs = shot.samples.filter((s) => s.need).map((s) => s.need.h);
  const tallest = needs.length > 0 ? Math.max(...needs) : Math.max(...shot.samples.map((s) => s.container.box.h));
  const widest = Math.max(...shot.samples.map((s) => s.container.box.w));
  const s = Math.min(1 / (tallest + 2 * VIEW_MARGIN), 1 / (NARROW_WIDTH * widest + 2 * VIEW_MARGIN));
  return fitReaches(clamp(s, 1, MAX_ZOOM.narrow), actions);
}

/** `x` slid, at one size, so neither edge of the view crosses a column of text; nearest wins. */
function slideOffColumns(x, size, columns, low, high) {
  const crosses = (edge) => columns.some(([left, right]) => edge > left + EPS && edge < right - EPS);
  if (!crosses(x) && !crosses(x + size)) return x;
  const candidates = columns
    .flatMap(([left, right]) => [left, right, left - size, right - size])
    .filter((c) => c >= low - EPS && c <= high + EPS && !crosses(c) && !crosses(c + size));
  if (candidates.length === 0) return x;
  return candidates.reduce((best, c) => (Math.abs(c - x) < Math.abs(best - x) ? c : best));
}

/** `y` moved so a title is wholly in the view or wholly out of it. */
function keepTitleWhole(y, size, title) {
  const top = y;
  const bottom = y + size;
  if (title.y < top - EPS && title.y + title.h > top + EPS) return title.y;
  if (title.y < bottom - EPS && title.y + title.h > bottom + EPS) return title.y + title.h - size;
  return y;
}

/** The view at every sample of a zoomed shot: placed, kept in bounds, panned with hysteresis. */
export function shotViews(shot, track, s) {
  const size = 1 / s;
  const views = [];
  let y = null;
  for (const sample of shot.samples) {
    const container = sample.container;
    const box = container.box;
    const point = sample.point ?? box;
    let x = track === "narrow" && box.w > size + EPS ? box.x - NARROW_LEFT : box.x + box.w / 2 - size / 2;
    let low = 0;
    let high = 1 - size;
    if (container.bounds && container.bounds.w >= size - EPS) {
      low = Math.max(low, container.bounds.x);
      high = Math.min(high, container.bounds.x + container.bounds.w - size);
    }
    x = clamp(x, low, high);
    if (container.columns) x = slideOffColumns(x, size, container.columns, low, high);
    if (box.h <= size) {
      y = box.y + box.h / 2 - size / 2;
    } else {
      const holds = y !== null && point.y >= y + EDGE * size && point.y + point.h <= y + size - EDGE * size;
      if (!holds) y = point.y + point.h / 2 - AIM * size;
      y = clamp(y, box.y, box.y + box.h - size);
    }
    if (track === "narrow" && container.title) y = keepTitleWhole(y, size, container.title);
    y = clamp(y, 0, 1 - size);
    views.push({ t: sample.t, view: { x: round(x), y: round(y), s: round(s) }, label: sample.label });
  }
  return views;
}

/** A dialog's title where it is at `t` — it moves as the dialog grows — or undefined. */
function titleAt(shot, t) {
  let title;
  for (const sample of shot?.samples ?? []) {
    if (!sample.container?.title) continue;
    if (title !== undefined && sample.t > t + EPS) break;
    title = sample.container.title;
  }
  return title;
}

/** The actions whose windows open during shot `index`. */
function actionsIn(shots, index, actions) {
  const from = shots[index].start;
  const until = index + 1 < shots.length ? shots[index + 1].start : Infinity;
  return actions.filter((a) => a.start >= from - EPS && a.start < until - EPS);
}

/** Every shot's views, its first labelled by what it frames and the rest as pans. */
export function trackFor(shots, track, actions = []) {
  const entries = [];
  shots.forEach((shot, index) => {
    if (shot.id === null || shot.samples.length === 0) {
      entries.push({ t: shot.start, view: "full", label: shot.browser ? "browser opened" : "full", shotStart: true });
      return;
    }
    let inShot = actionsIn(shots, index, actions);
    if (track === "narrow") {
      // An action crossing the title's rows cannot leave the title wholly out, so the narrow view
      // must be wide enough to hold the title whole beside it.
      inShot = inShot.map((a) => {
        const title = titleAt(shot, a.start);
        if (!title) return a;
        const r = a.reach;
        if (r.y > title.y + title.h || r.y + r.h < title.y) return a;
        const x = Math.min(r.x, title.x);
        const y = Math.min(r.y, title.y);
        return { ...a, reach: { x, y, w: Math.max(r.x + r.w, title.x + title.w) - x, h: Math.max(r.y + r.h, title.y + title.h) - y } };
      });
    }
    const s = track === "wide" ? wideZoom(shot, inShot) : narrowZoom(shot, inShot);
    shotViews(shot, track, s).forEach((view, i) =>
      entries.push(
        i === 0
          ? { t: shot.start, view: view.view, label: `${shot.kind}: ${shot.name}`, shotStart: true }
          : { t: view.t, view: view.view, label: `pan: ${view.label}`, shotStart: false },
      ),
    );
  });
  return entries;
}

function sameView(a, b) {
  if (a === "full" || b === "full") return a === b;
  return a.s === b.s && Math.abs(a.x - b.x) < SAME_VIEW - EPS && Math.abs(a.y - b.y) < SAME_VIEW - EPS;
}

/**
 * Views that change nothing, and pans too brief to watch, dropped until nothing changes. A view
 * held for an action (`held`) is never dropped: an action in view outranks every rule here.
 */
export function settleViews(input, duration) {
  let entries = [...input];
  for (let changed = true; changed; ) {
    changed = false;
    const kept = [];
    for (const entry of entries) {
      const last = kept.at(-1);
      const same = last === undefined ? entry.view === "full" : sameView(last.view, entry.view);
      if (same && entry.held === undefined) {
        changed = true;
        continue;
      }
      kept.push(entry);
    }
    entries = kept;
    for (let i = 0; i < entries.length && !changed; i++) {
      if (entries[i].held !== undefined) continue;
      const until = i + 1 < entries.length ? entries[i + 1].t : duration;
      if (until - entries[i].t >= MIN_HOLD - EPS) continue;
      if (!entries[i].shotStart) {
        entries.splice(i, 1);
        changed = true;
      } else if (i + 1 < entries.length && !entries[i + 1].shotStart && entries[i + 1].held === undefined) {
        // A shot's first view gone in a moment — a dialog easing in, then tall — starts the shot
        // where its first pan would have taken it.
        entries[i] = { ...entries[i], view: entries[i + 1].view };
        entries.splice(i + 1, 1);
        changed = true;
      }
    }
  }
  return entries;
}

/** Whether a view holds a reach inside its 4 % inset. The whole frame holds everything. */
export function contains(view, reach) {
  if (view === "full") return true;
  const size = 1 / view.s;
  const inset = REACH_INSET * size - SLACK;
  return (
    reach.x >= view.x + inset &&
    reach.y >= view.y + inset &&
    reach.x + reach.w <= view.x + size - inset &&
    reach.y + reach.h <= view.y + size - inset
  );
}

/** The view moved the least that holds a reach inside its inset, kept in the frame. */
function moveToContain(view, reach) {
  const size = 1 / view.s;
  const inset = REACH_INSET * size;
  const x = clamp(clamp(view.x, reach.x + reach.w + inset - size, reach.x - inset), 0, 1 - size);
  const y = clamp(clamp(view.y, reach.y + reach.h + inset - size, reach.y - inset), 0, 1 - size);
  return { x: round(x), y: round(y), s: view.s };
}

/**
 * Narrow only: a view that would cut a dialog's title across holds the title whole with the reach
 * when both fit, else drops below (or above) it — the title wholly out, the reach still in.
 */
function clearTitle(view, reach, title) {
  if (!title || view === "full") return view;
  const size = 1 / view.s;
  const overlaps = title.y < view.y + size - EPS && title.y + title.h > view.y + EPS;
  const cut = title.x < view.x - EPS || title.x + title.w > view.x + size + EPS;
  if (!overlaps || !cut) return view;
  const both = {
    x: Math.min(reach.x, title.x),
    y: Math.min(reach.y, title.y),
    w: Math.max(reach.x + reach.w, title.x + title.w) - Math.min(reach.x, title.x),
    h: Math.max(reach.y + reach.h, title.y + title.h) - Math.min(reach.y, title.y),
  };
  const room = size * (1 - 2 * REACH_INSET);
  if (both.w <= room && both.h <= room) return moveToContain(view, both);
  const below = { ...view, y: Math.ceil(Math.min(1 - size, title.y + title.h) * 1000) / 1000 };
  if (contains(below, reach)) return below;
  const above = { ...view, y: Math.floor(Math.max(0, title.y - size) * 1000) / 1000 };
  if (contains(above, reach)) return above;
  return view;
}

const inForce = (entries, t) => {
  let index = -1;
  entries.forEach((entry, i) => {
    if (entry.t <= t + EPS) index = i;
  });
  return index;
};

/**
 * Every action in view, above every anti-jitter rule: where the view in force when an action's
 * window opens does not hold its reach, a pan lands `PAN_LEAD` before — never before its shot's
 * start (that shot's first view moves instead) nor before the previous window's end. A view that
 * holds a reach is marked `held` through that window, and a view that would replace it inside the
 * window is dropped.
 */
export function frameActions(input, shots, actions, track = "wide") {
  let entries = input.map((entry) => ({ ...entry }));
  const notes = [];
  let previousEnd = -Infinity;
  for (const action of [...actions].sort((a, b) => a.start - b.start)) {
    const shotIndex = inForce(shots.map((shot) => ({ t: shot.start })), action.start);
    const shot = shots[shotIndex];
    const index = inForce(entries, action.start);
    const current = entries[index];
    if (shot === undefined || shot.id === null || current === undefined || current.view === "full") {
      previousEnd = Math.max(previousEnd, action.end);
      continue;
    }
    const titleOf = (of, t) => (track === "narrow" ? titleAt(of, t) : undefined);
    const title = titleOf(shot, action.start);
    let holder = current;
    if (!contains(current.view, action.reach)) {
      const view = clearTitle(moveToContain(current.view, action.reach), action.reach, title);
      const ideal = action.start - PAN_LEAD;
      const t = Math.max(ideal, shot.start, previousEnd);
      const first = entries.find((entry) => entry.shotStart && Math.abs(entry.t - shot.start) < EPS);
      if (t <= shot.start + EPS && first !== undefined) {
        first.view = view;
        holder = first;
      } else {
        // Short by more than a few milliseconds of rounding: worth a line in the report.
        if (t > ideal + 0.005 && t > shot.start + EPS) {
          notes.push(`step ${action.step} (${action.label}): pan ${(action.start - t).toFixed(2)} s before the cursor, not ${PAN_LEAD}`);
        }
        // Rounded up: rounded down, a pan made at the previous window's end lands back inside it.
        holder = { t: Math.ceil(t * 1000 - 1e-6) / 1000, view, label: `pan: ${action.label}`, shotStart: false };
        entries.push(holder);
        entries.sort((a, b) => a.t - b.t);
      }
    }
    holder.held = Math.max(holder.held ?? 0, action.end);
    // Nothing replaces the view inside the window but the next shot's start.
    entries = entries.filter(
      (entry) =>
        entry === holder ||
        entry.shotStart ||
        entry.t <= holder.t + EPS ||
        entry.t > action.end + EPS ||
        (entry.held !== undefined && entry.held >= action.start),
    );
    // A shot that starts inside the window — a dialog seen a moment after the cursor set off for a
    // button in it — holds the reach too.
    for (const entry of entries) {
      if (!entry.shotStart || entry.view === "full" || entry.t <= holder.t + EPS || entry.t > action.end + EPS) continue;
      // The title is the opening shot's own, not the one the window began in.
      const opening = shots.find((s) => Math.abs(s.start - entry.t) < EPS);
      if (!contains(entry.view, action.reach)) entry.view = clearTitle(moveToContain(entry.view, action.reach), action.reach, titleOf(opening, entry.t));
      entry.held = Math.max(entry.held ?? 0, action.end);
    }
    previousEnd = Math.max(previousEnd, action.end);
  }
  return { entries, notes };
}

/** Each action, in one track: whether every view in force during its window holds its reach. */
export function checkTrack(entries, actions) {
  return actions.map((action) => {
    const from = inForce(entries, action.start);
    const views = entries.filter((entry, i) => i === from || (entry.t > action.start + EPS && entry.t <= action.end + EPS));
    const ok = views.length > 0 && views.every((entry) => contains(entry.view, action.reach));
    return { step: action.step, label: action.label, ok: views.length === 0 ? true : ok };
  });
}

const strip = (entries) => entries.map(({ t, view, label }) => ({ t: round(t), view, label }));

/**
 * Normalized samples and actions to the two tracks the website plays, how many shots they hold,
 * each action's check in each track, and what the run should say about late pans.
 */
export function cameraTracks(samples, duration, actions = []) {
  const shots = settleShots(splitShots(samples), duration);
  const notes = [];
  const build = (track) => {
    const first = settleViews(trackFor(shots, track, actions), duration);
    const framed = frameActions(first, shots, actions, track);
    const entries = settleViews(framed.entries, duration);
    notes.push(...framed.notes.map((note) => `${track}: ${note}`));
    return { entries, checks: checkTrack(entries, actions) };
  };
  const wide = build("wide");
  const narrow = build("narrow");
  return {
    wide: strip(wide.entries),
    narrow: strip(narrow.entries),
    shots: shots.length,
    checks: { wide: wide.checks, narrow: narrow.checks },
    notes,
  };
}

/** How many times a track changes its zoom, and how many times it pans at one zoom. */
export function trackStats(track) {
  let zoom = 1;
  let zooms = 0;
  let pans = 0;
  for (const entry of track) {
    const next = entry.view === "full" ? 1 : entry.view.s;
    if (next !== zoom) {
      zooms++;
      zoom = next;
    } else if (entry.view !== "full") {
      pans++;
    }
  }
  return { zooms, pans };
}

/** The view in force at `t`: the whole frame before the first entry. */
export function viewAt(track, t) {
  let view = "full";
  for (const entry of track) if (entry.t <= t + EPS) view = entry.view;
  return view;
}

export function cameraFile({ clip, duration, frame, wide, narrow }) {
  return { version: 2, clip, duration: round(duration), frame, wide, narrow };
}
