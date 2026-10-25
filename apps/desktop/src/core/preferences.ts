/**
 * "Something rewrote a preference in `localStorage` behind the app's back" — sync, arriving from
 * another machine. Theme, accent, language and the enabled modules each re-read theirs.
 *
 * In `core/` because `i18n/` must hear it too, and `i18n/` imports nothing from `shell/`.
 */
const EVENT = "mixlab:preferences-changed";

export function announcePreferencesChanged(): void {
  window.dispatchEvent(new Event(EVENT));
}

export function onPreferencesChanged(listener: () => void): () => void {
  window.addEventListener(EVENT, listener);
  return () => window.removeEventListener(EVENT, listener);
}
