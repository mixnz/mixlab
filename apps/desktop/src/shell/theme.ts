import { useEffect, useState } from "react";
import { onPreferencesChanged } from "../core/preferences";
import { IS_MAC, IS_WINDOWS } from "../core/platform";
import { ACCENT_KEY as ACCENT_STORAGE_KEY, THEME_KEY as STORAGE_KEY } from "./storageKeys";
import { clearRetiredKeys, resolveTheme } from "./themeModel";

export type ThemeMode = "light" | "dark" | "system";

/** The accent a user can pick; the palette each one resolves to lives in App.css. */
export type AccentColor =
  | "mint"
  | "blue"
  | "indigo"
  | "violet"
  | "magenta"
  | "orange"
  | "amber"
  | "green"
  | "teal"
  | "cyan"
  | "slate";

export const ACCENT_COLORS: AccentColor[] = [
  "mint",
  "blue",
  "indigo",
  "violet",
  "magenta",
  "orange",
  "amber",
  "green",
  "teal",
  "cyan",
  "slate",
];

const DEFAULT_ACCENT: AccentColor = "mint";

function readStoredTheme(): ThemeMode {
  const stored = localStorage.getItem(STORAGE_KEY);
  return stored === "light" || stored === "dark" ? stored : "system";
}

function readStoredAccent(): AccentColor {
  const stored = localStorage.getItem(ACCENT_STORAGE_KEY);
  return ACCENT_COLORS.includes(stored as AccentColor) ? (stored as AccentColor) : DEFAULT_ACCENT;
}

const DARK_QUERY = "(prefers-color-scheme: dark)";

/* The attribute always names a theme; only the stored preference remembers that it was *system*. */
function applyTheme(theme: ThemeMode): void {
  const prefersDark = window.matchMedia(DARK_QUERY).matches;
  document.documentElement.setAttribute("data-theme", resolveTheme(theme, prefersDark));
  if (theme === "system") {
    localStorage.removeItem(STORAGE_KEY);
  } else {
    localStorage.setItem(STORAGE_KEY, theme);
  }
}

/* A theme the user picks fades in rather than landing in one frame: a whole window flipping from
   dark to light at once is hard on the eyes. The browser snapshots the old page and cross-fades to
   the new one, so the terminal's canvas fades with everything else. The timing lives in App.css.
   Only the click is animated — the first paint, a synced preference and the OS switching under
   *system* still apply at once. A webview without view transitions, or a user who asked for less
   motion, gets the switch as it always was. */
function applyThemeSmoothly(theme: ThemeMode): void {
  const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  if (reduced || typeof document.startViewTransition !== "function") {
    applyTheme(theme);
    return;
  }
  document.startViewTransition(() => applyTheme(theme));
}

/* The default is what `:root` already carries, so the attribute is left off for it rather than
   written out, which keeps the DOM clean for the common case. */
function applyAccent(accent: AccentColor): void {
  const root = document.documentElement;
  if (accent === DEFAULT_ACCENT) {
    root.removeAttribute("data-accent");
    localStorage.removeItem(ACCENT_STORAGE_KEY);
  } else {
    root.setAttribute("data-accent", accent);
    localStorage.setItem(ACCENT_STORAGE_KEY, accent);
  }
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
   paint, otherwise the window flashes the default accent on every launch.

   The theme is here too, and it is the one of them that is applied twice: `theme-preload.js`
   sets it earlier still, before the bundle has even been fetched, which is what stops a dark app
   flashing white while it loads. That file is the optimisation and this is the guarantee — a
   preload that 404s, is blocked, or is dropped by a future change to `index.html` would otherwise
   leave the theme unset for the whole session, and nothing would look wrong enough at build time
   to notice. Applying it again costs one attribute write against a value that is already there. */
applyPlatform();
applyTheme(readStoredTheme());
applyAccent(readStoredAccent());
clearRetiredKeys(localStorage);

/* Under *system* the window follows the OS while it is open, not only when it starts. */
window.matchMedia(DARK_QUERY).addEventListener("change", () => {
  if (readStoredTheme() === "system") applyTheme("system");
});

/* Another machine's preference, written by sync (T177d): the page follows at once. */
onPreferencesChanged(() => {
  applyTheme(readStoredTheme());
  applyAccent(readStoredAccent());
});

export function useTheme(): [ThemeMode, (theme: ThemeMode) => void] {
  const [theme, setTheme] = useState<ThemeMode>(readStoredTheme);
  useEffect(() => onPreferencesChanged(() => setTheme(readStoredTheme())), []);

  function updateTheme(next: ThemeMode) {
    applyThemeSmoothly(next);
    setTheme(next);
  }

  return [theme, updateTheme];
}

export function useAccent(): [AccentColor, (accent: AccentColor) => void] {
  const [accent, setAccent] = useState<AccentColor>(readStoredAccent);
  useEffect(() => onPreferencesChanged(() => setAccent(readStoredAccent())), []);

  function updateAccent(next: AccentColor) {
    applyAccent(next);
    setAccent(next);
  }

  return [accent, updateAccent];
}
