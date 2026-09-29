/**
 * What the row under an installed version shows — T193, drawn by `components/UpdateRow`.
 *
 * A running job wins over the button: the row is how the person follows the update they started,
 * and it keeps saying so until the list is read again after the job ends.
 */
import type { JobRow } from "./daemonState";

export type UpdateRowState<U> =
  | { kind: "none" }
  | { kind: "offer"; update: U }
  | { kind: "running"; percent: number; message: string };

export function updateRowState<U>(update: U | undefined, job: JobRow | undefined): UpdateRowState<U> {
  if (job) return { kind: "running", percent: job.percent, message: job.message };
  if (update !== undefined) return { kind: "offer", update };
  return { kind: "none" };
}
