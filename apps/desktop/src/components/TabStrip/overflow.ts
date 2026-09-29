/** What a horizontal scroll box says about itself. Three numbers, not an element — so this file
 *  can be tested without a DOM, just like `keyboard.ts`. */
export interface ScrollBox {
  scrollLeft: number;
  scrollWidth: number;
  clientWidth: number;
}

/** What the tab strip is still hiding at either end. */
export interface StripOverflow {
  /** Whether any tab does not fit the frame — what decides whether the two arrows are present. */
  overflowing: boolean;
  /** Already right at the left end: nothing is hidden on that side, so the left arrow cannot be
   *  clicked. */
  atStart: boolean;
  atEnd: boolean;
}

/* `scrollWidth` and `clientWidth` are rounded integers while `scrollLeft` is not, so a strip
   scrolled all the way often stands a fraction of a pixel short of the end. The same reason
   `core/scroll.ts` leaves exactly one pixel in `hasRoom`. */
const EPSILON = 1;

/**
 * Turns three numbers into two arrows.
 *
 * `hasTabs` false means the box holds nothing but `trailing`. That never counts as overflowing:
 * the arrows scroll tabs, and taking `trailing` out to make room for them would empty the box, make
 * it fit, and bring `trailing` back — a different answer on every render.
 */
export function overflowState(box: ScrollBox, hasTabs: boolean): StripOverflow {
  if (!hasTabs) return { overflowing: false, atStart: true, atEnd: true };
  const max = box.scrollWidth - box.clientWidth;
  if (max <= EPSILON) return { overflowing: false, atStart: true, atEnd: true };
  return {
    overflowing: true,
    atStart: box.scrollLeft <= EPSILON,
    atEnd: box.scrollLeft >= max - EPSILON,
  };
}

/** The part of the frame one arrow click travels. Keeps a little back so the eye can still catch
 *  the place just left. */
const STEP_RATIO = 0.8;

/** How many pixels one arrow click scrolls. At least one, because scrolling zero pixels is not
 *  scrolling. */
export function scrollStep(clientWidth: number): number {
  return Math.max(1, Math.round(clientWidth * STEP_RATIO));
}
