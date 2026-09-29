/** A row in the scroll box: its `offsetTop` and `offsetHeight`. */
export interface RowBox {
  top: number;
  height: number;
}

/** The scroll box: how far it is scrolled, and how tall it is (`clientHeight`). */
export interface ViewBox {
  scrollTop: number;
  height: number;
}

/**
 * Where the scroll box has to scroll to for a row to be visible, or `null` when no scrolling is
 * needed.
 *
 * `headerHeight` is the top part covered by the sticky header. `scrollIntoView({block:"nearest"})`
 * knows nothing about it: it scrolls the row to the box's very top edge, and that top edge is
 * behind the header — after scrolling, the thing just scrolled to is still unreadable. So the
 * arithmetic lives here.
 *
 * Pure, and takes numbers rather than elements: a call from `DbTab` can run in a test without a
 * browser, and all `DbTab` still has to do is read four numbers out of the DOM.
 */
export function scrollTopFor(row: RowBox, view: ViewBox, headerHeight: number): number | null {
  /** The top and bottom edges of the part really visible, in the box's content coordinates. */
  const top = view.scrollTop + headerHeight;
  const bottom = view.scrollTop + view.height;

  let target: number;
  if (row.top < top) {
    // Above: bring it down until it sits just below the header.
    target = row.top - headerHeight;
  } else if (row.top + row.height > bottom) {
    /* Below: bring it up just enough to see all of it. For a row taller than the frame this pushes
       its top out instead, and the top — the name — is the part worth seeing; so it aligns to the
       top edge. */
    target =
      row.height > view.height - headerHeight
        ? row.top - headerHeight
        : row.top + row.height - view.height;
  } else {
    return null;
  }

  target = Math.max(0, target);
  // If clamping lands on where it already is, there is nothing to scroll — the first row sitting
  // partly behind the header is this case, and scrolling cannot fix it.
  return target === view.scrollTop ? null : target;
}
