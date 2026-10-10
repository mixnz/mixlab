import { describe, expect, it } from "vitest";
import {
  cameraFile,
  cameraTracks,
  checkTrack,
  contains,
  cursorAt,
  frameActions,
  narrowZoom,
  normalizeAction,
  normalizeSample,
  settleShots,
  settleViews,
  shotViews,
  splitShots,
  trackFor,
  trackStats,
  viewAt,
  wideZoom,
} from "./shots.mjs";

const box = (x, y, w, h) => ({ x, y, w, h });
const dialog = (id, b) => ({ id, kind: "dialog", name: `Dialog ${id}`, box: b });
const block = (id, b) => ({ id, kind: "block", name: id, box: b });
const sample = (t, container, point = null, need = null, extra = {}) => ({
  t, step: 0, label: "step", browser: false, container, point, need, ...extra,
});
/** Samples every 0.1 s from `from` to `to`, all in one container. */
const span = (from, to, container, point = null) => {
  const out = [];
  for (let t = from; t < to - 1e-9; t += 0.1) out.push(sample(Math.round(t * 1000) / 1000, container, point));
  return out;
};
const shotOf = (start, container, samples) => ({
  id: container?.id ?? null, kind: container?.kind ?? null, name: container?.name ?? null,
  browser: false, start, samples,
});

describe("normalizeSample", () => {
  it("puts a raw sample on the video's time, in fractions of the frame", () => {
    const raw = {
      at: 1001.5, step: 2, label: "click x", browser: false,
      container: { id: "dialog:1", kind: "dialog", name: "Edit", box: box(128, 80, 640, 400) },
      point: box(128, 80, 64, 40), need: null,
    };
    expect(normalizeSample(raw, { viewport: { width: 1280, height: 800 }, start: 1000, duration: 10 })).toEqual({
      t: 1.5, step: 2, label: "click x", browser: false,
      container: { id: "dialog:1", kind: "dialog", name: "Edit", box: box(0.1, 0.1, 0.5, 0.5) },
      point: box(0.1, 0.1, 0.05, 0.05), need: null,
    });
  });

  it("drops the container while the browser overlay is up", () => {
    const raw = { at: 1002, step: 8, label: "w", browser: true, container: null, point: null, need: null };
    expect(normalizeSample(raw, { viewport: { width: 1280, height: 800 }, start: 1000, duration: 10 }).container).toBeNull();
  });
});

describe("splitShots", () => {
  it("cuts on the container's identity, a missing one being the whole frame", () => {
    const a = dialog("dialog:1", box(0.2, 0.2, 0.5, 0.3));
    const shots = splitShots([sample(1, a), sample(1.1, a), sample(2, null), sample(3, block("block:x", box(0, 0, 1, 1)))]);
    expect(shots.map((s) => [s.id, s.start, s.samples.length])).toEqual([
      ["dialog:1", 1, 2], [null, 2, 1], ["block:x", 3, 1],
    ]);
  });

  it("keeps a dialog that grows as one shot", () => {
    const grow = [0.3, 0.5, 0.8].map((h, i) => sample(1 + i * 0.1, dialog("dialog:1", box(0.25, 0.1, 0.5, h))));
    expect(splitShots(grow)).toHaveLength(1);
  });
});

