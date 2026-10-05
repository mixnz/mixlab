import { useEffect, useRef, type CSSProperties, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "../../i18n";
import { CloseIcon } from "../../icons";
import Button from "../Button";
import { isUnhandledEscape, useDialogExit } from "../dialogMotion";
import { actionVariant, arrangeActions, type ModalAction } from "./actions";
import { FOCUSABLE, nextFocusIndex } from "./focus";
import surface from "./surface.module.css";

export type ModalSize = "small" | "normal" | "large";

interface ModalProps {
  /** The heading, drawn by `Modal` beside the ✕. */
  title: ReactNode;
  /** Read aloud in place of the dialog's own text. Defaults to `title` when that is a string. */
  label?: string;
  /** The width: `small` 460px, `normal` 600px, `large` 900px — never wider than the window. */
  size?: ModalSize;
  /** As tall as the height cap whatever it holds, for a body that scrolls a list or panes itself. */
  fixedHeight?: boolean;
  /** The z-index layer, for a dialog that must sit above or below the default 80. */
  layer?: number;
  /** The buttons at the foot. `Modal` picks their look, their size and their side — see
   *  `arrangeActions`. Omitted, the dialog has no action row. */
  actions?: readonly ModalAction[];
  /** A short live status at the left of the action row ("Waiting for the system prompt…"). */
  footerNote?: ReactNode;
  /** `false`: no ✕, and neither Escape nor the overlay closes it — a dialog the user must answer. */
  closable?: boolean;
  /** What the dialog closing means. Called after the exit animation, not at the key press. */
  onClose: () => void;
  /**
   * Nothing may close it for now — a request is in flight, and closing would leave the user with
   * no way to see how it went. Escape, the overlay and the ✕ go quiet; the dialog's own buttons are
   * the caller's to disable.
   */
  locked?: boolean;
  /**
   * What this dialog is asking, for a caller that keeps it mounted through its own answer.
   *
   * Changing it puts the dialog back up — that is the whole of its meaning, so give it whatever
   * says "this is a different question now" and nothing that merely changes while the same one is
   * being asked. A caller that unmounts on its answer, which is most of them, has no use for it.
   */
  question?: unknown;
  /** The dialog's body — a `ModalBody`, then a `ModalErrors`. Given `close`, which is how a control
   *  inside sees the dialog out with the same animation Escape does. */
  children: (close: (reply: () => void) => void) => ReactNode;
}

/**
 * A dialog: the overlay, the portal, Escape, the keyboard staying inside it — and the whole frame:
 * the width, the height cap, the title and its ✕, and the action row. A caller hands over its body
 * and a list of actions; how every dialog looks is decided here and in `surface.module.css`, once.
 *
 * Thirteen dialogs had their own copy of the first three. None of them had the fourth — focus was
 * left wherever it happened to be, Tab walked straight out into the page behind, and closing the
 * dialog left the user with no focus at all, which for anyone on a keyboard means starting again
 * from the top of the document.
 *
 * Rendered into `document.body` rather than in place: the dialog is fixed to the viewport, and a
 * caller deep inside a scrolling panel shouldn't have to care whether some ancestor of theirs
 * establishes a containing block for it.
 */
function Modal({
  title,
  label,
  size = "normal",
  fixedHeight,
  layer,
  actions,
  footerNote,
  closable = true,
  onClose,
  locked,
  question,
  children,
}: ModalProps) {
  const { t } = useTranslation();
  const { close, cls, onEntered } = useDialogExit(question);
  const dialog = useRef<HTMLDivElement | null>(null);
  const quiet = locked || !closable;

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (isUnhandledEscape(e) && !quiet) close(onClose);
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [close, onClose, quiet]);

  /**
   * Focus in on the way up, and back where it was on the way out.
   *
   * A caller's own `autoFocus` wins, and the check for that is the whole trick: React applies it
   * while it commits, which is before this effect runs, so a dialog that asked for a particular
   * field already has focus in it by now. Stepping in only when nothing inside took focus is what
   * lets both rules hold at once — the caller decides when it has an opinion, and a dialog with
   * none still opens with its first control ready rather than with focus on the page behind.
   *
   * Restoring is the half that is easy to leave out and the half a keyboard user feels: without
   * it, closing a dialog drops focus onto `<body>` and the next Tab starts at the top of the app.
   */
  useEffect(() => {
    const returnTo = document.activeElement as HTMLElement | null;
    const box = dialog.current;
    if (box && !box.contains(document.activeElement)) {
      (box.querySelector<HTMLElement>(FOCUSABLE) ?? box).focus();
    }
    return () => {
      // Only if it is still there to go back to — the control that opened the dialog may have been
      // a row the dialog itself has just deleted.
      if (returnTo?.isConnected) returnTo.focus();
    };
  }, []);

  /** Tab, kept inside. On the dialog rather than on the window, so a press that reaches here is
   *  one nothing inside the dialog wanted for itself. */
  function trapTab(e: React.KeyboardEvent) {
    if (e.key !== "Tab") return;
    const box = dialog.current;
    if (!box) return;
    const stops = [...box.querySelectorAll<HTMLElement>(FOCUSABLE)];
    const next = nextFocusIndex(stops.length, stops.indexOf(document.activeElement as HTMLElement), e.shiftKey);
    if (next < 0) return;
    e.preventDefault();
    stops[next].focus();
  }

  function press(action: ModalAction) {
    if (action.kind === "cancel") close(action.onClick ?? onClose);
    else if (action.closes && action.onClick) close(action.onClick);
    else action.onClick?.();
  }

  function button(action: ModalAction, index: number) {
    return (
      <Button
        key={index}
        size="large"
        variant={actionVariant(action.kind)}
        disabled={action.disabled}
        busy={action.busy}
        autoFocus={action.autoFocus}
        data-demo={action.demo}
        onClick={() => press(action)}
      >
        {action.icon}
        {action.label}
      </Button>
    );
  }

  const row = actions === undefined ? null : arrangeActions(actions);
  const layerStyle =
    layer === undefined ? undefined : ({ "--dialog-layer": layer } as CSSProperties);
  const panel = [surface.dialog, surface[size]];
  if (fixedHeight) panel.push(surface.fixedHeight);

  return createPortal(
    <>
      <div
        className={cls(surface.overlay)}
        style={layerStyle}
        onClick={quiet ? undefined : () => close(onClose)}
      />
      <div
        ref={dialog}
        className={cls(panel.join(" "))}
        style={layerStyle}
        role="dialog"
        aria-modal="true"
        aria-label={label ?? (typeof title === "string" ? title : undefined)}
        /* So focus has somewhere to land in a dialog with no controls of its own, and so the
           restore above has something to take it from. Not a Tab stop — see `FOCUSABLE`. */
        tabIndex={-1}
        onKeyDown={trapTab}
        onAnimationEnd={onEntered}
      >
        <div className={surface.header}>
          <h3 className={surface.title}>{title}</h3>
          {closable && (
            <Button
              variant="ghost"
              className={surface.close}
              disabled={locked}
              aria-label={t("common.close")}
              title={t("common.close")}
              onClick={() => close(onClose)}
            >
              <CloseIcon size={18} />
            </Button>
          )}
        </div>
        {children(close)}
        {(row !== null || footerNote !== undefined) && (
          <div className={surface.footer}>
            {(footerNote !== undefined || (row !== null && row.start.length > 0)) && (
              <div className={surface.start}>
                {footerNote !== undefined && <span className={surface.note}>{footerNote}</span>}
                {row?.start.map(button)}
              </div>
            )}
            {row !== null && row.end.length > 0 && (
              <div className={surface.end}>{row.end.map((action, i) => button(action, i + 100))}</div>
            )}
          </div>
        )}
      </div>
    </>,
    document.body,
  );
}

export default Modal;
