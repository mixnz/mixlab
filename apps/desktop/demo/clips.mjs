/**
 * The promotional clips: a scene, then the steps a person would take, filmed by `demo/clip.mjs`.
 *
 * Plain JavaScript because the clip script reads it in node. `setup` runs before filming starts;
 * only `steps` are on film. A step clicks, types, selects, waits for an element to appear
 * (`waitFor`, for what the app decides the timing of) or pauses. Steps aim at `data-demo` hooks the window carries for exactly this, or
 * at option text the fixtures own — never interface copy, never a CSS class (`clips.test.mjs`
 * checks every hook exists in `src/`).
 *
 * A clip may also say what its last frame must show (`endsShowing`, checked when filming stops), a
 * step where the website's camera should look instead of the default (`focus: "full"` or a
 * selector — docs/specs/2026-10-10-demo-clip-camera-design.md), and its own `uiScale`.
 *
 * The design is docs/specs/2026-10-05-demo-clips-design.md.
 */
import { geometryFor } from "./geometry.mjs";

/**
 * Every clip is filmed with the interface this much larger in the same 2080×1300 frame; a clip may
 * set its own `uiScale` (1 films it as the stills are). docs/specs/2026-10-10-demo-clip-camera-design.md.
 */
export const DEFAULT_UI_SCALE = 1.125;

export function geometryOf(clip) {
  return geometryFor(clip.uiScale ?? DEFAULT_UI_SCALE);
}

export const CLIPS = [
  {
    id: "new-site",
    moduleId: "mixengine",
    tabTitle: "MixEngine",
    // The docs and the website's terminal demo both create blog.test; the clip creates the same site.
    fixtures: { sitesWithout: ["blog.test"] },
    setup: async ({ page }) => {
      await page.locator('nav [data-screen="sites"]').click();
    },
    steps: [
      { click: '[data-demo="new-site"]' },
      { select: '[data-demo="site-project"]', option: "blog" },
      { type: '[data-demo="site-domains"]', text: "blog.test" },
      { click: '[data-demo="site-https"]' },
      { click: '[data-demo="site-save"]' },
      { pause: 2500 },
    ],
  },
  {
    id: "quick-start",
    moduleId: "mixengine",
    tabTitle: "MixEngine",
    // A machine with nothing built yet, so the Dashboard offers Quick start; the folder picker
    // answers the project's folder; the site opens by itself once it works (T205, D7), and opening
    // it draws a browser (`demo/browser/overlay.ts`).
    fixtures: { fresh: true, folder: "/Users/ada/Sites/blog", browser: true },
    // The website's camera: card, the Apply dialog, the browser — no more
    // (docs/specs/2026-10-10-demo-clip-camera-shots-design.md).
    maxZoomChanges: 3,
    // The browser window alone, for the website's terminal demo, which ends in the same browser.
    still: { selector: "#__demo-browser", suffix: "browser" },
    // Laravel is the gallery's first blueprint, so the card opens on it: no step picks it.
    steps: [
      { type: '[data-demo="qs-project"]', text: "blog" },
      { click: '[data-demo="qs-folder"]' },
      { click: '[data-demo="qs-create"]' },
      { click: '[data-demo="apply-preview"]' },
      // A signed blueprint's command comes agreed already (T205a): no step ticks it.
      { click: '[data-demo="apply-run"]' },
      { click: '[data-demo="apply-log"]' },
      // Close appears the moment the job is done: wait for it, then a beat to read "Done".
      { waitFor: '[data-demo="apply-close"]' },
      { pause: 800 },
      { click: '[data-demo="apply-close"]' },
      // The site is ready, and so opens by itself, the moment the open button is drawn.
      { waitFor: '[data-demo="open-site"]' },
      { pause: 3000 },
    ],
  },
  {
    id: "pin-runtime",
    moduleId: "mixengine",
    tabTitle: "MixEngine",
    // blog and legacy, PHP 8.4.26 the default and 8.1.34 beside it: the versions the website's
    // terminal shows `php -v` printing (docs/demo/pin-runtime-terminal.md).
    fixtures: { runtimePins: true },
    // The last frame has to show what changed and what did not: both rows, and legacy's pins.
    endsShowing: ['[data-demo-key="blog"]', '[data-demo-key="legacy"]', '[data-demo="project-pin-list"]'],
    setup: async ({ page }) => {
      await page.locator('nav [data-screen="projects"]').click();
    },
    // A hook is on every row alike, so `data-demo-key` — the project's name, the runtime's kind —
    // says which one a step means.
    steps: [
      { click: '[data-demo-key="legacy"] [data-demo="project-menu"]' },
      { click: '[data-demo="project-edit"]' },
      { click: '[data-demo="project-pins"]' },
      { select: '[data-demo-key="php"] [data-demo="project-pin"]', option: "8.1.34" },
      { click: '[data-demo="project-save"]' },
      { click: '[data-demo-key="legacy"] [data-demo="project-details"]' },
      // The open row and its pins: what the website's camera ends on.
      { pause: 2500, focus: '[data-demo-focus="project-open"]' },
    ],
  },
];

export function describeStep(step) {
  if ("click" in step) return `click ${step.click}`;
  if ("type" in step) return `type "${step.text}" into ${step.type}`;
  if ("select" in step) return `select "${step.option}" in ${step.select}`;
  if ("waitFor" in step) return `wait for ${step.waitFor}`;
  if ("pause" in step) return `pause ${step.pause} ms`;
  return JSON.stringify(step);
}