describe("settleShots", () => {
  const A = dialog("A", box(0.25, 0.2, 0.5, 0.3));
  const B = dialog("B", box(0.25, 0.2, 0.5, 0.3));
  const C = dialog("C", box(0.25, 0.2, 0.5, 0.3));

  it("drops an opening shot under 1.2 s into the whole frame", () => {
    const shots = settleShots([shotOf(0.6, A, [sample(0.6, A)]), shotOf(1.3, B, [sample(1.3, B)])], 10);
    expect(shots.map((s) => s.id)).toEqual([null, "B"]);
  });

  it("folds a short shot mid-clip into the one before, and merges what that joins", () => {
    const shots = settleShots(
      [shotOf(0.6, A, [sample(0.6, A)]), shotOf(3, B, [sample(3, B)]), shotOf(4, A, [sample(4, A)]), shotOf(8, C, [sample(8, C)])],
      12,
    );
    expect(shots.map((s) => [s.id, s.start])).toEqual([["A", 0.6], ["C", 8]]);
    expect(shots[0].samples.map((s) => s.t)).toEqual([0.6, 4]);
  });

  it("folds a whole-frame shot under 1.5 s between two zoomed ones, and keeps one of 1.6 s", () => {
    const short = settleShots([shotOf(0.6, A, [sample(0.6, A)]), shotOf(3, null, [sample(3, null)]), shotOf(4.4, B, [sample(4.4, B)])], 10);
    expect(short.map((s) => s.id)).toEqual(["A", "B"]);
    const long = settleShots([shotOf(0.6, A, [sample(0.6, A)]), shotOf(3, null, [sample(3, null)]), shotOf(4.6, B, [sample(4.6, B)])], 10);
    expect(long.map((s) => s.id)).toEqual(["A", null, "B"]);
  });

  it("keeps a short opening whole-frame shot: it is the lead-in", () => {
    const shots = settleShots([shotOf(0.6, null, [sample(0.6, null)]), shotOf(1.0, A, [sample(1.0, A)])], 10);
    expect(shots.map((s) => s.id)).toEqual([null, "A"]);
  });
});

describe("zoom", () => {
  it("wide: from the widest box only, clamped to 1 and 2", () => {
    const shot = (...widths) => ({ samples: widths.map((w, i) => sample(i, dialog("A", box(0, 0, w, 0.2 + i * 0.3)))) });
    expect(wideZoom(shot(0.5))).toBeCloseTo(1 / 0.58);
    expect(wideZoom(shot(0.5, 0.5))).toBeCloseTo(1 / 0.58); // taller, not wider: the same zoom
    expect(wideZoom(shot(0.4, 0.5))).toBeCloseTo(1 / 0.58);
    expect(wideZoom(shot(0.95))).toBe(1);
    expect(wideZoom(shot(0.2))).toBe(2);
  });

  it("wide: a block's own cap replaces 2", () => {
    const open = { ...block("block:project-open", box(0.223, 0.335, 0.28, 0.144)), zoomMax: 3.3 };
    expect(wideZoom({ samples: [sample(1, open)] })).toBeCloseTo(1 / 0.36);
    expect(wideZoom({ samples: [sample(1, { ...open, box: box(0.2, 0.3, 0.1, 0.1) })] })).toBe(3.3);
  });

  it("narrow: legible, with 90 % of the container's width, clamped to 1 and 2.0", () => {
    const shot = (width, ...heights) => ({
      samples: heights.map((h, i) => sample(i, dialog("A", box(0, 0, width, 0.9)), box(0, 0, 0.1, 0.02), box(0, 0, 0.3, h))),
    });
    expect(narrowZoom(shot(0.494, 0.05))).toBeCloseTo(1 / (0.9 * 0.494 + 0.08)); // the width decides
    expect(narrowZoom(shot(0.494, 0.6))).toBeCloseTo(1 / 0.68); // the tallest need decides
    expect(narrowZoom(shot(0.494, 0.1, 0.6))).toBeCloseTo(1 / 0.68);
    expect(narrowZoom(shot(0.3, 0.05))).toBe(2);
    expect(narrowZoom(shot(0.494, 0.95))).toBe(1);
  });

  it("narrow: a sample with nothing to read does not stand in its container's height for one", () => {
    const d = dialog("A", box(0.253, 0.05, 0.494, 0.9));
    const shot = { samples: [sample(1, d, null, box(0, 0, 0.3, 0.05)), sample(2, d)] };
    expect(narrowZoom(shot)).toBeCloseTo(1 / (0.9 * 0.494 + 0.08));
    expect(narrowZoom({ samples: [sample(2, d)] })).toBeCloseTo(1 / 0.98); // none has one: the container's height
  });

  it("either track: lowered until every reach in the shot fits inside the view's inset", () => {
    const shot = { samples: [sample(1, dialog("A", box(0.25, 0.1, 0.5, 0.3)), null, box(0, 0, 0.1, 0.03))] };
    const reach = { reach: box(0.1, 0.1, 0.7, 0.05) };
    expect(wideZoom(shot, [reach])).toBeCloseTo(0.92 / 0.7);
    expect(narrowZoom(shot, [reach])).toBeCloseTo(0.92 / 0.7);
    expect(wideZoom(shot, [{ reach: box(0.3, 0.2, 0.1, 0.1) }])).toBeCloseTo(1 / 0.58); // a small reach changes nothing
  });
});

