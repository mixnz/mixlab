import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { BROWSER_ID } from "./browser/overlay";
import { SITES } from "./fixtures/mixengine";
import { MODULE_IDS } from "./args.mjs";
import { CLIPS, DEFAULT_UI_SCALE, describeStep, geometryOf } from "./clips.mjs";
import { isWholeGeometry } from "./geometry.mjs";

/* Every hook a clip aims at has to exist in the window: a redesign that drops one fails here,
   before anyone films and wonders why a step times out. */
const SRC = fileURLToPath(new URL("../src", import.meta.url));
const SOURCE = readdirSync(SRC, { recursive: true })
  .filter((file) => /\.tsx?$/.test(file))
  .map((file) => readFileSync(join(SRC, file), "utf8"))
  .join("\n");

const selectorOf = (step) => step.click ?? step.type ?? step.select ?? step.waitFor;

describe("clips", () => {
  it("are the three the spec names", () => {
    expect(CLIPS.map((clip) => clip.id)).toEqual(["new-site", "quick-start", "pin-runtime"]);
  });

  it("pin-runtime films the machine the terminal recording was made on, ending on what is pinned", () => {
    const clip = CLIPS.find((c) => c.id === "pin-runtime");
    expect(clip.fixtures).toEqual({ runtimePins: true });
    expect(clip.steps.find((s) => "select" in s).option).toBe("8.1.34");
    expect(clip.steps.at(-2)).toEqual({ click: '[data-demo-key="legacy"] [data-demo="project-details"]' });
    expect(clip.steps.at(-1).pause).toBeGreaterThanOrEqual(2000);
  });

  it("pin-runtime ends showing both rows and legacy's pins", () => {
    expect(CLIPS.find((c) => c.id === "pin-runtime").endsShowing).toEqual([
      '[data-demo-key="blog"]',
      '[data-demo-key="legacy"]',
      '[data-demo="project-pin-list"]',
    ]);
  });

  it("films at a scale that gives whole pixels, by default and per clip", () => {
    expect(isWholeGeometry(DEFAULT_UI_SCALE)).toBe(true);
    for (const clip of CLIPS) {
      if (clip.uiScale !== undefined) expect(isWholeGeometry(clip.uiScale), clip.id).toBe(true);
      expect(geometryOf(clip)).toEqual({ viewport: { width: 1280, height: 800 }, scale: 2.25 });
    }
  });

  it("quick-start allows the website's camera three zoom changes at most", () => {
    expect(CLIPS.find((c) => c.id === "quick-start").maxZoomChanges).toBe(3);
  });

  it("pin-runtime ends framed on the open row and its pins, by a hook", () => {
    expect(CLIPS.find((c) => c.id === "pin-runtime").steps.at(-1)).toMatchObject({
      focus: '[data-demo-focus="project-open"]',
    });
    expect(SOURCE.includes('"project-open"')).toBe(true);
    expect(SOURCE.includes('data-demo-fit="text"')).toBe(true);
  });

  it("the open row is measured by its words, may be framed closer than 2, and the view keeps off the sidebar", () => {
    expect(SOURCE.includes('data-demo-fit={open ? "text" : undefined}')).toBe(true);
    expect(SOURCE.includes('data-demo-zoom-max={open ? "3.3" : undefined}')).toBe(true);
    expect(SOURCE.includes('className="mixengine-screen" data-demo-bounds')).toBe(true);
  });

  it("the camera's focus blocks exist in the window", () => {
    for (const hook of ['data-demo-focus="quick-start"', 'demoFocus="projects"']) {
      expect(SOURCE.includes(hook), `no ${hook} in src/`).toBe(true);
    }
  });

  it("quick-start films a fresh machine, picks a folder, and keeps the browser as a still", () => {
    const clip = CLIPS.find((c) => c.id === "quick-start");
    expect(clip.fixtures).toEqual({ fresh: true, folder: "/Users/ada/Sites/blog", browser: true });
    expect(clip.still).toEqual({ selector: `#${BROWSER_ID}`, suffix: "browser" });
  });

  it("quick-start closes the apply as soon as it is done, not after a guessed pause", () => {
    const steps = CLIPS.find((c) => c.id === "quick-start").steps;
    const close = steps.findIndex((s) => s.click === '[data-demo="apply-close"]');
    expect(steps[close - 2]).toEqual({ waitFor: '[data-demo="apply-close"]' });
    expect(steps[close - 1].pause).toBeLessThanOrEqual(1000);
  });

  for (const clip of CLIPS) {
    it(`${clip.id}: opens a module the registry has`, () => {
      expect(MODULE_IDS).toContain(clip.moduleId);
    });

    it(`${clip.id}: every step is exactly one known action`, () => {
      for (const step of clip.steps) {
        const actions = ["click", "type", "select", "waitFor", "pause"].filter((key) => key in step);
        expect(actions, describeStep(step)).toHaveLength(1);
        if ("type" in step) expect(step.text.length).toBeGreaterThan(0);
        if ("select" in step) expect(step.option.length).toBeGreaterThan(0);
        if ("pause" in step) expect(step.pause).toBeGreaterThan(0);
      }
    });

    it(`${clip.id}: aims only at data-demo hooks the window actually has`, () => {
      for (const step of clip.steps.filter((s) => !("pause" in s))) {
        // An optional scope first: which row or field of several alike, by a name the fixtures own.
        const match = /^(?:\[data-demo-key="([a-z0-9.-]+)"\] )?\[data-demo="([a-z-]+)"\]$/.exec(selectorOf(step));
        expect(match, describeStep(step)).not.toBeNull();
        if (match[1] !== undefined) expect(SOURCE.includes("data-demo-key="), "no data-demo-key in src/").toBe(true);
        const hook = match[2];
        const present = [`data-demo="${hook}"`, `demo="${hook}"`, `demo: "${hook}"`].some((s) => SOURCE.includes(s));
        expect(present, `no data-demo="${hook}" in src/`).toBe(true);
      }
    });

    it(`${clip.id}: a zoom bound, when it has one, is a positive whole number`, () => {
      if (clip.maxZoomChanges !== undefined) {
        expect(Number.isInteger(clip.maxZoomChanges) && clip.maxZoomChanges > 0).toBe(true);
      }
    });

    it(`${clip.id}: a step's focus is "full" or a selector`, () => {
      for (const step of clip.steps.filter((s) => "focus" in s)) {
        expect(typeof step.focus, describeStep(step)).toBe("string");
        expect(step.focus.length).toBeGreaterThan(0);
      }
    });

    it(`${clip.id}: what it ends showing is hooks the window has`, () => {
      for (const selector of clip.endsShowing ?? []) {
        const match = /^\[(data-demo|data-demo-key)="([a-z0-9.-]+)"\]$/.exec(selector);
        expect(match, selector).not.toBeNull();
        const present =
          match[1] === "data-demo-key"
            ? SOURCE.includes("data-demo-key=")
            : [`data-demo="${match[2]}"`, `demo="${match[2]}"`].some((s) => SOURCE.includes(s));
        expect(present, `no ${selector} in src/`).toBe(true);
      }
    });

    it(`${clip.id}: asks the fixtures only what they understand`, () => {
      const known = ["sitesWithout", "fresh", "folder", "browser", "runtimePins"];
      for (const key of Object.keys(clip.fixtures ?? {})) expect(known).toContain(key);
      if (clip.fixtures?.fresh !== undefined) expect(typeof clip.fixtures.fresh).toBe("boolean");
      if (clip.fixtures?.runtimePins !== undefined) expect(typeof clip.fixtures.runtimePins).toBe("boolean");
    });

    it(`${clip.id}: starts without domains the fixtures really have`, () => {
      const domains = SITES.map((site) => site.domain);
      for (const domain of clip.fixtures?.sitesWithout ?? []) expect(domains).toContain(domain);
    });
  }

  it("describes a step by what it does and where", () => {
    expect(describeStep({ click: '[data-demo="new-site"]' })).toBe('click [data-demo="new-site"]');
    expect(describeStep({ type: '[data-demo="site-domains"]', text: "blog.test" })).toBe(
      'type "blog.test" into [data-demo="site-domains"]',
    );
    expect(describeStep({ select: '[data-demo="site-project"]', option: "blog" })).toBe(
      'select "blog" in [data-demo="site-project"]',
    );
    expect(describeStep({ waitFor: '[data-demo="apply-close"]' })).toBe('wait for [data-demo="apply-close"]');
    expect(describeStep({ pause: 2500 })).toBe("pause 2500 ms");
  });
});
