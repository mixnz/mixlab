import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { BROWSER_ID } from "./browser/overlay";
import { SITES } from "./fixtures/mixengine";
import { MODULE_IDS } from "./args.mjs";
import { CLIPS, describeStep } from "./clips.mjs";

/* Every hook a clip aims at has to exist in the window: a redesign that drops one fails here,
   before anyone films and wonders why a step times out. */
const SRC = fileURLToPath(new URL("../src", import.meta.url));
const SOURCE = readdirSync(SRC, { recursive: true })
  .filter((file) => /\.tsx?$/.test(file))
  .map((file) => readFileSync(join(SRC, file), "utf8"))
  .join("\n");

const selectorOf = (step) => step.click ?? step.type ?? step.select ?? step.waitFor;

describe("clips", () => {
  it("are the two the spec names", () => {
    expect(CLIPS.map((clip) => clip.id)).toEqual(["new-site", "quick-start"]);
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
        const match = /^\[data-demo="([a-z-]+)"\]$/.exec(selectorOf(step));
        expect(match, describeStep(step)).not.toBeNull();
        const hook = match[1];
        const present = [`data-demo="${hook}"`, `demo="${hook}"`, `demo: "${hook}"`].some((s) => SOURCE.includes(s));
        expect(present, `no data-demo="${hook}" in src/`).toBe(true);
      }
    });

    it(`${clip.id}: asks the fixtures only what they understand`, () => {
      const known = ["sitesWithout", "fresh", "folder", "browser"];
      for (const key of Object.keys(clip.fixtures ?? {})) expect(known).toContain(key);
      if (clip.fixtures?.fresh !== undefined) expect(typeof clip.fixtures.fresh).toBe("boolean");
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
