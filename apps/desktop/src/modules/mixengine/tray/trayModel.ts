import type { DaemonShutdown } from "@mixengine/api";

import type { ServiceRow } from "../daemonState";
import { toggleMode } from "../serviceStateLabel";

/**
 * The tray panel's pure halves — `docs/specs/2026-09-19-t168-mixengine-in-the-tray-design.md`.
 */

/**
 * What the panel asks about before doing (D4). Only *Stop MixEngine* today — *Stop all* acts at
 * once, as it does on the Dashboard — but the machine is written for more than one question, and
 * its tests hold it to that.
 */
export type Confirmable = "stopAll" | "shutdown";

/** How long a question waits for its answer before the row goes back to the button. */
export const CONFIRM_TIMEOUT_MS = 5000;

export type ConfirmEvent =
  | { type: "arm"; key: Confirmable }
  | { type: "cancel" }
  | { type: "timeout"; key: Confirmable }
  | { type: "hide" };

/**
 * Which question is on screen after `event`.
 *
 * **A timeout only clears the question it was started for.** Arm *Stop all*, cancel, arm *Stop
 * MixEngine* four seconds later: the first timer still fires, and taking the second question away
 * a second after it appeared would be answering for the person.
 */
export function nextConfirm(current: Confirmable | null, event: ConfirmEvent): Confirmable | null {
  switch (event.type) {
    case "arm":
      return event.key;
    case "timeout":
      return current === event.key ? null : current;
    case "cancel":
    case "hide":
      return null;
  }
}

/** "N of M services up": `up` is what the Dashboard's toggle would offer to stop. */
export function serviceCounts(rows: ServiceRow[]): { up: number; total: number } {
  return {
    up: rows.filter((row) => toggleMode(row.state, false) === "up").length,
    total: rows.length,
  };
}

/** What a `daemon.shutdown` answer says, in the three parts the panel prints. */
export interface ShutdownReport {
  stopped: number;
  /** The service that would not stop, when one would not. */
  failed: string | null;
  /** The daemon's own words for why there was no order to stop in, when there was none. */
  unordered: string | null;
}

export function shutdownReport(answer: DaemonShutdown): ShutdownReport {
  return {
    stopped: answer.services.reached.length,
    failed: answer.services.failed?.service ?? null,
    unordered: answer.unordered?.message ?? null,
  };
}
