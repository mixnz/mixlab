/** What the *Remove MixLab* dialog draws, decided from values alone: T182a spec D4. */
import type { Plan, PlanRow } from "./api";

export type Stage = "checking" | "plan" | "removing" | "failed";

export interface Blocked {
  what: string;
  location: string;
  by: string;
}

/** The programs in the way: P2, found before anything is removed. */
export function blockedRows(plan: Plan): Blocked[] {
  return plan.items
    .filter((row) => row.outcome.removal === "blocked")
    .map((row) => ({ what: row.what, location: row.location, by: row.outcome.by ?? "" }));
}

/** The folders `[paths]` moved out of the home, for the second checkbox. */
export function relocatedRows(plan: Plan): string[] {
  return plan.items.filter((row) => row.id === "relocated_directory").map((row) => row.location);
}

/** Remove is offered only on a plan with nothing in the way. */
export function canRemove(stage: Stage, plan: Plan | null): boolean {
  return stage === "plan" && plan !== null && blockedRows(plan).length === 0;
}

/**
 * Nothing was removed and nothing is going: what a declined prompt leaves (ADR 0051, decision 3).
 * Read off the outcome rather than off the daemon's sentences: after a decline the privileged rows
 * are `failed`, still waiting for permission (T182b, D7), and a grant that failed before changing
 * anything reads the same way, which is the same thing to say.
 */
export function declined(report: Plan): boolean {
  const removal = (row: PlanRow) => row.outcome.removal;
  return !report.items.some(
    (row) => removal(row) === "removed" || removal(row) === "on_exit" || removal(row) === "on_restart",
  );
}

/** The rows the act left on the machine. */
export function failedRows(report: Plan): PlanRow[] {
  return report.items.filter((row) => row.outcome.removal === "failed");
}
