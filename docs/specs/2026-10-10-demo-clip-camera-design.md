---
status: implemented
date: 2026-10-10
---

# Demo clip camera — where to look, and a larger window on film

2026-10-10.

**The case this comes from**: the website's hero plays the clips at most 1040 CSS pixels wide, so
the interface text in them is about 9 px on a desktop and about 3 px on a phone. The website is
getting a "camera" that zooms and pans to the part of the window being used while the clip plays.
It needs to know, over time, *where* that part is — and guessing it by hand for every clip, every
redesign, is the same trap the clips themselves were built to avoid. So the pipeline that films a
clip also writes down where to look. And every clip is filmed with the window's interface larger
to begin with, so there is less to zoom.

The marketing screenshots (`npm run screenshots`) do not change.

The website's camera is not part of this repository and not part of this design.

## What is already true

- `npm run clips` films each clip from fixtures and encodes it — the design is
  [the clips spec](2026-10-05-demo-clips-design.md). Steps aim at `data-demo` hooks; a hook on
  every row alike is narrowed by the row's `data-demo-key`.
- **The video's clock is the screencast's.** `recorder.mjs` keeps each CDP screencast frame with
  `metadata.timestamp` (seconds since the epoch); `resample()` makes tick 0 of the mp4 the first
  frame, and the mp4 runs to when recording stopped. `LEAD_IN_MS` is on film.
- **Geometry.** The page is 1440×900 CSS pixels at device scale 2; the screencast is 2880×1800;
  the video and posters are scaled to 2080×1300. All three are 16:10 and the scale is uniform —
  nothing is cropped or letterboxed — so a fraction of the viewport is the same fraction of the
  video.
- Dialogs are `Modal`'s `role="dialog"` (and `Popover`'s). `Select`'s listbox and `ContextMenu`'s
  menu are portals: outside the dialog they belong to.
- Quick start's `AfterApply`, where the clip ends before the browser opens, is drawn inside the
  Quick start card, not in a dialog.
- The browser overlay (`#__demo-browser`) is positioned in percentages of the viewport.

## What changes

### 1. A camera file for every clip

Each encoded clip also writes `<clip>-<theme>.camera.json` beside its mp4:

```json
{
  "version": 1,
  "clip": "quick-start",
  "duration": 13.71,
  "frame": { "width": 2080, "height": 1300 },
  "keys": [
    { "t": 0.6, "focus": { "x": 0.2, "y": 0.2, "w": 0.78, "h": 0.17 }, "step": 0, "label": "type \"blog\" into [data-demo=\"qs-project\"]" },
    { "t": 11.2, "focus": "full", "step": 9, "label": "browser opened" }
  ]
}
```

- `duration` is the mp4's: its tick count over 24 fps.
- `frame` is the encoded video's size.
- A key says "from `t` on, look at `focus`" and holds until the next key. Before the first key —
  the lead-in — the whole frame is shown. `focus` is either `"full"` or a rectangle as fractions of
  the frame, clamped to 0–1. Its aspect ratio is whatever the region's is; fitting it to the
  player is the website's choice.
- `step` is the index in the clip's `steps`, and `label` is `describeStep(step)` — or
  `"browser opened"`.
- Times and fractions are rounded to three decimals.

#### Where to look during a step

The step's *target* is the element it clicks, types into, selects in or waits for. A `pause` keeps
the target of the step before it, so a region that grows during a pause is still followed. The
region is decided in the page, in this order:

1. **The browser overlay.** While `#__demo-browser` is in the page, the focus is `"full"`.
2. **A clip's override.** A step may carry `focus: "full"` or `focus: "<css selector>"`; the
   selector's element is the region.
3. **The dialog** — the target's nearest `[role="dialog"]` ancestor.
4. **A marked block** — the target's nearest ancestor with `data-demo-focus`.
5. **The target itself**, widened by a margin.

Then, **an open listbox or menu is added**: while a `[role="listbox"]` or `[role="menu"]` is in the
page, the region is the union of the one above and it — a dropdown or a context menu is a portal,
so it is never inside the dialog that opened it, and a camera on the dialog alone would cut it off.

