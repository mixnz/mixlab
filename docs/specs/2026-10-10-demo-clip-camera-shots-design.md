---
status: implemented
date: 2026-10-10
---

# Demo clip camera, version 2 — shots that hold a zoom and pan, with every action in view

2026-10-10.

**The case this comes from**: the first camera file
([the camera spec](2026-10-10-demo-clip-camera-design.md)) made the website's camera zoom in and
out all the time. `quick-start` light went 1.16 → 1.74 → 1.02 → 1.74 → 1.23 → 1.02 → 1 in eleven
seconds, because one dialog — Apply "Laravel", on screen from about 3.7 s to 10.2 s — changes height
with every state, and every key carried that dialog's box at that moment. A camera should zoom
rarely and mostly pan. This replaces the camera file's contents and how they are computed; filming,
`uiScale`, the frame checks and the contact sheet's place in the run stay as that spec has them.

The website then played the first version-2 render and found four faults, which this spec also
answers:

1. **An action out of view.** In `pin-runtime` the click on **Save** was not seen: `wide` held
   `x .226 y .276 s 1.821` from 2.28 s to 6.46 s, about 27–82 % of the height, with Save at the
   dialog's foot, and the cursor flew outside the view at times. The anti-jitter rules had dropped
   the pans that would have shown it.
2. **`pin-runtime`'s last frame showed too much**: a strip of the sidebar on the left, the
   MIXENGINE.TOML column cut on the right, empty space below.
3. **`narrow` zoomed too far.** At 2.4× its view was 42 % of the frame wide where a dialog is 49 %,
   so both sides were lost — "Done" read "one".
4. **Nothing checked it.** No part of the run said whether a step's target was in view.

## What is already true

- The rig samples, every 80 ms and around every step, the region a step works in, and writes
  `<clip>-<theme>.camera.json` and a contact sheet. A target gone, or covered by a modal it is not
  in, looks at the next step's.
- The rig's cursor glides to a target over `GLIDE_MS` (450 ms), then presses; a `select` glides
  twice, to the trigger and to the option.
- Dialogs are `Modal`'s `role="dialog"` with `aria-modal` and an `aria-label` (its title). Blocks
  are `data-demo-focus`: the Quick start card, the Projects card.

## What changes

### What the rig measures

Each **sample**, measured in the page, carries:

- **`container`** — the element framing the step (a dialog, else the nearest `data-demo-focus`
  block), as `{ id, kind, name, box }`, plus for a dialog its **`title`** box (its first heading), for
  a block its **`zoomMax`** (below), and for either its **`bounds`** and **`columns`** (below). `id`
  identifies the *node*: the page tags each dialog it meets with a number the first time, so one
  dialog that grows is one id and a second dialog is another. A block's `id` and `name` are its
  `data-demo-focus` value. No container — the browser overlay is up, a bare target, or nothing to
  measure — is `container: null`.
- **`point`** — where to look inside it: the box of **what appeared** in the container within the
  last 600 ms (watched by a `MutationObserver`: the log as its lines arrive, the plan when Preview
  answers, "Done" when the job ends), else the step's target (or the next step's).
- **`need`** — what has to be legible on a phone: the step's target grown to its row (the nearest
  `label`, `tr`, `li` or `[role="option"]`), or nothing.
- **`browser`** — true while `#__demo-browser` is in the page.

A block may be **several elements sharing one value**; its box is their union. An element marked
`data-demo-fit="text"` counts by the extent of its words, not its box.

Every `click`, `type` and `select` — each glide of it, the option included — also records an
**action**: `{ start, end, from, to, target, step, label }`, `start` when the glide begins, `end`
when the press has landed (for `type`, when the last character is in), `from` and `to` the cursor's
positions, `target` the element's box. An action's **reach** is the box holding its target and the
cursor's whole path; its **window** runs from `start` to `end`.

Samples and actions are written raw beside the camera, as `<clip>-<theme>.samples.json`, so a
later change to the rules below recomputes a camera without filming again
(`npm run clips -- --camera-only`, which rewrites the camera file and the contact sheet).

### From samples to a camera — pure, in `shots.mjs`

Fractions of the frame throughout; times on the mp4's clock.

**1. Shots.** Consecutive samples with the same `container.id` form a shot; samples with
`container: null` form a `full` shot. A shot starts at its first sample's time.

