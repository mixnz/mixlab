import { describe, expect, it } from "vitest";
import appCss from "./App.css?raw";

/**
 * The two roles of text, asserted on the stylesheet itself.
 *
 * There is nothing to render here, and nothing in here is visible in review. Before this token
 * work, `var(--font-mono)` was used in four places without ever being defined: three had a
 * `, monospace` fallback behind them, the fourth did not, so its rule was void and the text fell
 * back to the font inherited from `:root` — which happened to be Fira Code too. It was right by
 * accident, and that accident vanished exactly when `:root` switched to sans. The only symptom was
 * a dialog changing font, so it is asserted here.
 *
 * `?raw` only returns real content because `vite.config.ts` turns on `test.css`: by default Vitest
 * stubs every `.css` as an empty string, even through `?raw`. That is why the first case below
 * exists — a test parsing an empty string is green without reading a single line, and
 * `glass.test.ts` was green exactly like that from the day it was written.
 */

/** Every stylesheet under `src/`. `App.css` is among them and is filtered out where needed. */
const sheets = import.meta.glob("../**/*.css", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

describe("font tokens", () => {
  it("reads the stylesheets, not an empty blob", () => {
    const values = Object.values(sheets);
    expect(values.length).toBeGreaterThan(40);
    expect(values.every((css) => css.length > 0)).toBe(true);
    expect(appCss).toContain(":root");
  });

  it("defines both roles on :root", () => {
    expect(appCss).toMatch(/--font-ui:\s*[^;]+;/);
    expect(appCss).toMatch(/--font-mono:\s*[^;]+;/);
  });

  it("names Geist for both roles", () => {
    expect(appCss).toMatch(/--font-ui:\s*"Geist Variable",/);
    expect(appCss).toMatch(/--font-mono:\s*"Geist Mono Variable",/);
  });

  it("no stylesheet names a font outside the token definitions", () => {
    const offenders = Object.entries(sheets)
      .filter(([path]) => !path.endsWith("/App.css"))
      .filter(([, css]) => /font-family:[^;]*(Geist|Fira Code|system-ui|sans-serif|monospace)/.test(css))
      .map(([path]) => path);
    expect(offenders).toEqual([]);
  });

  it("every var(--font-*) used has a definition", () => {
    const defined = new Set([...appCss.matchAll(/(--font-[\w-]+):/g)].map(([, name]) => name));
    const used = new Set(
      Object.values(sheets).flatMap((css) =>
        [...css.matchAll(/var\((--font-[\w-]+)/g)].map(([, name]) => name),
      ),
    );
    expect([...used].filter((name) => !defined.has(name))).toEqual([]);
  });
});
