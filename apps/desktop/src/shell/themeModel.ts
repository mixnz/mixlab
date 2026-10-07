/**
 * The parts of the theme that are decisions rather than side effects, kept free of `document`,
 * `window` and `localStorage` so they run under Vitest's node environment.
 */

/** Keys earlier builds wrote that nothing reads any more. */
export const RETIRED_KEYS: readonly string[] = ["mixdb-glass"];

/** Forgets settings whose feature is gone, so a stale value cannot surprise a later build. */
export function clearRetiredKeys(storage: Pick<Storage, "removeItem">): void {
  for (const key of RETIRED_KEYS) storage.removeItem(key);
}

/** Light, Dark, the OS's choice of the two, or one of the colour themes below. */
export type ThemeMode = "light" | "dark" | "system" | "color";

export type ResolvedTheme = "light" | "dark";

/** The colour themes, in the order Settings shows them. Each is a dark ground with its own accent;
 *  the palettes live at the foot of App.css. Navy is the dark MixLab had before the redesign. */
export const COLOR_THEMES = [
  "navy",
  "dim",
  "midnight",
  "forest",
  "plum",
  "ocean",
  "ember",
  "rose",
  "graphite",
  "nord",
] as const;

export type ColorTheme = (typeof COLOR_THEMES)[number];

export const DEFAULT_COLOR_THEME: ColorTheme = "navy";

/** A stored mode, or *system* for anything this build does not know — absent included. */
export function parseThemeMode(stored: string | null): ThemeMode {
  return stored === "light" || stored === "dark" || stored === "color" ? stored : "system";
}

/** A stored colour theme, or the first one for anything this build does not know. */
export function parseColorTheme(stored: string | null): ColorTheme {
  return (COLOR_THEMES as readonly string[]).includes(stored ?? "") ? (stored as ColorTheme) : DEFAULT_COLOR_THEME;
}

/** What `data-theme` carries for a stored preference. *System* is settled here, against the OS,
 *  so no stylesheet has to restate its dark rules under a media query; a colour theme is dark
 *  underneath, so every rule written for dark holds for it too. */
export function resolveTheme(mode: ThemeMode, prefersDark: boolean): ResolvedTheme {
  if (mode === "system") return prefersDark ? "dark" : "light";
  if (mode === "color") return "dark";
  return mode;
}

/** What `data-palette` carries: the colour theme under *Colour*, nothing under the other three. */
export function resolvePalette(mode: ThemeMode, colorTheme: ColorTheme): ColorTheme | null {
  return mode === "color" ? colorTheme : null;
}
