import { describe, expect, it } from "vitest";

/**
 * Invariants over every stylesheet and every component source under `src/`.
 *
 * `?raw` returns real contents only because `vite.config.ts` turns on `test.css`; the first case
 * guards that, so an empty read cannot pass the rest.
 */
const sheets = import.meta.glob("../**/*.css", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

const sources = import.meta.glob(["../**/*.{ts,tsx}", "!../**/*.test.{ts,tsx}"], {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

function offenders(files: Record<string, string>, test: (text: string) => boolean): string[] {
  return Object.entries(files)
    .filter(([, text]) => test(text))
    .map(([path]) => path);
}

describe("stylesheets", () => {
  it("reads real stylesheets and sources", () => {
    expect(Object.keys(sheets).length).toBeGreaterThan(40);
    expect(Object.values(sheets).every((css) => css.length > 0)).toBe(true);
    expect(Object.keys(sources).length).toBeGreaterThan(100);
  });

  it("carries no Liquid glass", () => {
    expect(
      offenders(sheets, (css) => /data-glass|glass-(scrim|sheet|pill)|--glass-|--rail-filter/.test(css)),
    ).toEqual([]);
    expect(
      // A class name "glass" / "glass-pill" follows a space or a quote; the retired glass key
      // that themeModel.ts still names follows a hyphen and is allowed.
      offenders(sources, (src) => /GlassFilter|glass\.css|useGlass|[\s`"']glass(-pill)?[`"']/.test(src)),
    ).toEqual([]);
  });

  it("never waits for data-theme to be absent", () => {
    expect(offenders(sheets, (css) => /prefers-color-scheme|:not\(\[data-theme\]\)/.test(css))).toEqual([]);
  });
});
