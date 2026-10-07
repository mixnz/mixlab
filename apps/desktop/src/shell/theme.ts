import { useEffect, useState } from "react";
import { onPreferencesChanged } from "../core/preferences";
import { IS_MAC, IS_WINDOWS } from "../core/platform";
import { PALETTE_KEY, THEME_KEY as STORAGE_KEY } from "./storageKeys";
import {
  clearRetiredKeys,
  DEFAULT_COLOR_THEME,
  parseColorTheme,
  parseThemeMode,
  resolvePalette,
  resolveTheme,
  type ColorTheme,
  type ThemeMode,
} from "./themeModel";

export { COLOR_THEMES } from "./themeModel";
export type { ColorTheme, ThemeMode } from "./themeModel";

function readStoredTheme(): ThemeMode {
  return parseThemeMode(localStorage.getItem(STORAGE_KEY));
}

function readStoredColorTheme(): ColorTheme {
  return parseColorTheme(localStorage.getItem(PALETTE_KEY));
}

const DARK_QUERY = "(prefers-color-scheme: dark)";

/* The attribute always names a theme; only the stored preference remembers that it was *system*.
   A colour theme is `data-theme="dark"` plus `data-palette`, which App.css lays over dark. */
function applyTheme(theme: ThemeMode, colorTheme: ColorTheme = readStoredColorTheme()): void {
  const prefersDark = window.matchMedia(DARK_QUERY).matches;
  const root = document.documentElement;
  root.setAttribute("data-theme", resolveTheme(theme, prefersDark));
  const palette = resolvePalette(theme, colorTheme);
  if (palette === null) root.removeAttribute("data-palette");
  else root.setAttribute("data-palette", palette);
  if (theme === "system") {
    localStorage.removeItem(STORAGE_KEY);
  } else {
    localStorage.setItem(STORAGE_KEY, theme);
  }
}

/* The colour theme is remembered whatever the mode, so switching to Light and back to Colour
   returns to the one that was picked. The first is the absence of the key. */
function storeColorTheme(colorTheme: ColorTheme): void {
  if (colorTheme === DEFAULT_COLOR_THEME) localStorage.removeItem(PALETTE_KEY);
  else localStorage.setItem(PALETTE_KEY, colorTheme);
}

/* A theme the user picks fades in rather than landing in one frame: a whole window flipping from
   dark to light at once is hard on the eyes. The browser snapshots the old page and cross-fades to
   the new one, so the terminal's canvas fades with everything else. The timing lives in App.css.
   Only the click is animated — the first paint, a synced preference and the OS switching under
   *system* still apply at once. A webview without view transitions, or a user who asked for less
   motion, gets the switch as it always was. */
function applyThemeSmoothly(theme: ThemeMode, colorTheme: ColorTheme): void {
  const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  if (reduced || typeof document.startViewTransition !== "function") {
    applyTheme(theme, colorTheme);
    return;
  }
  document.startViewTransition(() => applyTheme(theme, colorTheme));
}

/* Which webview is drawing the window, as an attribute CSS can select on.
 *
 * Not a preference and not stored anywhere: it is written every load from the user agent, for the
 * rule that has a question neither a media query nor `@supports` can answer — what a given webview
 * actually draws. Every platform is named rather than just the one that needs it, so the next rule
 * that has to split does not have to add the other half of the answer first.
 *
 * Written out in full, unlike the three above, because there is no default to be the absence of:
 * a root with no `data-platform` is a root the script has not run on yet. */
function applyPlatform(): void {
  document.documentElement.setAttribute(
    "data-platform",
    IS_MAC ? "mac" : IS_WINDOWS ? "windows" : "linux",
  );
}

/* Read before React mounts: the stored choice has to be on the root element for the very first
   paint, otherwise the window flashes the default theme on every launch.

   The theme is here too, and it is the one of them that is applied twice: `theme-preload.js`
   sets it earlier still, before the bundle has even been fetched, which is what stops a dark app
   flashing white while it loads. That file is the optimisation and this is the guarantee — a
   preload that 404s, is blocked, or is dropped by a future change to `index.html` would otherwise
   leave the theme unset for the whole session, and nothing would look wrong enough at build time
   to notice. Applying it again costs one attribute write against a value that is already there. */
applyPlatform();
applyTheme(readStoredTheme());
clearRetiredKeys(localStorage);

/* Under *system* the window follows the OS while it is open, not only when it starts. */
window.matchMedia(DARK_QUERY).addEventListener("change", () => {
  if (readStoredTheme() === "system") applyTheme("system");
});

/* Another machine's preference, written by sync (T177d): the page follows at once. */
onPreferencesChanged(() => {
  applyTheme(readStoredTheme());
});

/** The mode and the colour theme together: Settings changes either, and both decide the page. */
export function useTheme(): {
  theme: ThemeMode;
  colorTheme: ColorTheme;
  setTheme: (theme: ThemeMode) => void;
  setColorTheme: (colorTheme: ColorTheme) => void;
} {
  const [theme, setThemeState] = useState<ThemeMode>(readStoredTheme);
  const [colorTheme, setColorThemeState] = useState<ColorTheme>(readStoredColorTheme);
  useEffect(
    () =>
      onPreferencesChanged(() => {
        setThemeState(readStoredTheme());
        setColorThemeState(readStoredColorTheme());
      }),
    [],
  );

  function setTheme(next: ThemeMode) {
    applyThemeSmoothly(next, colorTheme);
    setThemeState(next);
  }

  function setColorTheme(next: ColorTheme) {
    storeColorTheme(next);
    applyThemeSmoothly(theme, next);
    setColorThemeState(next);
  }

  return { theme, colorTheme, setTheme, setColorTheme };
}
