import type { ReactNode } from "react";

/**
 * One button at the foot of a dialog. Data rather than markup, so a dialog says *what* a button is
 * and `Modal` decides how it looks and where it sits — the same for every dialog in the app.
 */
export interface ModalAction {
  /** What the button is in this dialog. Decides its look and which side of the row it sits on. */
  kind: "cancel" | "confirm" | "danger" | "secondary";
  label: ReactNode;
  /** Omitted on a `cancel`: it closes the dialog through `onClose`, like the ✕ and Escape. */
  onClick?: () => void;
  /** Run `onClick` after the exit animation rather than at the click — for an answer that ends the
   *  dialog. A `cancel` always does. */
  closes?: boolean;
  disabled?: boolean;
  /** `Button`'s busy label ("Saving…"): the button locks and shows it while a request runs. */
  busy?: string;
  icon?: ReactNode;
  autoFocus?: boolean;
  /** `data-demo` on the button, for the promotional clips (`demo/clips.mjs`). Inert everywhere else. */
  demo?: string;
}

export type ActionVariant = "default" | "primary" | "danger";

export function actionVariant(kind: ModalAction["kind"]): ActionVariant {
  if (kind === "confirm") return "primary";
  if (kind === "danger") return "danger";
  return "default";
}

/**
 * The row's two groups. The answer — `confirm` or `danger` — is always last on the right with
 * `cancel` just before it; tools that do not answer the dialog (`secondary`) go left. A row with no
 * answer and no cancel puts its secondaries on the right, where a lone button is read as the
 * dialog's own. Order inside a group follows the array; order across kinds does not.
 */
export function arrangeActions(actions: readonly ModalAction[]): {
  start: ModalAction[];
  end: ModalAction[];
} {
  const secondary = actions.filter((a) => a.kind === "secondary");
  const cancel = actions.filter((a) => a.kind === "cancel");
  const answer = actions.filter((a) => a.kind === "confirm" || a.kind === "danger");
  if (cancel.length === 0 && answer.length === 0) return { start: [], end: secondary };
  return { start: secondary, end: [...cancel, ...answer] };
}