describe("shotViews", () => {
  const s = 1 / 0.58; // a 0.5-wide container, so the view is 0.58 of the frame
  const size = 0.58;

  it("centres a container no taller than the view", () => {
    const shot = shotOf(1, null, [sample(1, dialog("A", box(0.25, 0.3, 0.5, 0.3)), box(0.3, 0.35, 0.1, 0.05))]);
    const [v] = shotViews(shot, "wide", s);
    expect(v.view.x).toBeCloseTo(0.21);
    expect(v.view.y).toBeCloseTo(0.16);
  });

  it("pans a taller one only when the point nears an edge, then aims it at 40 %", () => {
    const tall = dialog("A", box(0.25, 0.05, 0.5, 0.9));
    const at = (t, y) => sample(t, tall, box(0.3, y, 0.1, 0.05));
    const views = shotViews(shotOf(1, tall, [at(1, 0.1), at(2, 0.4), at(3, 0.6)]), "wide", s);
    expect(views.map((v) => v.view.y)).toEqual([0.05, 0.05, 0.37]);
    // 0.6 + 0.025 − 0.4·0.58 = 0.393, held inside the container's bottom: 0.95 − 0.58 = 0.37
  });

  it("keeps the view inside the frame for a container taller than it", () => {
    const huge = dialog("A", box(0.25, -0.1, 0.5, 1.3));
    const [v] = shotViews(shotOf(1, huge, [sample(1, huge, box(0.3, 1.1, 0.1, 0.05))]), "wide", s);
    expect(v.view.y).toBeCloseTo(1 - size);
  });

  it("uses the container when there is no point", () => {
    const c = dialog("A", box(0.25, 0.3, 0.5, 0.3));
    const [v] = shotViews(shotOf(1, c, [sample(1, c)]), "wide", s);
    expect(v.view.y).toBeCloseTo(0.16);
  });

  it("narrow: anchored to the container's left less 2 % when the view is narrower than it", () => {
    const c = dialog("A", box(0.25, 0.3, 0.6, 0.3));
    const [v] = shotViews(shotOf(1, c, [sample(1, c, box(0.8, 0.35, 0.04, 0.03))]), "narrow", 2);
    expect(v.view.x).toBeCloseTo(0.23);
  });

  it("narrow: centred when the view is wider than the container", () => {
    const c = dialog("A", box(0.253, 0.3, 0.494, 0.3));
    const [v] = shotViews(shotOf(1, c, [sample(1, c, box(0.3, 0.35, 0.04, 0.03))]), "narrow", 2);
    expect(v.view.x).toBeCloseTo(0.25);
  });

  it("keeps the view inside the container's bounds, off the sidebar", () => {
    const c = { ...block("block:x", box(0.15, 0.3, 0.2, 0.1)), bounds: box(0.165, 0, 0.835, 1) };
    const [v] = shotViews(shotOf(1, c, [sample(1, c)]), "wide", 2);
    expect(v.view.x).toBeCloseTo(0.165); // centred it would start at 0
  });

  it("slides the view's edges off the table's text, keeping the zoom", () => {
    const columns = [[0.226, 0.27], [0.29, 0.5], [0.53, 0.6]];
    const c = { ...block("block:project-open", box(0.223, 0.335, 0.28, 0.144)), bounds: box(0.165, 0, 0.835, 1), columns };
    const [v] = shotViews(shotOf(1, c, [sample(1, c)]), "wide", 1 / 0.36);
    // centred it is 0.183–0.543, its right edge inside the third column; 0.17 puts it at 0.53
    expect(v.view.x).toBeCloseTo(0.17);
    expect(v.view.s).toBeCloseTo(1 / 0.36, 2);
  });

  it("narrow: never cuts a dialog's title", () => {
    const c = { ...dialog("A", box(0.25, 0.05, 0.6, 0.9)), title: box(0.26, 0.1, 0.15, 0.03) };
    const [v] = shotViews(shotOf(1, c, [sample(1, c, box(0.3, 0.3, 0.1, 0.05))]), "narrow", 2);
    expect(v.view.y).toBeCloseTo(0.1); // aimed at the point it would start at 0.125, through the title
  });
});

