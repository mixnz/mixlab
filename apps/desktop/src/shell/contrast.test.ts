import { describe, expect, it } from "vitest";
import appCss from "./App.css?raw";
import { COLOR_THEMES } from "./themeModel";

/**
 * WCAG contrast of the pairs the design promises, read off `App.css` itself.
 *
 * A theme is the `:root` block with the `:root[data-theme="dark"]` block laid over it, and a colour
 * theme is that with its `[data-palette]` block laid over in turn. Values are
 * resolved through `var()` inside that map, colours are `#rrggbb`, and a wash is a `-rgb` token at
 * an alpha composited over the surface it sits on — which is what the eye actually reads.
 */

type Rgb = [number, number, number];
type Theme = Map<string, string>;
type Fail = (label: string, ratio: number) => void;

const css = appCss.replace(/\/\*[\s\S]*?\*\//g, "");

function block(selector: string): Theme {
  const start = css.indexOf(`${selector} {`);
  if (start < 0) throw new Error(`no block ${selector}`);
  const body = css.slice(css.indexOf("{", start) + 1, css.indexOf("}", start));
  return new Map([...body.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)].map(([, k, v]) => [k, v.trim()]));
}

const light = block(":root");
const dark = new Map([...light, ...block(':root[data-theme="dark"]')]);
const palettes: [string, Theme][] = COLOR_THEMES.map((id) => [
  id,
  new Map([...dark, ...block(`:root[data-theme="dark"][data-palette="${id}"]`)]),
]);
const THEMES: [string, Theme][] = [["light", light], ["dark", dark], ...palettes];

function value(theme: Theme, name: string): string {
  const raw = theme.get(name);
  if (raw === undefined) throw new Error(`${name} is not defined`);
  const ref = /^var\((--[\w-]+)\)$/.exec(raw);
  return ref ? value(theme, ref[1]) : raw;
}

function colour(theme: Theme, name: string): Rgb {
  const v = value(theme, name);
  const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(v);
  if (!m) throw new Error(`${name} is ${v}, not #rrggbb`);
  return [parseInt(m[1], 16), parseInt(m[2], 16), parseInt(m[3], 16)];
}

function channels(theme: Theme, name: string): Rgb {
  const parts = value(theme, name).split(/\s+/).map(Number);
  if (parts.length !== 3 || parts.some(Number.isNaN)) throw new Error(`${name} is not "r g b"`);
  return parts as Rgb;
}

function wash(rgb: Rgb, alpha: number, under: Rgb): Rgb {
  return rgb.map((c, i) => Math.round(c * alpha + under[i] * (1 - alpha))) as Rgb;
}

