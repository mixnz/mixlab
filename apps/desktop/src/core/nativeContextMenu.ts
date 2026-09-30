/**
 * The webview's own right-click menu, taken off it.
 *
 * MixLab is a desktop application that happens to be drawn by a webview, and the menu the webview
 * offers on a right-click is a browser's: Back, Reload, Save as, Print, View source. None of it
 * means anything here, and two of the entries are actively harmful — a reload drops every open
 * connection, every unsaved query draft and every staged edit, exactly as `Ctrl+R` would if it were
 * not already claimed in {@link ./reload}. Worse, it appears in every corner of the app that has no
 * menu of its own, so what a right-click does depends on where it lands.
 *
 * So the default is refused everywhere, and the panes that have something to offer answer the
 * gesture themselves through their own `onContextMenu` — the sidebar's connections, the Redis keys,
 * the item lists. Those handlers see the event first and are unaffected by this one, which sits on
 * the document and only ever acts on what nothing else has claimed.
 *
 * Text fields are the exception: cut, copy and paste on a right-click are the webview's to give, no
 * part of the app replaces them, and a form you cannot paste a password into is worse than a stray
 * browser menu. Which elements those are is {@link ./textEntry}'s to say, and `Ctrl+A` asks it the
 * same question — a tickbox is an `<input>` and holds no text, so it keeps no menu.
 *
 * Selected text is the other exception, and for the same reason. Text the app only *shows* — an
 * error message, a row of a details panel, a value nobody can type into — is still text somebody
 * drags across and wants a copy of, and the right-click on top of their own selection is where they
 * ask for it. Refusing it there leaves `Ctrl+C` as the only way to take a copy of something the
 * user has already told the app they are interested in. The selection has to be *under the click*:
 * a right-click somewhere else while text happens to be selected elsewhere on screen is the
 * ordinary case this file exists for, and keeps no menu.
 *
 * Unlike the reload keys, this makes no exception for a development build, and the cost is known:
 * right-click → Inspect element goes with it, leaving `F12` and `Ctrl+Shift+C`. It is deliberate.
 * A right-click is a gesture users make, not a developer's tool, and one that behaved differently
 * under `npm run dev` would be one nobody was really developing against — a pane that forgot its
 * `preventDefault` would look right for as long as it was being worked on and only break once
 * packaged. `F5` is left alive there precisely because nobody but a developer presses it.
 */
import { isTextEntry } from "./textEntry";

/** The part of a `DOMRect` this file reads. Named so the geometry below can be tested without a
 *  layout engine to produce the real thing. */
type Bounds = { left: number; right: number; top: number; bottom: number };

/**
 * Whether a point falls inside any of a set of rectangles.
 *
 * The edges count as inside. A click is a whole pixel and a text rectangle is fractional, so a
 * click on the last character of a selection lands on the boundary often enough to matter, and
 * excluding it would refuse the menu exactly where the user was most deliberate.
 */
export function pointInRects(x: number, y: number, rects: Iterable<Bounds>): boolean {
  for (const r of rects) {
    if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) return true;
  }
  return false;
}

/**
 * Whether the click landed on the text the user has selected.
 *
 * Asked of the selection's own rectangles rather than of the element under the cursor, because a
 * partly selected paragraph answers the two questions differently: the element is the same one
 * either way, and only the geometry can tell the selected half from the rest of it. A selection
 * spanning several elements is several rectangles — one per line — and any of them will do.
 *
 * `rangeCount` is all but always 1: the webview keeps a single range, and only a scripted multiple
 * selection would make it more. The loop costs nothing and means this does not have to care.
 */
function isOnSelectedText(x: number, y: number): boolean {
  const selection = window.getSelection();
  if (!selection || selection.isCollapsed) return false;
  for (let i = 0; i < selection.rangeCount; i++) {
    if (pointInRects(x, y, selection.getRangeAt(i).getClientRects())) return true;
  }
  return false;
}

/** Refuses the webview's menu for the life of the window. Called once, at startup. */
export function blockNativeContextMenu(): void {
  document.addEventListener("contextmenu", (e) => {
    if (isTextEntry(e.target)) return;
    if (isOnSelectedText(e.clientX, e.clientY)) return;
    e.preventDefault();
  });
}
