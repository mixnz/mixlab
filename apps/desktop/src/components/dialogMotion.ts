import { useCallback, useEffect, useRef, useState } from "react";
import { enterModal } from "../core/shortcuts";
import { exit, OPEN, type ExitState } from "./dialogExit";
import styles from "./dialogMotion.module.css";

/** Kept in step with the `.closing` animations in `dialogMotion.module.css`. */
const EXIT_MS = 130;

/**
 * Whether this Escape is still going spare, or has already been spent on something.
 *
 * A dialog listens on `window`, so it hears every Escape in the app — including the ones a control
 * inside it has already answered. A `Select` with its menu open answers Escape by closing that
 * menu and marks the press handled; without this the same press also closed the dialog around it,
 * which on `ColumnDialog` or `IndexDialog` meant a filled-in form thrown away for the sin of
 * picking a collation.
 *
 * `defaultPrevented` and not a list of elements to exclude: it is the one signal that means
 * *somebody has already dealt with this*, whoever they were, and it costs them nothing to raise.
 */
export function isUnhandledEscape(e: KeyboardEvent): boolean {
  return e.key === "Escape" && !e.defaultPrevented;
}

/**
 * Lets a dialog see itself out.
 *
 * Without this there is no exit animation to speak of: callers render a dialog as
 * `{open && <Dialog … />}`, so the moment it reports back it is gone from the tree and whatever
 * the stylesheet had to say about leaving never runs. So the dialog holds the answer for the
 * length of that animation and hands it over at the end — the caller is unchanged, and it is the
 * dialog that decides when it has finished.
 *
 * Only for the ways a dialog closes *itself* — Cancel, Escape, a click on the overlay. A dialog
 * that closes because a save succeeded is unmounted by its caller and goes at once, which is the
 * right answer there anyway: the user has been watching a "Saving…" button and wants the result,
 * not another frame of the form they are done with.
 *
 * **A dialog asked something new opens back up.** `question` is what it is asking — whatever the
 * caller would call a different question, `ConfirmDialog` its message — and a change in it is the
 * one sign, from out here, that a dialog its caller kept mounted through its own answer has more
 * to say. Left out, a dialog closes once and never again. See [`exit`](./dialogExit.ts).
 */
export function useDialogExit(question?: unknown, visible?: () => boolean) {
  /* Every dialog in the app calls this hook, which makes it the one place that knows a dialog is
     up — so it is where the count is kept. Ten dialogs, and not one of their files has to say so.
     The count is what a global shortcut asks before acting: the keyboard belongs to whatever is on
     top, and a reload fired blind from behind a form throws away what was being typed into it.

     It stays up through the exit animation, because the dialog is still on screen for those 130ms
     and still holds the keyboard. */
  // `visible`: a dialog drawn into a tab counts only while that tab is on screen (`Modal/host`).
  // eslint-disable-next-line react-hooks/exhaustive-deps -- read through a ref by the caller; counted once per mount
  useEffect(() => enterModal(visible), []);

  const [closing, setClosing] = useState(false);
  const [settled, setSettled] = useState(false);
  /* A ref and not state: a dialog hears Escape from the window and clicks from its own overlay,
     so two answers can arrive in one tick, and the second has to see the first. */
  const state = useRef<ExitState>(OPEN);

  /**
   * Fires once `dialog-in` has run its course. `transform: translate(-50%, -50%)` is meant to
   * recompute against the dialog's own size on every layout — that is the whole trick behind
   * centring it with no JS — but at least one WebKit build keeps the value the entrance animation
   * last interpolated instead, and never re-resolves it once a control inside the dialog (a
   * disclosure opening, say) changes its height. The dialog then sits off-centre until something
   * unrelated forces a style recalc (a hover, a resize). `animation-fill-mode: both` is exactly
   * the mechanism that leaves a value "held" past the animation's end, so once it has served its
   * purpose the fix is to take the animation off the element entirely — `.settled` below — which
   * hands `transform` back to the plain, always-live rule in `surface.module.css`.
   */
  const onEntered = useCallback(() => setSettled(true), []);

  /** Starts the exit; `reply` is called once it finishes. First call wins — a second Escape, or a
      click on the overlay behind a dialog already on its way out, is ignored. */
  const close = useCallback((reply: () => void) => {
    const answered = exit(state.current, { type: "answer", reply });
    // The state handed straight back means somebody answered first, and this press is theirs.
    if (answered === state.current) return;
    state.current = answered;
    setClosing(true);
  }, []);

  /**
   * Back up, for the caller that kept the dialog mounted and changed what it asks.
   *
   * Only ever after the answer has gone back — [`exit`](./dialogExit.ts) is what holds that line,
   * since reopening any earlier drops the reply the timer below is still holding and the
   * confirmed action never runs. And nothing here for the ordinary caller, the one that unmounts
   * on its answer: its question never changes, so the dialog leaves once and stays gone rather
   * than flashing back up while the request it started is still in the air.
   */
  useEffect(() => {
    const reopened = exit(state.current, { type: "asked" });
    if (reopened === state.current) return;
    state.current = reopened;
    setClosing(false);
  }, [question]);

  useEffect(() => {
    if (!closing) return;
    // With motion turned down there is nothing to wait for, so the answer goes back at once.
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const id = window.setTimeout(() => {
      const reply = state.current.answer;
      state.current = exit(state.current, { type: "delivered" });
      reply?.();
    }, reduced ? 0 : EXIT_MS);
    return () => window.clearTimeout(id);
  }, [closing]);

  /** Wraps a class in the shared `closing`/`settled` marker — `closing` while the dialog animates
   *  out, `settled` once the entrance animation has finished and stays that way until it does. The
   *  two never apply together: closing starts fresh from whatever `settled` was, and `.closing.dialog`
   *  in `dialogMotion.module.css` takes the animation back over regardless. */
  const cls = useCallback(
    (base: string) => {
      if (closing) return `${base} ${styles.closing}`;
      if (settled) return `${base} ${styles.settled}`;
      return base;
    },
    [closing, settled],
  );

  return { closing, close, cls, onEntered };
}