**2. Anti-jitter, on shots**, until nothing changes:
- a shot shorter than **1.2 s** is folded into the shot before it (its samples are dropped from that
  shot's measurements); a first shot shorter than 1.2 s is dropped into the `full` lead-in;
- a `full` shot shorter than **1.5 s** between two zoomed shots is folded into the one before;
- two neighbouring shots now on the same container merge.

**3. Zoom, fixed per shot.**
- *wide*: `s = clamp(1 / (wMax + 2·0.04), 1, cap)`, `wMax` the container's widest box over the shot;
  height does not count. `cap` is 2, or the block's `zoomMax` (below).
- *narrow*: `s = clamp(min(1 / (hNeed + 2·0.04), 1 / (0.9·wMax + 2·0.04)), 1, 2.0)`, `hNeed` the
  tallest need over the shot (the container's height when there is none): legible, and **at least
  90 % of the container's width** in view. What appeared steers the pan but never the zoom — a plan
  0.7 of the frame tall would otherwise hold a phone's whole dialog shot near 1×.
- Either track's zoom is then lowered, if it must be, until **every reach in the shot fits** the
  view less its 4 % inset: `s ≤ (1 − 2·0.04) / max(reach.w, reach.h)`.
- A `full` shot is `s = 1`, view `"full"`.

**4. The view** is `w = h = 1/s` of the frame (the frame's own aspect), its top-left `x, y`.
- *wide, horizontally*: centred on the container.
- *narrow, horizontally*: where the view is narrower than the container, anchored to the
  container's **left edge less 2 %** — a dialog's title, labels and the start of its lines are on
  the left; otherwise centred on it.
- *both*: kept inside the container's `bounds` where it fits (below), then inside the frame; then
  its left and right edges are slid, without changing the zoom, off any of the container's
  `columns` (below).
- *both, vertically*: a container no taller than the view is centred. A taller one is **panned**:
  the view stays while the point is at least **8 %** of the view's height from its top and bottom;
  when not, the view moves so the point's centre sits at **40 %** of its height. It is then kept
  inside the container and the frame.
- *narrow*: a dialog's **title is never cut** — when any of it is in a view, all of it is.

**5. Anti-jitter, on views.** A pan that would be held less than **1.2 s** is dropped, the view
before it held on. A shot's first view held less than 1.2 s before a pan takes that pan's view and
the pan is dropped: a dialog easing in is short for a moment, then tall. A view within **3 %** of
the one before is dropped.

**6. Every action in view — above every rule in 2 and 5.** For every action, in both tracks, every
view in force during its window contains its reach with an inset of **4 % of the view's size**:
- where the view in force when the window opens does not, a pan is placed **0.5 s before the window
  opens** — the website's pan takes 0.5 s, so it has arrived when the cursor starts — no earlier
  than the shot's start (then the shot's first view is moved instead) nor than the end of the
  previous action's window; when that leaves less than 0.5 s the pan is still made, and noted. It
  moves the view the least that brings the reach in;
- such a pan, and a view that already contains an action's reach, is **held** through that window:
  no anti-jitter rule drops it, and a later view that would lose the reach inside the window is
  dropped instead. Anti-jitter drops a pan only where every reach stays in view.

**7. The check.** Every action is checked against both tracks at every view in force inside its
window. A reach out of its inset view fails the clip, naming the step and the track, and **no
camera file is written**.

**8. Labels.** A shot's first view is labelled by its container — `dialog: <aria-label>`,
`block: <data-demo-focus value>`, or `full` / `browser opened` — and a pan `pan: <step>`.

### Bounds, columns and a block's own cap

- The MixEngine screen's content pane (`MixEngineTab.tsx`'s `.mixengine-screen`) carries
  `data-demo-bounds`; a sample records the nearest such pane's box as the container's **`bounds`**,
  so a view stays off the sidebar where it fits.
- For a container in a table, the sample records the horizontal extent of every text run in that
  table's rows as **`columns`**, and a view's left and right edges are slid off them: a view cuts
  between columns, never through one. Where no such place keeps every reach in view, the reach wins.
- A `data-demo-focus` element may carry **`data-demo-zoom-max`**; a block's `wide` cap is the largest
  any of its elements sets, in place of 2. `narrow`'s cap stays 2.0.

### The file, version 2

```json
{
  "version": 2,
  "clip": "quick-start",
  "duration": 13.625,
  "frame": { "width": 2080, "height": 1300 },
  "wide": [
    { "t": 0.6, "view": { "x": 0.17, "y": 0.1, "s": 1.16 }, "label": "block: quick-start" },
    { "t": 3.7, "view": { "x": 0.21, "y": 0.25, "s": 1.74 }, "label": "dialog: Apply “Laravel”" },
    { "t": 5.1, "view": { "x": 0.21, "y": 0.45, "s": 1.74 }, "label": "pan: click [data-demo=\"apply-run\"]" },
    { "t": 10.3, "view": "full", "label": "browser opened" }
  ],
  "narrow": [ ]
}
```

`x`, `y` are the view's top-left as fractions of the frame and `s` its zoom; the website applies
`transform: scale(s) translate(-x·100%, -y·100%)`. Before the first entry the frame is whole. Times
and fractions are rounded to three decimals.

### The clips

- **`pin-runtime`** ends on the `legacy` row and its **Effective runtime pins**: the open row's name
  and root cells (`Projects.tsx`, only while that row is open) and the pins section
  (`ProjectDetailPanel.tsx`) carry `data-demo-focus="project-open"`, all measured by their words —
  the folder icon, `legacy`, its path and the pins line, about 0.28 of the frame wide, not the root
  cell's empty right half — and the block sets `data-demo-zoom-max="3.3"`, so its shot comes to
  about 2.8× (the website's own hand-made region, `.23 .33 .30 .17`, was about 3.3×). The view stays
  off the sidebar and cuts no column; vertically, at that zoom, the foot of the `blog` row above
  is unavoidable. Those cells are not ancestors of the **Details** button, so the clip's last step,
  the pause on the result, says `focus: '[data-demo-focus="project-open"]'`. The website's override
  for this clip can go. Before **Save** is clicked, both tracks pan to it, by rule 6.
- **`new-site`**: once **Save** closes the dialog, the shot is `full` and the new `blog.test` row is
  in view. The **New site** button alone has no container, so it is part of the opening `full` shot.
- **`quick-start`**: the Quick start card, then the Apply dialog (one shot however its height
  changes, panning to the plan, the log and "Done"), then `full` when the browser opens — **three
  zoom changes at most**, which the run checks.

### Checks and report

- The contact sheet draws each second's *wide* view as a solid box and *narrow* as a dashed one, and
  marks the **cursor** with a dot where it is at that second, so a cursor outside the boxes is seen.
- The run prints, per clip, theme and track: the zoom changes and pans, and **every step with
  "target in view: yes/no"** — all must be yes for the camera to be written — and notes when light
  and dark differ in their number of shots. `quick-start` over its three-zoom bound fails the clip.
- `--check` films and computes the camera (so the bounds and rule 7 are checked) but writes nothing.

### Settled while building

- **An action ends as its press lands**, not when the click returns: by then what the click opened
  — a dialog, a taller dialog — is on screen and the camera has rightly moved to it, and counting
  that as the action's window failed every click that opens something.
- **A shot that starts inside an action's window holds its reach too.** The cursor can set off for
  a button in a dialog a moment before the dialog's shot is seen.
- **A pan made at the previous window's end is rounded up**, to the next millisecond: rounded down
  it landed back inside that window, and failed it by chance.
- **While nothing is there to act on** — a job running before its Close appears — the last
  container is kept if it is still on screen, rather than cutting to the whole frame and back.
  Such samples have no need, so they do not set `narrow`'s zoom; the container's height stands in
  only when no sample of the shot has one.
- **A narrow action crossing a dialog title's rows** cannot leave the title wholly out, so that
  shot's narrow zoom also fits title and reach together; the title is read where it is at the
  action's time, since it moves as the dialog grows.
- **The clock.** Samples and frame arrivals are timed with `Date.now`, the system clock Chromium's
  timestamps use; `performance.timeOrigin + now()` drifted from it by several milliseconds while the
  system clock was being adjusted. A frame may seem up to 5 ms early before the clocks are judged
  different.

## MixLab

Tooling for MixLab's promotional material. The window gains inert attributes only:
`data-demo-focus="project-open"` with `data-demo-fit="text"` and `data-demo-zoom-max` on a project's
open row and its pins, and `data-demo-bounds` on the MixEngine screen's content pane. No screen
changes.

## Testing

`demo/shots.test.mjs`, as pure functions over hand-written samples and actions:

- shots split on container id, `null` as `full`; a dialog that grows is one shot;
- a 0.7 s opening shot is dropped; a 1 s shot mid-clip folds into the one before; a 1.4 s `full`
  between two zoomed shots folds; a 1.6 s one stays; neighbours on one container merge after a fold;
- wide zoom from the widest box only, clamped to 1 and the cap, a block's `zoomMax` raising it;
- narrow zoom from the tallest need and 90 % of the width, clamped to 1 and 2.0; anchored at the
  container's left less 2 %; a dialog title never cut;
- a short container centred; a tall one panned only when the point is within 8 % of an edge, and
  then to 40 %; bounds kept; edges slid off columns;
- a pan held under 1.2 s dropped; a view within 3 % dropped; a shot's first view followed within
  1.2 s by a pan opens on the pan's view;
- an action's reach spans its target and its cursor's path; a shot's zoom falls to fit its widest
  reach; a reach out of view gets a pan 0.5 s before its window, held through it, never before the
  shot's start nor the previous window's end; a held pan survives anti-jitter;
- the check refuses a camera with a reach out of view, naming step and track;
- the `quick-start`-shaped sequence gives three zoom changes.

`clips.test.mjs` checks the hooks exist. `npm run clips` renders all three clips, filmed again for
the actions and the hooks; every contact sheet is looked at, and `pin-runtime`'s last frame against
what the website asked.
