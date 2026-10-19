import { useEffect, useLayoutEffect, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { enterModal } from "../core/shortcuts";
import { useContextMenuPosition } from "./contextMenuPosition";
import { nextFocusIndex } from "./Modal";

interface Props {
  /** Where the pointer was when the menu was asked for, in client coordinates. */
  x: number;
  y: number;
  /** Called for every way out that isn't choosing an entry: a press outside, Escape, a scroll.
   * Choosing an entry is the caller's own business, since only it knows what the entry does. */
  onClose: () => void;
  /** The entries — plain `<button>`s, which `.context-menu` in `chrome.css` styles. */
  children: ReactNode;
}

/**
 * The menu a right-click opens, hung off the pointer and kept inside the window.
 *
 * Rendered out at the body rather than where it was asked for: every caller sits in a pane that
 * scrolls, and a menu drawn inside one is clipped by it. Being at the body is also what makes the
 * window, not the pane, the thing it is fitted against — see {@link useContextMenuPosition}.
 *
 * The rest of the app stays live while it is open. Nothing is laid over the window to catch the
 * dismissing click: a press outside closes the menu and then goes on to do whatever it would have
 * done anyway — pick another row, press a button, right-click somewhere else and get that menu.
 * The alternative costs a press for every menu opened by mistake.
 */
function ContextMenu({ x, y, onClose, children }: Props) {
  const { ref, style } = useContextMenuPosition(x, y);

  /* An open menu is about to act on a selection, so it holds the keyboard the same way a dialog
     does — which is what lets the grid stop keeping its own `menu !== null` guard for `Ctrl+A`. */
  useEffect(() => enterModal(), []);

  /**
   * The entries say what they are, and the menu can be walked with the arrow keys.
   *
   * The roles are set on the elements rather than passed down as props because the entries are the
   * caller's own JSX — plain `<button>`s, arriving through maps, fragments and conditionals — and
   * there is no prop this component could add to all of them. A layout effect rather than a
   * passive one, so nothing is ever painted without them.
   */
  useLayoutEffect(() => {
    for (const item of entries()) item.setAttribute("role", "menuitem");
  });

  /** The entries, in the order they are drawn. Disabled ones are skipped: an entry that cannot be
   *  chosen is not somewhere to stop on the way to one that can. */
  function entries(): HTMLButtonElement[] {
    return [...(ref.current?.querySelectorAll<HTMLButtonElement>("button:not([disabled])") ?? [])];
  }

  /* Focus goes on the menu itself, not on its first entry: the menu is usually opened by pointer,
     and pre-selecting an entry in a menu full of destructive ones invites an Enter that was meant
     for something else. An arrow key is what steps in — the same key that would in any menu. */
  useEffect(() => {
    const returnTo = document.activeElement as HTMLElement | null;
    const menu = ref.current;
    menu?.focus();
    return () => {
      /* Only when nothing else has taken the keyboard. A menu dismissed by a press somewhere else
         has already handed focus to whatever was pressed, and taking it back would undo that;
         asking whether the menu still holds it does not work, because by the time this runs the
         menu is off the document and `activeElement` has fallen back to `<body>` either way. */
      const now = document.activeElement;
      if ((now === null || now === document.body) && returnTo?.isConnected) returnTo.focus();
    };
  }, [ref]);

  function onMenuKeyDown(e: React.KeyboardEvent) {
    const items = entries();
    if (items.length === 0) return;
    const current = items.indexOf(document.activeElement as HTMLButtonElement);
    let next: number | null = null;
    if (e.key === "ArrowDown") next = nextFocusIndex(items.length, current, false);
    else if (e.key === "ArrowUp") next = nextFocusIndex(items.length, current, true);
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = items.length - 1;
    if (next === null) return;
    e.preventDefault();
    items[next].focus();
  }

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    /**
     * Closes on a press anywhere but in the menu, without swallowing it.
     *
     * Watched from the capture phase so the menu is gone before the handler under the pointer
     * runs, and on `pointerdown` rather than `click` so a press that turns into a drag — a column
     * being resized, a pane divider — closes it too. The press that *opened* the menu cannot be
     * caught here: `contextmenu` follows it, so this listener does not exist yet.
     */
    function onPointerDown(e: PointerEvent) {
      if (!ref.current?.contains(e.target as Node)) onClose();
    }
    // The menu hangs at a point in the window, not at the row it was opened from, so anything that
    // moves what is underneath leaves it pointing at the wrong thing. A menu long enough to scroll
    // inside itself is the one exception.
    function onScroll(e: Event) {
      if (e.target !== ref.current) onClose();
    }
    function onResize() {
      onClose();
    }
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", onResize);
    };
  }, [onClose, ref]);

  return createPortal(
    <div
      className="context-menu"
      ref={ref}
      style={style}
      role="menu"
      /* Somewhere for focus to land without becoming a Tab stop of its own. */
      tabIndex={-1}
      onKeyDown={onMenuKeyDown}
    >
      {children}
    </div>,
    document.body,
  );
}

export default ContextMenu;