describe("actions", () => {
  const viewport = { width: 1280, height: 800 };

  it("are put on the video's time, their reach the target and the cursor's whole path", () => {
    const raw = {
      start: 1002, end: 1002.6, from: { x: 640, y: 480 }, to: { x: 1000, y: 700 },
      target: box(980, 690, 40, 22), step: 3, label: "click save",
    };
    const a = normalizeAction(raw, { viewport, start: 1000, duration: 10 });
    expect(a.start).toBe(2);
    expect(a.end).toBeCloseTo(2.6);
    expect(a.reach).toEqual(box(0.5, 0.6, 0.297, 0.29));
    expect(a).toMatchObject({ step: 3, label: "click save", to: { x: 0.781, y: 0.875 } });
  });

  it("place the cursor along their paths, and where the last one left it", () => {
    const actions = [
      { start: 1, end: 1.6, from: { x: 0.5, y: 0.5 }, to: { x: 0.7, y: 0.9 } },
      { start: 3, end: 3.6, from: { x: 0.7, y: 0.9 }, to: { x: 0.3, y: 0.1 } },
    ];
    expect(cursorAt(actions, 0.5, 0.45)).toEqual({ x: 0.5, y: 0.5 });
    expect(cursorAt(actions, 1.225, 0.45)).toEqual({ x: 0.6, y: 0.7 });
    expect(cursorAt(actions, 2, 0.45)).toEqual({ x: 0.7, y: 0.9 });
    expect(cursorAt([], 2, 0.45)).toBeNull();
  });
});

