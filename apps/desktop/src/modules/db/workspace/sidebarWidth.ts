import { useCallback, useEffect, useRef, useState } from "react";
import { clampSize } from "../../../components/Splitter";

/**
 * The sidebar's width, and the divider that changes it.
 *
 * Written once because the SQL, Mongo and Redis workspaces each had their own copy of it — the same
 * four listeners, the same clamp written out twice per copy, and the same trick for fitting the
 * sidebar to its longest name. The three differed in nothing but which list they measured and how
 * wide they let the pane get.
 *
 * The drag itself belongs to {@link Splitter}, which is where the listeners and their cleanup are.
 * What is left here is the part that is about a sidebar rather than about a divider: a width that
 * follows the saved one, a clamp, and the measurement behind the double-click.
 */

/** The bar's own right-hand padding, plus a little so the longest name is not flush against it. */
const SIDEBAR_PADDING = 4;

/**
 * Where the double-click lands: wide enough for the longest name, never narrower than the default
 * and never past the maximum.
 *
 * Apart from the measuring so it can be checked without a DOM — the arithmetic is the half that can
 * be wrong, and `Math.ceil` on a fractional text width is the difference between a name fitting and
 * a name ending in an ellipsis.
 */
export function fitWidth(
  textWidth: number,
  horizontalPadding: number,
  extraColumns: number,
  defaultWidth: number,
  maxWidth: number,
): number {
  const target = Math.ceil(textWidth + horizontalPadding + extraColumns + SIDEBAR_PADDING);
  return Math.min(maxWidth, Math.max(defaultWidth, target));
}

/**
 * How wide `text` is drawn when a sidebar row draws it, and how much padding that row adds.
 *
 * Measured off a real element rather than guessed at: the row's font comes from the stylesheet, so
 * the only way to know it is to make one, ask the browser what it computed, and take it away again.
 * `measureText` is used in preference to the element's own `scrollWidth` because it answers in
 * fractions of a pixel; the element is the fallback for a canvas context that cannot be had.
 */
function measure(text: string, itemClassName: string): { textWidth: number; padding: number } {
  const probe = document.createElement("button");
  probe.className = itemClassName;
  probe.style.position = "fixed";
  probe.style.top = "-9999px";
  probe.style.left = "-9999px";
  probe.style.width = "auto";
  probe.style.whiteSpace = "nowrap";
  probe.textContent = text;
  document.body.appendChild(probe);
  const style = getComputedStyle(probe);
  const canvas = document.createElement("canvas");
  const ctx = canvas.getContext("2d");
  let textWidth = probe.scrollWidth;
  if (ctx) {
    ctx.font = style.font;
    textWidth = ctx.measureText(text).width;
  }
  const padding = parseFloat(style.paddingLeft) + parseFloat(style.paddingRight);
  document.body.removeChild(probe);
  return { textWidth, padding };
}

export interface SidebarWidthOptions {
  /** The width remembered for this tab, or undefined before one has been. */
  saved: number | undefined;
  /** Where a width the user settled on is written down. Called on release, not during the drag. */
  onChange?: (width: number) => void;
  defaultWidth: number;
  minWidth: number;
  maxWidth: number;
  /** What is listed in the sidebar. The double-click fits the pane to the longest of them. */
  names: string[];
  /** The class one row is drawn with — what the measurement above is taken in. */
  itemClassName: string;
  /** Fixed-width things left of the name that measuring the text alone does not account for —
   *  Redis draws a chevron and a type badge there, the other two draw nothing. */
  extraColumns?: number;
}

export interface SidebarWidth {
  /** The pane's width right now, for its `flexBasis`. Moves during a drag. */
  width: number;
  /** Handed straight to `<Splitter>`. */
  splitter: {
    onDragStart: () => void;
    onDrag: (delta: number) => void;
    onDragEnd: (delta: number) => void;
    onDoubleClick: () => void;
  };
}

export function useSidebarWidth({
  saved,
  onChange,
  defaultWidth,
  minWidth,
  maxWidth,
  names,
  itemClassName,
  extraColumns = 0,
}: SidebarWidthOptions): SidebarWidth {
  const [width, setWidth] = useState(saved ?? defaultWidth);

  useEffect(() => {
    setWidth(saved ?? defaultWidth);
  }, [saved, defaultWidth]);

  /* Where the drag began. A ref rather than state because nothing draws it, and because reading it
     from the width would mean the callbacks below changed identity on every pixel of the drag. */
  const start = useRef(width);

  const onDragStart = useCallback(() => {
    start.current = width;
  }, [width]);

  const onDrag = useCallback(
    (delta: number) => setWidth(clampSize(start.current, delta, minWidth, maxWidth)),
    [minWidth, maxWidth],
  );

  /* The same arithmetic again rather than the width in state: the last `mousemove` and the `mouseup`
     can arrive at different places, and it is where the button was let go that gets written down. */
  const onDragEnd = useCallback(
    (delta: number) => {
      const settled = clampSize(start.current, delta, minWidth, maxWidth);
      setWidth(settled);
      onChange?.(settled);
    },
    [minWidth, maxWidth, onChange],
  );

  const onDoubleClick = useCallback(() => {
    // Nothing listed is nothing to fit to, so the double-click means "back to the default".
    if (names.length === 0) {
      setWidth(defaultWidth);
      onChange?.(defaultWidth);
      return;
    }
    const longest = names.reduce((a, b) => (b.length > a.length ? b : a), "");
    const { textWidth, padding } = measure(longest, itemClassName);
    const next = fitWidth(textWidth, padding, extraColumns, defaultWidth, maxWidth);
    setWidth(next);
    onChange?.(next);
  }, [names, itemClassName, extraColumns, defaultWidth, maxWidth, onChange]);

  return { width, splitter: { onDragStart, onDrag, onDragEnd, onDoubleClick } };
}
