import { hasPrimaryModifier, shortcutLabel } from "./platform";
import { useShortcut } from "./shortcuts";

/**
 * `Ctrl+R` — `⌘R` on a Mac — taken off the webview and handed to the pane on screen.
 *
 * Every content pane in the app carries a reload button, and `Ctrl+R` is the key a user reaches for
 * to press it — except that the webview underneath answers first and reloads the whole app, which
 * drops every open connection, every unsaved query draft and every staged edit with it. So the
 * gesture is intercepted here: in a packaged build none of the reload keys reach the webview at
 * all, and the plain one presses the reload button of whichever pane the user is looking at — or,
 * with none listening, nothing.
 *
 * The hard reload and `F5` stay live under `npm run dev:app`: that build is being edited while it
 * runs, and taking away the developer's own way of picking up a change would be no kindness.
 * `Ctrl+R` is claimed there too, so what is being developed against is the behaviour that ships.
 */

/** The platform's own reload chord on its own — the gesture the panes answer. */
function isPaneReload(e: KeyboardEvent): boolean {
  return hasPrimaryModifier(e) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "r";
}

/** The rest of what a webview reloads itself on: the cache-skipping variant, and the function key
 *  with or without a modifier held over it. */
function isWebviewReload(e: KeyboardEvent): boolean {
  return e.key === "F5" || (hasPrimaryModifier(e) && e.key.toLowerCase() === "r");
}

/**
 * Whether this is a reload gesture the webview must not be left to act on, **other than the plain
 * `Ctrl+R`**.
 *
 * That one is `pane.reload` in the shortcut catalogue, which runs the pane's reload or, with no pane
 * listening, swallows it. Refusing it here as well would undo that: the listener calling this runs
 * before the dispatcher's whenever the dispatcher has been re-added, and the dispatcher steps aside
 * for an event already prevented.
 */
export function isBlockedReload(e: KeyboardEvent): boolean {
  return import.meta.env.PROD && isWebviewReload(e) && !isPaneReload(e);
}

/** How the shortcut is written in a tooltip, in the platform's own spelling — see
 *  {@link shortcutLabel}. */
export const RELOAD_SHORTCUT = shortcutLabel("R");

/** Names a reload button after the key that also presses it. Without this the shortcut is one
 *  nothing on screen mentions, which is the same as one nobody has. */
export function withReloadShortcut(label: string): string {
  return `${label} (${RELOAD_SHORTCUT})`;
}

/**
 * Presses this pane's reload button on `Ctrl+R`, for as long as `active` says the pane is the one
 * being looked at.
 *
 * `active` is what keeps the key unambiguous: plenty of panes are mounted at once — the connection
 * tabs sitting in the background, the stats grid kept behind the data grid — and every one of them
 * would otherwise answer the same keystroke together.
 *
 * A dialog standing over the pane is answered centrally now: `pane.reload` is not marked `inModal`,
 * so anything open holds the key on its own. Call sites still pass their own dialogs in, and are
 * right to — a pane knows things about its own state that a modal count does not, and the flag
 * reads the same as the `disabled` on the button beside it.
 *
 * Kept as a named hook rather than a bare `useShortcut` call at each site: five panes say `Ctrl+R`
 * reloads me, and the name is where that fact and its reasons are written down.
 */
export function useReloadShortcut(active: boolean, reload: () => void): void {
  useShortcut("pane.reload", reload, active);
}