describe("frameActions — every action in view, above anti-jitter", () => {
  const view = (x, y, s) => ({ x, y, s });
  const start = (t, v, label = "dialog: Edit") => ({ t, view: v, label, shotStart: true });
  const pan = (t, v, label = "pan") => ({ t, view: v, label, shotStart: false });
  const act = (startAt, endAt, reach, step = 4, label = "click save") => ({ start: startAt, end: endAt, reach, step, label });
  const shots = [{ id: "d", start: 1, samples: [] }];
  const save = box(0.66, 0.88, 0.05, 0.055);

  it("pans 0.5 s before the window to a reach out of view, moving the view the least, and holds it", () => {
    const { entries } = frameActions([start(1, view(0.226, 0.276, 1.821))], shots, [act(6, 6.6, save)]);
    expect(entries).toHaveLength(2);
    expect(entries[1]).toMatchObject({ t: 5.5, view: view(0.226, 0.408, 1.821), held: 6.6 });
  });

  it("moves the shot's first view rather than pan before the shot starts", () => {
    const { entries } = frameActions([start(1, view(0.226, 0.276, 1.821))], shots, [act(1.2, 1.8, save)]);
    expect(entries).toHaveLength(1);
    expect(entries[0].view).toEqual(view(0.226, 0.408, 1.821));
  });

  it("never pans before the previous action's window has ended, and notes it", () => {
    const inside = box(0.3, 0.4, 0.05, 0.05);
    const { entries, notes } = frameActions(
      [start(1, view(0.226, 0.276, 1.821))],
      shots,
      [act(3, 3.6, inside, 3, "click pins"), act(3.8, 4.4, save)],
      10,
    );
    expect(entries[1].t).toBeCloseTo(3.6);
    expect(notes).toEqual([expect.stringMatching(/step 4 .*0\.20 s before/)]);
  });

  it("puts a pan made at the previous window's end after it, never rounded back into it", () => {
    const inside = box(0.3, 0.4, 0.05, 0.05);
    const first = act(3, 3.6004, inside, 3, "click pins");
    const { entries } = frameActions([start(1, view(0.226, 0.276, 1.821))], shots, [first, act(3.8, 4.4, save)]);
    expect(entries[1].t).toBe(3.601);
    expect(checkTrack(entries, [first])[0].ok).toBe(true);
  });

  it("drops a later view that would change the frame inside a window", () => {
    const inside = box(0.3, 0.4, 0.05, 0.05);
    const { entries } = frameActions(
      [start(1, view(0.226, 0.276, 1.821)), pan(6.2, view(0.226, 0.1, 1.821))],
      shots,
      [act(6, 6.6, inside)],
      10,
    );
    expect(entries.map((e) => e.t)).toEqual([1]);
  });

  it("moves a shot that starts inside an action's window to hold its reach", () => {
    // The cursor sets off for a button in a dialog a moment before the dialog's shot is seen.
    const twoShots = [{ id: "card", start: 0.6, samples: [] }, { id: "d", start: 3.7, samples: [] }];
    const preview = box(0.66, 0.88, 0.05, 0.055);
    const { entries } = frameActions(
      [start(0.6, view(0.165, 0, 1.198), "block: card"), start(3.7, view(0.226, 0.276, 1.821))],
      twoShots,
      [act(3.6, 4.2, preview, 3, "click preview")],
    );
    expect(entries.find((e) => e.t === 3.7)).toMatchObject({ view: view(0.226, 0.408, 1.821), held: 4.2 });
    expect(checkTrack(entries, [act(3.6, 4.2, preview)])[0].ok).toBe(true);
  });

  it("narrow: a pan that would cut a dialog's title moves below it when both cannot be held", () => {
    const title = box(0.263, 0.31, 0.12, 0.03);
    const titled = [{ id: "d", start: 1, samples: [sample(1, { ...dialog("d", box(0.253, 0.3, 0.494, 0.45)), title })] }];
    const reach = box(0.647, 0.382, 0.19, 0.309); // the cursor's flight from the card's right to Preview
    const { entries } = frameActions([start(1, view(0.249, 0.274, 1.992))], titled, [act(3, 3.6, reach, 3, "click preview")], "narrow");
    const moved = entries.find((e) => e.held !== undefined).view;
    expect(moved.y).toBeGreaterThanOrEqual(0.34); // wholly below the title's foot
    expect(contains(moved, reach)).toBe(true);
  });

  it("narrow: a dialog opening inside the window keeps its own title whole", () => {
    const title = box(0.263, 0.31, 0.12, 0.03);
    const twoShots = [
      { id: "card", start: 0.6, samples: [sample(0.6, block("card", box(0.209, 0.193, 0.78, 0.26)))] },
      { id: "d", start: 3.7, samples: [sample(3.7, { ...dialog("d", box(0.253, 0.3, 0.494, 0.45)), title })] },
    ];
    const reach = box(0.647, 0.382, 0.19, 0.309);
    const { entries } = frameActions(
      [start(0.6, view(0.165, 0, 1.317), "block: card"), start(3.7, view(0.249, 0.274, 1.992))],
      twoShots,
      [act(3.6, 4.2, reach, 3, "click preview")],
      "narrow",
    );
    expect(entries.find((e) => e.t === 3.7).view.y).toBeGreaterThanOrEqual(0.34);
  });

  it("narrow: a shot whose action crosses its title's band zooms out enough to hold both", () => {
    // quick-start's Preview: the cursor flies from the card's right, through the title's rows.
    const title = box(0.29, 0.394, 0.099, 0.028);
    const d = { ...dialog("d", box(0.271, 0.363, 0.457, 0.361)), title };
    const shots = [{ id: "d", kind: "dialog", name: "Apply", start: 3.7, samples: [sample(3.7, d, null, box(0.3, 0.6, 0.1, 0.05))] }];
    const preview = { start: 4.1, end: 4.6, reach: box(0.647, 0.382, 0.19, 0.309), step: 3, label: "click preview" };
    const [first] = trackFor(shots, "narrow", [preview]);
    const both = 0.837 - 0.29;
    expect(first.view.s).toBeLessThanOrEqual(0.92 / both + 0.001);
    const { entries } = frameActions([first], shots, [preview], "narrow");
    const v = entries[0].view;
    expect(contains(v, preview.reach)).toBe(true);
    expect(v.x).toBeLessThanOrEqual(0.29); // the title starts inside the view
  });

  it("narrow: reads the title where it is when the action happens, not where it ends up", () => {
    const d = dialog("d", box(0.271, 0.363, 0.457, 0.361));
    const early = sample(3.7, { ...d, title: box(0.29, 0.394, 0.099, 0.028) }, null, box(0.3, 0.6, 0.1, 0.05));
    const late = sample(9, { ...d, box: box(0.271, 0.05, 0.457, 0.9), title: box(0.29, 0.08, 0.099, 0.028) }, null, box(0.3, 0.6, 0.1, 0.05));
    const shots = [{ id: "d", kind: "dialog", name: "Apply", start: 3.7, samples: [early, late] }];
    const preview = { start: 4.1, end: 4.6, reach: box(0.647, 0.382, 0.19, 0.309), step: 3, label: "click preview" };
    const [first] = trackFor(shots, "narrow", [preview]);
    expect(first.view.s).toBeLessThanOrEqual(0.92 / (0.837 - 0.29) + 0.001);
  });

  it("wide: a dialog's title is no constraint", () => {
    const title = box(0.263, 0.31, 0.12, 0.03);
    const titled = [{ id: "d", start: 1, samples: [sample(1, { ...dialog("d", box(0.253, 0.3, 0.494, 0.45)), title })] }];
    const reach = box(0.647, 0.382, 0.19, 0.309);
    const { entries } = frameActions([start(1, view(0.249, 0.274, 1.992))], titled, [act(3, 3.6, reach, 3, "click preview")], "wide");
    expect(entries.find((e) => e.held !== undefined).view.y).toBeCloseTo(0.274);
  });

  it("leaves a whole-frame shot alone", () => {
    const { entries } = frameActions([{ t: 1, view: "full", label: "full", shotStart: true }], [{ id: null, start: 1, samples: [] }], [act(6, 6.6, save)]);
    expect(entries).toEqual([{ t: 1, view: "full", label: "full", shotStart: true }]);
  });

  it("keeps a held pan through anti-jitter, however short", () => {
    const { entries } = frameActions([start(1, view(0.226, 0.276, 1.821)), start(6.4, "full", "full")], [...shots, { id: null, start: 6.4, samples: [] }], [act(6, 6.3, save)]);
    const settled = settleViews(entries, 10);
    expect(settled.map((e) => e.t)).toEqual([1, 5.5, 6.4]);
  });

  it("is checked: a reach out of view fails its step, a whole frame never does", () => {
    const entries = [start(1, view(0.226, 0.276, 1.821))];
    expect(checkTrack(entries, [act(6, 6.6, save)])).toEqual([{ step: 4, label: "click save", ok: false }]);
    expect(checkTrack(entries, [act(6, 6.6, box(0.3, 0.4, 0.05, 0.05))])).toEqual([{ step: 4, label: "click save", ok: true }]);
    expect(checkTrack([{ t: 1, view: "full", label: "full", shotStart: true }], [act(6, 6.6, save)])[0].ok).toBe(true);
  });
});