function luminance([r, g, b]: Rgb): number {
  const lin = (c: number) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

function ratio(a: Rgb, b: Rgb): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

const SURFACES = ["--page-bg", "--surface-bg", "--surface-sunken", "--table-head-bg", "--sidebar-bg", "--hover-bg"];
const TEXTS = ["--text", "--text-secondary", "--text-muted"];
const HUES = ["mint", "blue", "indigo", "violet", "magenta", "orange", "amber", "green", "teal", "cyan", "slate"];
const STATUSES = ["success", "warning", "danger"];
const CATEGORIES = ["teal", "sand", "sky", "periwinkle", "coral", "blue", "purple", "green"];
const STATUS_WASH = 0.12;
const ACCENT_WASH = 0.12;

function failures(check: (theme: Theme, fail: Fail) => void): string[] {
  const out: string[] = [];
  for (const [name, theme] of THEMES) {
    check(theme, (label, r) => out.push(`${name} ${label}: ${r.toFixed(2)}`));
  }
  return out;
}

function atLeast(r: number, min: number, label: string, fail: Fail) {
  if (r < min) fail(label, r);
}

describe("contrast", () => {
  it("reads App.css", () => {
    expect(appCss.length).toBeGreaterThan(1000);
    expect(light.size).toBeGreaterThan(50);
  });

  it("gives every colour theme its ground and its accent", () => {
    for (const [, theme] of palettes) {
      for (const name of ["--page-bg", "--surface-bg", "--text", "--accent", "--accent-on-solid"]) {
        expect(theme.get(name)).not.toBe(dark.get(name));
      }
    }
  });

  it("text tokens read on every surface", () => {
    expect(
      failures((theme, fail) => {
        for (const s of SURFACES)
          for (const t of TEXTS) atLeast(ratio(colour(theme, t), colour(theme, s)), 4.5, `${t} on ${s}`, fail);
      }),
    ).toEqual([]);
  });

  it("every accent reads as text, on its wash, and under its ink", () => {
    expect(
      failures((theme, fail) => {
        for (const h of HUES) {
          const text = colour(theme, `--c-${h}-text`);
          for (const s of SURFACES) atLeast(ratio(text, colour(theme, s)), 4.5, `--c-${h}-text on ${s}`, fail);
          const washed = wash(channels(theme, `--c-${h}-rgb`), ACCENT_WASH, colour(theme, "--surface-bg"));
          atLeast(ratio(text, washed), 4.5, `--c-${h}-text on its wash`, fail);
          const fill = theme.has(`--c-${h}-solid`) ? `--c-${h}-solid` : `--c-${h}`;
          atLeast(ratio(colour(theme, `--c-${h}-on-solid`), colour(theme, fill)), 4.5, `--c-${h}-on-solid on ${fill}`, fail);
        }
      }),
    ).toEqual([]);
  });

  it("the accent in force reads as text, on its wash, and under its ink", () => {
    expect(
      failures((theme, fail) => {
        const text = colour(theme, "--accent-text");
        for (const s of SURFACES) atLeast(ratio(text, colour(theme, s)), 4.5, `--accent-text on ${s}`, fail);
        const washed = wash(channels(theme, "--accent-rgb"), ACCENT_WASH, colour(theme, "--surface-bg"));
        atLeast(ratio(text, washed), 4.5, "--accent-text on its wash", fail);
        atLeast(ratio(colour(theme, "--accent-on-solid"), colour(theme, "--accent-solid")), 4.5, "--accent-on-solid", fail);
      }),
    ).toEqual([]);
  });

  it("status text reads on its own pill", () => {
    expect(
      failures((theme, fail) => {
        for (const s of STATUSES) {
          const washed = wash(channels(theme, `--${s}-rgb`), STATUS_WASH, colour(theme, "--surface-bg"));
          atLeast(ratio(colour(theme, `--${s}-text`), washed), 4.5, `--${s}-text on its pill`, fail);
        }
      }),
    ).toEqual([]);
  });

  it("categorical hues read on their own wash", () => {
    expect(
      failures((theme, fail) => {
        const alpha = Number(value(theme, "--cat-wash-alpha"));
        for (const c of CATEGORIES) {
          const washed = wash(channels(theme, `--cat-${c}-rgb`), alpha, colour(theme, "--surface-bg"));
          atLeast(ratio(colour(theme, `--cat-${c}`), washed), 4.5, `--cat-${c} on its wash`, fail);
        }
      }),
    ).toEqual([]);
  });

  it("SQL syntax reads on the editor", () => {
    const SQL = ["text", "keyword", "type", "builtin", "string", "identifier", "number", "comment", "operator", "punctuation", "warning", "error"];
    expect(
      failures((theme, fail) => {
        for (const k of SQL) atLeast(ratio(colour(theme, `--sql-${k}`), colour(theme, "--sql-bg")), 4.5, `--sql-${k}`, fail);
      }),
    ).toEqual([]);
  });

  it("terminal colours read on the terminal", () => {
    const ANSI = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];
    // The colours that exist to be the background's own shade are not meant to be read on it.
    const EXEMPT: Record<string, string[]> = {
      light: ["--ansi-white", "--ansi-bright-white"],
      dark: ["--ansi-black"],
      ...Object.fromEntries(COLOR_THEMES.map((id) => [id, ["--ansi-black"]])),
    };
    const out: string[] = [];
    for (const [name, theme] of THEMES) {
      const names = ["--ansi-foreground", ...ANSI.map((c) => `--ansi-${c}`), ...ANSI.map((c) => `--ansi-bright-${c}`)];
      for (const n of names) {
        if (EXEMPT[name].includes(n)) continue;
        const r = ratio(colour(theme, n), colour(theme, "--ansi-background"));
        if (r < 4.5) out.push(`${name} ${n}: ${r.toFixed(2)}`);
      }
    }
    expect(out).toEqual([]);
  });

  it("names mint as the default accent", () => {
    expect(value(light, "--accent")).toBe(value(light, "--c-mint"));
    // The ink the default accent carries is only checked against this fill.
    expect(value(light, "--accent-solid")).toBe(value(light, "--c-mint-solid"));
  });
});
