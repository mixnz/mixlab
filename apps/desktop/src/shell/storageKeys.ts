/**
 * The window's own `localStorage` keys, in one place and free of side effects — T176g.
 *
 * `public/storage-keys.js` moves each from the name the standalone client gave it, before any of
 * the readers below runs; `storageKeys.test.ts` holds the two lists equal.
 */
export const THEME_KEY = "mixlab-theme";
/** The accent picker's choice, read by nothing since colour themes replaced it; kept as a name
 *  because its presence still says an older build ran here (`profiles.ts`). */
export const ACCENT_KEY = "mixlab-accent";
/** The colour theme in force when the theme is *Colour*; absent means the first one. A key the
 *  standalone client never had, so `public/storage-keys.js` has nothing to move for it. */
export const PALETTE_KEY = "mixlab-palette";
export const LANGUAGE_KEY = "mixlab-lang";
export const MODULES_KEY = "mixlab-modules";
/** Which preset `MODULES_KEY` is, `custom` for a set of somebody's own, absent on a home no build of
 *  T203 has written. A key the standalone client never had, so `public/storage-keys.js` has nothing
 *  to move for it. */
export const MODULES_PRESET_KEY = "mixlab-modules-preset";
export const SESSION_KEY = "mixlab-session";