describe("settleViews", () => {
  const v = (t, x, y, s, shotStart = false, label = "l") => ({ t, view: { x, y, s }, label, shotStart });

  it("drops a leading whole frame and a view within 3 % of the last kept", () => {
    const out = settleViews(
      [{ t: 0.6, view: "full", label: "full", shotStart: true }, v(1, 0.2, 0.2, 1.5, true), v(2.5, 0.21, 0.22, 1.5), v(4, 0.2, 0.3, 1.5)],
      10,
    );
    expect(out.map((e) => e.t)).toEqual([1, 4]);
  });

  it("gives a shot's start the view of a pan that follows it within 1.2 s", () => {
    // A dialog easing in is short, then tall: its first view would last a moment before a pan.
    const out = settleViews([v(1.27, 0.226, 0.317, 1.8, true, "dialog: New site"), v(1.87, 0.226, 0.104, 1.8), v(5.3, 0.226, 0.418, 1.8)], 10);
    expect(out).toEqual([v(1.27, 0.226, 0.104, 1.8, true, "dialog: New site"), v(5.3, 0.226, 0.418, 1.8)]);
  });

  it("drops a pan held under 1.2 s, never a shot's start", () => {
    const out = settleViews([v(1, 0.2, 0.1, 1.5, true), v(2, 0.2, 0.3, 1.5), v(2.5, 0.2, 0.5, 1.5), v(4, 0.1, 0.1, 1.2, true)], 10);
    expect(out.map((e) => e.t)).toEqual([1, 2.5, 4]);
  });
});