**A target that is gone has done its work.** Once a step's target has left the page — a menu item
that opened a dialog, a **Save** that closed one — or a modal dialog it is not in has covered it —
Quick start's **Create it**, behind the dialog it opened — the region is decided from the next
step's target instead, by the same rules. Without this the camera would hold its last region until the
next step began, half a second or more after the dialog had opened or closed on film.

**A target inside a listbox or menu starts no region of its own.** Its region is the previous
step's, plus the menu. Without this, `pin-runtime`'s click on **Edit** — an item of the row's
context menu, a portal outside the Projects card — would fall through to a bare menu item, and the
camera would dive onto one word and back out to the card a second later.

Margins, in CSS pixels: 16 around a dialog, a marked block or an override, 48 around a bare target.

`data-demo-focus` is a new inert attribute, like `data-demo`. It goes on:

- the **Quick start card** (`QuickStart.tsx`) — the stack, project, folder and create steps, and
  `AfterApply` once the apply is closed;
- the **Projects table card** (`Projects.tsx`) — `pin-runtime`'s row menu and details.

`new-site`'s first click is the Sites screen's primary button, alone in the page header: it falls
through to the bare target with its 48-pixel margin. That region is small, so how far to zoom into
it is the website's call — a camera ought to cap its zoom anyway. Every later step is in the dialog.

#### Sampling

While filming, from the first step on — the lead-in is not sampled — the rig measures the region:

- just before a step runs, and just after it (after `SETTLE_MS`);
- every 80 ms until recording stops, whatever is running — a dialog grows when its plan or log
  appears, and a step that waits for a job lasts seconds. A tick that finds the last measurement
  still running skips.

The interval is how late a key can be: a key carries the time of the sample that first saw the new
region, so a change that lands just after a sample is keyed one interval later. 250 ms was the
first figure, and it cannot meet the 0.1 s bound below; 80 ms, about two frames of the clip, can.

Each sample is `{ at, step, label, focus }`: the step running (or the last one run), and the region
as a rectangle in CSS pixels or `"full"`. The measuring is one `page.evaluate` running the rules
above; `at` is the wall clock at the midpoint of that call. Measuring changes nothing in the page,
so it cannot disturb a step it runs beside.

#### Times on the video's clock

