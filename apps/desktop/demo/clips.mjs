/**
 * The promotional clips: a scene, then the steps a person would take, filmed by `demo/clip.mjs`.
 *
 * Plain JavaScript because the clip script reads it in node. `setup` runs before filming starts;
 * only `steps` are on film. A step clicks, types, selects, waits for an element to appear
 * (`waitFor`, for what the app decides the timing of) or pauses. Steps aim at `data-demo` hooks the window carries for exactly this, or
 * at option text the fixtures own — never interface copy, never a CSS class (`clips.test.mjs`
 * checks every hook exists in `src/`).
 *
 * The design is docs/specs/2026-10-05-demo-clips-design.md.
 */
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
    // answers the project's folder; opening the site draws a browser (`demo/browser/overlay.ts`).
    fixtures: { fresh: true, folder: "/Users/ada/Sites/blog", browser: true },
    // The browser window alone, for the website's terminal demo, which ends in the same browser.
    still: { selector: "#__demo-browser", suffix: "browser" },
    // Laravel is the gallery's first blueprint, so the card opens on it: no step picks it.
    steps: [
      { type: '[data-demo="qs-project"]', text: "blog" },
      { click: '[data-demo="qs-folder"]' },
      { click: '[data-demo="qs-create"]' },
      { click: '[data-demo="apply-preview"]' },
      { click: '[data-demo="apply-scaffold"]' },
      { click: '[data-demo="apply-run"]' },
      { click: '[data-demo="apply-log"]' },
      // Close appears the moment the job is done: wait for it, then a beat to read "Done".
      { waitFor: '[data-demo="apply-close"]' },
      { pause: 800 },
      { click: '[data-demo="apply-close"]' },
      { click: '[data-demo="open-site"]' },
      { pause: 3000 },
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