describe("cameraTracks on a quick-start-shaped clip", () => {
  const card = block("block:quick-start", box(0.209, 0.193, 0.78, 0.26));
  const heights = [0.41, 0.903, 0.449, 0.732, 0.903];
  const samples = [
    ...span(0.6, 3.7, card, box(0.3, 0.25, 0.2, 0.04)).map((s) => ({ ...s, need: box(0.3, 0.25, 0.2, 0.04) })),
    ...heights.flatMap((h, i) => {
      const d = dialog("dialog:1", box(0.253, (1 - h) / 2, 0.494, h));
      const at = box(0.3, (1 - h) / 2 + h - 0.06, 0.1, 0.04);
      return span(3.7 + i * 1.3, 3.7 + (i + 1) * 1.3, d, at).map((s) => ({ ...s, need: at }));
    }),
    ...span(10.2, 13.6, null).map((s) => ({ ...s, browser: true })),
  ];
  const { wide, narrow, shots, checks } = cameraTracks(samples, 13.625, []);

  it("makes three shots and three zoom changes: card, dialog, whole frame", () => {
    expect(shots).toBe(3);
    expect(trackStats(wide).zooms).toBe(3);
    expect(wide[0].label).toBe("block: block:quick-start");
    expect(wide[1].label).toBe("dialog: Dialog dialog:1");
    expect(wide.at(-1)).toMatchObject({ view: "full", label: "browser opened" });
  });

  it("has nothing to check when nothing acted", () => {
    expect(checks).toEqual({ wide: [], narrow: [] });
  });

  it("holds one zoom through the dialog however tall it gets", () => {
    const dialogZooms = new Set(wide.filter((e) => e.view !== "full" && e.t >= 3.7).map((e) => e.view.s));
    expect(dialogZooms.size).toBe(1);
  });

  it("has a narrow track of the same shots, each wide enough for 90 % of its container", () => {
    expect(narrow.at(-1).view).toBe("full");
    expect(trackStats(narrow).zooms).toBe(3);
    expect(narrow[0].view.s).toBeCloseTo(1 / (0.9 * 0.78 + 0.08), 2);
  });
});

describe("trackStats, viewAt and the file", () => {
  const track = [
    { t: 1, view: { x: 0.1, y: 0.1, s: 1.2 }, label: "a" },
    { t: 3, view: { x: 0.2, y: 0.2, s: 1.7 }, label: "b" },
    { t: 5, view: { x: 0.2, y: 0.4, s: 1.7 }, label: "pan" },
    { t: 9, view: "full", label: "browser opened" },
  ];

  it("counts zoom changes and pans", () => {
    expect(trackStats(track)).toEqual({ zooms: 3, pans: 1 });
  });

  it("is the whole frame before the first entry, then the entry in force", () => {
    expect(viewAt(track, 0.5)).toBe("full");
    expect(viewAt(track, 4)).toEqual({ x: 0.2, y: 0.2, s: 1.7 });
  });

  it("writes version 2", () => {
    expect(cameraFile({ clip: "q", duration: 13.6254, frame: { width: 2080, height: 1300 }, wide: track, narrow: [] })).toEqual({
      version: 2, clip: "q", duration: 13.625, frame: { width: 2080, height: 1300 }, wide: track, narrow: [],
    });
  });
});
