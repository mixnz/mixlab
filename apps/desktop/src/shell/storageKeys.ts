/**
 * The window's own `localStorage` keys, in one place and free of side effects — T176g.
 *
 * `public/storage-keys.js` moves each from the name the standalone client gave it, before any of
 * the readers below runs; `storageKeys.test.ts` holds the two lists equal.
 */
export const THEME_KEY = "mixlab-theme";
export const ACCENT_KEY = "mixlab-accent";
export const LANGUAGE_KEY = "mixlab-lang";
export const MODULES_KEY = "mixlab-modules";
export const SESSION_KEY = "mixlab-session";
