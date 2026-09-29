import type { PathReport } from "@mixengine/api";

/**
 * Whether the Dashboard reminds the user to put `<root>/bin` on PATH.
 *
 * **It is the machine's state, not a saved flag** — for the same reason as
 * `shouldOfferQuickStart`: someone who just ran `mix path uninstall` sees the reminder card come
 * back, and there is nothing to reset.
 *
 * `null` means the report has not arrived or failed to read, not "not installed".
 */
export function shouldOfferPathInstall(report: PathReport | null): boolean {
  return report !== null && !report.on_path;
}

/** Whether an install/uninstall actually wrote anywhere. */
export type PathOutcome = "changed" | "unchanged";

/**
 * Read from `PathPlace.changed` — the flag the daemon sets for exactly this question, so the client
 * can say "already there" instead of taking credit for a write it did not do. Only when some place
 * really changed does "open a new terminal" mean anything.
 */
export function pathOutcome(report: PathReport): PathOutcome {
  return report.places.some((place) => place.changed) ? "changed" : "unchanged";
}
