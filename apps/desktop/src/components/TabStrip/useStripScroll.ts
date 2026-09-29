import { useCallback, useEffect, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { overflowState, scrollStep, type StripOverflow } from "./overflow";
import styles from "./TabStrip.module.css";

/**
 * A tab strip longer than the room it has: horizontal scrolling with the mouse, and two arrows
 * saying which side still has hidden tabs.
 *
 * Three small jobs share one hook because they look at the same element and the same three numbers
 * of it — split into three, each would measure again by itself.
 *
 * There is no scrollbar to see (see `TabStrip.module.css`), so the two arrows are the only thing
 * saying there are tabs outside the frame. They show and hide *together*, following `overflowing`,
 * and only fade following `atStart`/`atEnd`: an arrow adding and removing itself would narrow the
 * frame and then widen it again, and a tab strip sitting right at the boundary would flicker
 * between the two states.
 */
export interface StripScroll extends StripOverflow {
  /** `-1` is towards the left, `1` towards the right. */
  scrollBy: (direction: -1 | 1) => void;
}

const FITS: StripOverflow = { overflowing: false, atStart: true, atEnd: true };

/* -------------------------------------------------------------------------------------------
   THE GLIDE

   The tab strip travels to where the wheel notches just asked, one step per frame, instead of
   jumping there.

   `behavior: "smooth"` cannot do this. Calling it again while it is running *rebuilds* the ease
   curve from where the strip currently stands, so every notch breaks the velocity once — spin
   fast and every notch breaks it, and what the eye sees is stutter. The browser does not do that
   with Shift+wheel: it continues the running animation. An exponential approach can continue like
   that because it has no ease curve to rebuild — one more notch just moves the target, and the
   velocity stays continuous.

   This is exactly the glide `core/scroll.ts` runs for the vertical axis, with the same constants,
   and for the same reason written there. Two copies rather than one, because that one has two more
   jobs this one does not: working out which pane a notch belongs to, and stretching a notch when
   the wheel spins in a burst. The only overlap is the few lines below. */

/** The glide's time constant. */
const GLIDE_MS = 55;
/** Closer to the target than this, stop right at the target. */
const SETTLE_PX = 0.5;
/** How far the tab strip may drift from where the glide just put it before concluding something
 *  else is scrolling it — an arrow click, or a tab pulling itself into the frame. `scrollLeft` is
 *  snapped to device pixels, so it still drifts a fraction of a pixel even when nobody touches it.
 * */
const DRIFT_PX = 2;

/** A glide in progress. */
interface Glide {
  /** Where the notches so far, taken together, are asking to go. */
  target: number;
  /** The last `scrollLeft` this glide wrote itself, to recognise a scroll coming from elsewhere. */
  applied: number;
  /**
   * The latest frame that moved, on `requestAnimationFrame`'s clock — `null` until the first frame,
   * and that frame only takes the time rather than taking any step.
   *
   * `performance.now()` at the wheel notch is not used as the reference, even though the two
   * clocks share an origin: a frame's timestamp is when that frame *started*, which may be earlier
   * than the notch that just happened within that very frame. The interval comes out negative, and
   * a negative interval in the approach below makes the tab strip go backwards. And when it comes
   * out large, the first frame swallows the whole notch and the glide stops right there — every
   * notch becomes a jolt followed by a standstill, exactly what had to be fixed.
   */
  time: number | null;
  frame: number;
}

/** How far a tab strip can scroll at most. */
function maxScrollLeft(el: HTMLElement): number {
  return Math.max(0, el.scrollWidth - el.clientWidth);
}

/** The slot holding a tab strip's glide. Outside the hook because the three functions below only
 *  touch it and the element passed in — they capture nothing from a render, so there are no
 *  dependencies to declare and no stale copy to hold on to by mistake. */
type GlideRef = { current: Glide | null };

/** Drops the running glide, if any. Can be called even when there is none. */
function stopGlide(glide: GlideRef): void {
  if (glide.current !== null) cancelAnimationFrame(glide.current.frame);
  glide.current = null;
}

/** One step of the glide. */
function step(el: HTMLElement, glide: GlideRef, now: number): void {
  const g = glide.current;
  if (g === null) return;

  // The first frame only sets the clock. See `Glide.time`.
  if (g.time === null) {
    g.time = now;
    g.frame = requestAnimationFrame((t) => step(el, glide, t));
    return;
  }

  /* Something else just scrolled the tab strip — a tab pulling itself into the frame, a tab drag
     touching the edge. That wins, and this glide goes stale on the spot. */
  if (Math.abs(el.scrollLeft - g.applied) > DRIFT_PX) {
    stopGlide(glide);
    return;
  }

  // Tabs opened or closed midway change how far is left to go.
  g.target = Math.min(Math.max(g.target, 0), maxScrollLeft(el));

  const remaining = g.target - el.scrollLeft;
  if (Math.abs(remaining) < SETTLE_PX) {
    el.scrollLeft = g.target;
    stopGlide(glide);
    return;
  }

  // An exponential approach, computed on real frame time, so the glide takes exactly as long on a
  // 144Hz screen as on a 60Hz one.
  const before = el.scrollLeft;
  el.scrollLeft = before + remaining * (1 - Math.exp(-(now - g.time) / GLIDE_MS));
  if (el.scrollLeft === before) {
    // A step so small that rounding swallows it would keep this loop running forever.
    el.scrollLeft = g.target;
    stopGlide(glide);
    return;
  }
  g.applied = el.scrollLeft;
  g.time = now;
  g.frame = requestAnimationFrame((t) => step(el, glide, t));
}

/**
 * Moves the tab strip's target by `delta`, and glides there.
 *
 * One single door for both the wheel and the two arrows. Wheel notches arrive faster than frames,
 * so they add onto the target the glide is heading for; recomputing from where the strip currently
 * stands would swallow most of them, which is exactly where `behavior: "smooth"` breaks. Arrow
 * clicks take the same road rather than one of their own, because a click cutting in while the
 * wheel is in flight would drop the rest of the wheel's travel — and because two ways of scrolling
 * the same tab strip have no reason to feel different.
 *
 * Clamped, otherwise spinning all the way at one end builds a target far beyond it and the first
 * notch in the other direction does not budge.
 */
function glideBy(el: HTMLElement, glide: GlideRef, delta: number): void {
  const max = maxScrollLeft(el);
  if (max === 0) return;

  const g = glide.current;
  if (g !== null && Math.abs(el.scrollLeft - g.applied) <= DRIFT_PX) {
    g.target = Math.min(Math.max(g.target + delta, 0), max);
    return;
  }

  stopGlide(glide);
  const next: Glide = {
    target: Math.min(Math.max(el.scrollLeft + delta, 0), max),
    applied: el.scrollLeft,
    time: null,
    frame: 0,
  };
  glide.current = next;
  next.frame = requestAnimationFrame((t) => step(el, glide, t));
}

export function useStripScroll(scroller: RefObject<HTMLDivElement | null>): StripScroll {
  const [state, setState] = useState<StripOverflow>(FITS);
  /* The state being displayed, readable from inside the listener without rebuilding the listener
     every time it changes. `setState` with a new object on every measurement is an endless render
     loop, so the comparison lives here. */
  const shown = useRef(state);
  /** The running glide, or `null` when the tab strip is standing still. */
  const glide = useRef<Glide | null>(null);

  const measure = useCallback(() => {
    const el = scroller.current;
    // A box with no tab in it holds only `trailing`, which never counts as overflow — see
    // `overflowState`.
    const hasTabs = el !== null && el.querySelector(`.${styles.tab}`) !== null;
    const next = el === null ? FITS : overflowState(el, hasTabs);
    const was = shown.current;
    if (
      next.overflowing === was.overflowing &&
      next.atStart === was.atStart &&
      next.atEnd === was.atEnd
    ) {
      return;
    }
    shown.current = next;
    setState(next);
  }, [scroller]);

  /* After every render, because opening or closing a tab changes `scrollWidth` without changing any
     element's size — the `ResizeObserver` below sees nothing. As cheap as `useTabSlide` right next
     to it: three properties, and no `setState` when the three numbers give the same result. */
  useLayoutEffect(measure);

  useEffect(() => {
    const el = scroller.current;
    if (el === null) return;
    const stop = new AbortController();

    // Wherever it scrolls to, which arrow is off follows — including scrolls the two arrows cause
    // themselves.
    el.addEventListener("scroll", measure, { passive: true, signal: stop.signal });

    /* The mouse wheel has only one axis, and it is the axis the tab strip does not have. Notches go
       into the glide at the top of the file — blocking the default also blocks the animation the
       browser would run for a notch, so that part has to run on its own, and `glideBy` explains why
       it runs the way it does.

       `passive: false` because it has to block: otherwise the page behind scrolls along. A
       touchpad sends its own `deltaX` and is left alone — the browser already scrolls horizontally
       for it, and so for Shift+wheel, so both pass through here untouched.

       Only notches measured in pixels are taken: a `deltaY` in line or page mode is not the number
       `scrollLeft` needs, the same line `core/scroll.ts` draws. Every webview this app runs on
       sends pixels. */
    el.addEventListener(
      "wheel",
      (e) => {
        if (e.deltaY === 0 || e.ctrlKey || e.shiftKey || e.altKey || e.metaKey) return;
        if (e.deltaMode !== WheelEvent.DOM_DELTA_PIXEL) return;
        if (maxScrollLeft(el) === 0) return;
        e.preventDefault();
        glideBy(el, glide, e.deltaY);
      },
      { passive: false, signal: stop.signal },
    );

    // A narrower window means fewer tabs fit the frame, and the tab strip of a tab not in front is
    // 0 wide until it is looked at.
    const resize = new ResizeObserver(measure);
    resize.observe(el);
    return () => {
      stop.abort();
      resize.disconnect();
      stopGlide(glide);
    };
  }, [scroller, measure]);

  const scrollTo = useCallback(
    (direction: -1 | 1) => {
      const el = scroller.current;
      if (el === null) return;
      glideBy(el, glide, direction * scrollStep(el.clientWidth));
    },
    [scroller],
  );

  return { ...state, scrollBy: scrollTo };
}

/**
 * The open tab always sits inside the frame.
 *
 * `Ctrl+Tab` to the tenth one, or a new tab opening at the end of a strip that is already full,
 * leaves the selected tab outside the visible area — and there is no scrollbar left to say where it
 * is. Only runs when the open tab *changes*, not after every render: otherwise a user scrolling
 * across to look at another tab would be pulled straight back.
 *
 * `data-active` rather than a prop: `TabStrip` takes its tabs as `children` and does not know
 * which one is open — `Tab` does, and marks it on itself.
 */
export function useActiveTabInView(scroller: RefObject<HTMLDivElement | null>): void {
  const last = useRef<Element | null>(null);
  useLayoutEffect(() => {
    const el = scroller.current;
    if (el === null) return;
    const active = el.querySelector("[data-active]");
    if (active === last.current) return;
    last.current = active;
    /* A strip on a tab that is not in front is laid out at size 0 and everything in it piles up at
       the left edge — scrolling by what is measured there scrolls to a meaningless place. The same
       reason `useTabSlide` does not measure a hidden strip. */
    if (active === null || el.offsetParent === null) return;
    active.scrollIntoView({ block: "nearest", inline: "nearest" });
  });
}