A screencast frame's `metadata.timestamp` is when Chromium swapped that frame, in seconds since the
epoch — the machine's wall clock, the one Node reads too: both processes run on the same machine.
So a sample's time on the video is simply `at − frames[0].t`, with no conversion between clocks.
Measured on this machine before writing this (Windows 11, Playwright's Chromium, 1440×900 @2): a
frame reached Node 6–28 ms after its `timestamp`, and a style change made from Node was on the next
swapped frame 15–24 ms later — one frame, on the same clock.

Nothing about this is estimated from delivery: a frame reaches Node only after Chromium has encoded
a PNG, so arrival times carry the encoding, and a clock fitted to them would carry it too. Arrivals
are used for one thing — **checking the assumption**. The recorder keeps each frame's arrival on
the wall clock, and `arrival − metadata.timestamp` must be positive and under one second for every
frame. A clock that disagrees with that is not the same clock, and the clip fails rather than
writing a camera that is wrong by an unknown amount. The median of that delay is printed as a note,
so a slow machine is visible.

What is left is the time a layout change takes to reach the screen — a frame, about 17 ms — and
half an `evaluate`: well inside 0.1 s.

#### Fractions of the frame

A rectangle in CSS pixels becomes fractions by dividing by the viewport's CSS size. The screencast
is the viewport at the device scale, and the video is that scaled uniformly, so no further
correction exists to apply. This holds for `uiScale` too (below): the viewport is smaller and the
device scale larger, and a fraction of it is still a fraction of the video.

#### From samples to keys — a pure function

`demo/camera.mjs` holds the arithmetic, tested in `demo/camera.test.mjs` without a browser:

- `toVideoTime(at, start, duration)` — a wall-clock time onto the video, clamped to 0…`duration`
  (a sample taken in the instant recording stopped is not past the end).
- `clockCheck(frames)` — the assumption above: every delay positive and under a second; and the
  median.
- `normalize(rect, viewport)` — CSS pixels to fractions, clamped.
- `keysFrom(samples)` — samples in time order become keys. A sample opens a key when its focus
  differs from the last key's: one is `"full"` and the other is not, or any of `x`, `y`, `w`, `h`
  differs by 0.02 or more. Otherwise it is dropped as the same region. A key takes its sample's
  `step` and `label`. **A region still settling is folded**: one that differs within 0.4 s
  (`SETTLING`) of the last key's start replaces that key's region and keeps its time, so a dialog
  easing in and then growing as its content arrives is one move — made when the change begins, to
  where it ends — rather than three keys 80 ms apart. A switch to or from `"full"` is never folded.
- `compareKeys(a, b)` — how far two clips' keys are apart: whether the counts and steps agree, the
  largest time difference, and the largest edge difference.

#### Light and dark agree

The two themes film the same flow and should give nearly the same keys. After both themes of a
clip are encoded in one run — a run given `--theme` has only one, and compares nothing — the run
compares them with `compareKeys` and prints a note when the counts or steps differ, a time differs
by more than 0.25 s, or an edge by more than 0.02. A note, not a failure: timing jitters between
two runs, and the person looking decides.

#### The contact sheet

`<clip>-<theme>.camera.png` is for a person checking the camera by eye, not for the website. One
frame per second of the **encoded mp4** — decoded by ffmpeg, so it shows what the website plays —
each with the focus that applies at that second drawn over it and its time written under it,
four to a row. The sheet is composed as HTML in a page of the browser the rig already has and
photographed, so there is no image library to add. Because the frames come from the mp4 and the
rectangles from the keys, a rectangle drawn beside the dialog rather than on it is a timing error
anyone can see.

### 2. `uiScale` — a larger window on film

Every clip is filmed at `uiScale` 1.125 — `DEFAULT_UI_SCALE` in `demo/clips.mjs` — and a clip may
set its own `uiScale` instead (1 films it as before). The interface is then that much larger in the
frame, the video still 2080×1300.

**How**: the viewport shrinks by `uiScale` and the device scale grows by it. At 1.125 that is
1280×800 CSS pixels at device scale 2.25 — still a 2880×1800 screencast, still a 2080×1300 video.

**Why 1.125 and not 1.25**, the first figure: at 1.25 the window is 1152 pixels wide, and MixLab
reflows there — the Dashboard's Reload / Stop all / New service drop under the title, Quick start's
**Create it** falls to a line of its own, the Services table loses its last column, and the browser
overlay's page goes under Laravel's desktop breakpoint. Seen on the first render, at the
reviewer's word that it was zoomed too far. At 1280×800 — a common laptop window, and the largest
whole-pixel scale below 1.25 checked — every screen the clips film keeps the layout it has at
1440×900, larger.

- **Sharp, not enlarged.** Chromium draws text and borders at the device scale, so every glyph is
  rendered at 2.25× rather than drawn at 2× and scaled up.
- **Nothing measured or clicked is transformed.** Playwright's clicks and every
  `getBoundingClientRect` are in CSS pixels of an ordinary page. The alternatives were weighed and
  set aside: CSS `zoom` on the app has a history of disagreeing with `getBoundingClientRect` and
  pointer coordinates inside portals — exactly what the camera measures — and Chromium's page
  scale is a magnifier that crops the edges rather than a larger interface.
- **What it costs**: MixLab is laid out for a 1280-pixel-wide window. Anything that reflows at
  that width reflows on film — the frame checks below are what catch it. The rig's cursor, drawn
  in CSS pixels, is 1.125× larger too.
- **The browser still.** The overlay is placed in percentages, so `quick-start-<theme>-browser.png`
  — the overlay photographed at the device scale — has the same pixel size as before and still
  fits the frame. Its own fixed-size chrome (the 13 px address bar) is 1.125× larger, like the
  rest. **The page inside it is not**: it is laid out at the width it has in a 1440-pixel window
  and shrunk to fit (`demo/browser/overlay.ts`), so Laravel's welcome page keeps its desktop layout
  at any `uiScale`.
- The rig's viewport and scale stop being constants that `recorder.mjs` imports; `openScene`
  takes them, and the screencast's size, the cursor's starting point and the camera's fractions
  come from the clip's own geometry. A screenshot scene keeps 1440×900 at 2.

All three clips — `new-site`, `quick-start`, `pin-runtime` — take the default. One size for all of
them rather than only the hero's: the three play on one website, where one window drawn at two sizes
would look like two products, and text too small to read is every clip's problem, not one's.
`clips.test.mjs`
checks that the default and any clip's own `uiScale` are numbers from 1 to 2 whose viewport is whole
pixels (1440 and 900 divided by it).

The budget is unchanged. If a clip goes over 1 MB, its pauses shorten first, as before.

#### Everything a step uses stays in the frame

A smaller viewport shows less of a screen, so what fitted at 1440×900 may not at 1280×800. Two
checks keep that from reaching film unnoticed:

- **Every step's target is wholly in the frame when it acts.** Before a click, a type, a select's
  option or a `waitFor` returns, the rig reads the element's box; one not wholly inside the
  viewport fails the clip, naming the step. Playwright scrolls a target into view on its own; a
  step whose target moved while being reached is printed as a note (`step 3 scrolled into view`),
  since a scroll is on film and somebody should know it is there.
- **A clip can say what its last frame must show.** `endsShowing` lists selectors that must be
  wholly in the viewport when filming stops; one that is not fails the clip. `pin-runtime` ends on
  the `blog` row, the `legacy` row and its **Effective runtime pins** section —
  `[data-demo-key="blog"]`, `[data-demo-key="legacy"]` and a new hook, `data-demo="project-pin-list"`,
  on that section in `ProjectDetailPanel.tsx`.

### For this change only: images to approve the scale

Before the new clips are published, each clip and theme gets one image made by hand
(`<clip>-<theme>-scale.png`): the **first and last frames** at `uiScale` 1 on the left and at 1.125 on
the right, one row each — the four posters, stacked by ffmpeg, the uiScale-1 ones from a render made
before this change. The last frame is where `pin-runtime`'s rows have to fit; the first is where the
scale shows, since `quick-start` ends on the browser overlay, which is placed in percentages and
looks the same at both. They are a review aid for this change, not a pipeline output.

## Output

`apps/desktop/screenshots/out/clips/`, beside what is there today, for every clip and theme:

- `<clip>-<theme>.camera.json`
- `<clip>-<theme>.camera.png`

`--check` films and samples but encodes nothing, so it writes neither: the times are only final
against an encoded mp4.

## Failures

- A sample for which neither the step's target nor the next step's is in the page is skipped, not
  an error.
- A screencast clock that fails `clockCheck` fails the clip.
- A clip whose camera has no key fails: the measuring found nothing on any step.
- A step's `focus` selector that matches nothing fails the clip, naming the step.

## MixLab

This is tooling for MixLab's promotional material. The window gains one inert attribute,
`data-demo-focus`, on the Quick start card and on the Projects table card, and one more `data-demo`
hook, `project-pin-list`, on a project's pins section. No screen changes.

## Testing

- `demo/camera.test.mjs`: the clock conversion and its clamping; `clockCheck` passing ordinary
  delays and failing a negative one and one over a second; fractions from CSS pixels at both
  geometries (1440×900 @2 and 1152×720 @2.5 give the same fractions for the same share of the
  window);
  clamping; `keysFrom` keeping the first sample, dropping a region that moved less than 0.02,
  keeping one that moved 0.02, switching to and from `"full"`; `compareKeys` on equal keys, on a
  missing key and on a shifted one.
- `demo/clips.test.mjs`: every `data-demo-focus` the design names exists in `src/`; a step's
  `focus` is `"full"` or a non-empty string; the default `uiScale` and any clip's own are numbers
  from 1 to 2 giving whole pixels; every `endsShowing` selector is a `data-demo` or
  `data-demo-key` hook that exists in `src/`, and `pin-runtime`'s names the two rows and the pins
  section.
- `npm run clips -- --check` and `npm run screenshots -- --check` both pass, and the screenshots
  are byte-for-byte what they were before this change (`npm run screenshots` into a scratch
  directory, compared).
- **Once, by hand, for this change**: for each clip, the keys where a dialog opens and where the
  browser opens are checked against frames decoded from the mp4 around those times; the region
  must appear within 0.1 s of its key. The contact sheets are looked at for all three clips, both
  themes.
