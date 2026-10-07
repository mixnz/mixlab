import type { ServiceState, StoppedBy } from "@mixengine/api";
import type { ServiceSummary } from "@mixengine/api";

/**
 * Reduces the event stream to the table's state.
 *
 * Pure and calls nothing — which is why it lives here and not inside a component. The rule **"state
 * is announced, never inferred"** is worth testing, and a `useEffect` cannot be tested.
 */

/** One row of the service table. Only what the table draws. */
export interface ServiceRow {
  id: string;
  state: ServiceState | null;
  port: number | null;
  /** Whether it starts along with the daemon — T112. Only a **setting**, not a prediction: it
   *  states the column in the database, not that this service will run after the next login (that
   *  also depends on whether the port is free and the program is still there). A
   *  `service_state_changed` event does not change it, so `applyEvent` keeps the old value and only
   *  rereading `service.list` can change it. */
  autostart: boolean;
  /** Who stopped it, when it is stopped — T167g. `daemon` means MixEngine stopped it itself (for
   *  being idle), and the next request will start it again; `null` while running or when an old
   *  daemon does not send it. */
  stoppedBy: StoppedBy | null;
  /** The version of the program the service is running — T183. `null` when the daemon does not
   *  send it (an old daemon, an extension's service…), and then nothing is drawn. */
  version: string | null;
  /** Why it failed, while it is failed — T200b, D6. The pill's tooltip; `null` otherwise, and from
   *  a daemon that does not send it. An event moving the state does not carry it, so only rereading
   *  `service.list` fills it in. */
  lastFailure: string | null;
}

/** A `service.list` answer, as rows. */
export function rowsFrom(list: ServiceSummary[]): ServiceRow[] {
  return list.map((service) => ({
    id: service.id,
    state: service.state ?? null,
    port: service.port ?? null,
    autostart: service.autostart,
    stoppedBy: service.stopped_by ?? null,
    version: service.version ?? null,
    lastFailure: service.state === "failed" ? (service.last_failure?.detail ?? null) : null,
  }));
}

/**
 * Whether this message means "do not trust what we have, read again".
 *
 * Split from [`applyEvent`] because the answer depends only on the message, not on the table — and
 * because calling it **outside** a `setState` updater is the only correct place: React calls
 * updaters twice under StrictMode, so a side effect placed inside one would run twice per event.
 */
export function needsResync(raw: string): boolean {
  try {
    const { type } = JSON.parse(raw) as { type?: unknown };
    return type === "resync" || type === "mixlab_disconnected";
  } catch {
    return false;
  }
}

/** Whether it is a `job_finished` — for any job. The Dashboard rereads `daemon.status` when it sees
 *  one, because a finished `elevation.grant` (from the dialog, from the CLI, or from another MixLab
 *  window) changes the waiting count without the daemon emitting any event of its own for that
 *  (`elevation_required` only fires when the queue *grows*). */
/** The event stream ended — the daemon stopped, or was stopped from somewhere else (T168: the tray). */
export function isDisconnected(raw: string): boolean {
  try {
    const { type } = JSON.parse(raw) as { type?: unknown };
    return type === "mixlab_disconnected";
  } catch {
    return false;
  }
}

export function isJobFinished(raw: string): boolean {
  try {
    const { type } = JSON.parse(raw) as { type?: unknown };
    return type === "job_finished";
  } catch {
    return false;
  }
}

/**
 * The jobs a Cancel was pressed for, after a message: a `job_finished` takes its id out.
 *
 * **Only `job_finished` does.** `job.cancel` is cooperative and its answer may still say `running`;
 * a job that never looks at its token runs to its end, and its button stays busy until then,
 * because that is what is happening. Returns the same set when nothing changed, so React skips
 * the render.
 */
export function forgetFinished(cancelling: ReadonlySet<number>, raw: string): ReadonlySet<number> {
  let event: { type?: unknown; job?: unknown };
  try {
    event = JSON.parse(raw) as { type?: unknown; job?: unknown };
  } catch {
    return cancelling;
  }
  if (event.type !== "job_finished" || typeof event.job !== "number") return cancelling;
  if (!cancelling.has(event.job)) return cancelling;
  const next = new Set(cancelling);
  next.delete(event.job);
  return next;
}

/**
 * Whether this message changes a row of the service table.
 *
 * Split from [`applyEvent`] because the answer depends only on the message — and because the only
 * place it can be asked is **outside** the `setRows` updater, for the reason [`needsResync`] gives:
 * React calls updaters twice under StrictMode.
 *
 * `Dashboard` asks this to know whether a message races a `service.list` on its way back — see
 * `readOrder.ts`. Only `service_state_changed` counts: job progress fires continuously throughout a
 * runtime install, and treating it as a reason to reread turns one long job into a barrage of RPCs.
 */
export function movesARow(raw: string): boolean {
  try {
    const { type } = JSON.parse(raw) as { type?: unknown };
    return type === "service_state_changed";
  } catch {
    return false;
  }
}

/**
 * The table after a message.
 *
 * `resync` is `true` when what just arrived means "do not trust what we have, read again": the bus
 * on the other side overflowed, or the connection dropped. Events are best-effort and **never the
 * only way to know the state**.
 */
export function applyEvent(
  rows: ServiceRow[],
  raw: string,
): { rows: ServiceRow[]; resync: boolean } {
  let event: { type?: unknown; service?: unknown; to?: unknown; reason?: unknown };
  try {
    event = JSON.parse(raw) as typeof event;
  } catch {
    return { rows, resync: false };
  }

  switch (event.type) {
    case "resync":
    case "mixlab_disconnected":
      return { rows, resync: true };

    case "service_state_changed": {
      /* `service`, **not** `id`. The Rust sketch in MixEngine's `daemon-and-ipc.md` writes
         `ServiceStateChanged { id, .. }`, but what the daemon actually sends is
         `ServiceTransition`, and it calls that field `service`. Misread the name and every event
         falls silent and the table never changes — the contract generated in `bindings/` (the
         `@mixengine/api` alias) is the truth; the architecture document is not. */
      const id = typeof event.service === "string" ? event.service : null;
      const to = typeof event.to === "string" ? (event.to as ServiceState) : null;
      if (id === null || to === null) return { rows, resync: false };
      // Build no row for an unknown service: `service.list` is where a row is born, and it knows
      // things this event does not carry.
      const stoppedBy = to === "stopped" ? stoppedByReason(event.reason) : null;
      return {
        rows: rows.map((row) => (row.id === id ? { ...row, state: to, stoppedBy } : row)),
        resync: false,
      };
    }

    default:
      // A variant from a later version. Ignoring it is the contract, not an oversight: events are
      // internally tagged precisely so this can happen.
      return { rows, resync: false };
  }
}

/**
 * Who stopped a service, read from the `reason` of the event moving it to `stopped` — the same rule
 * as MixEngine's `StoppedBy::of`: `requested` and `credential_reset` are a person, every other
 * reason is the machine. An unreadable `reason` is not guessed at: `null`, and the row shows as it
 * did before T167.
 */
export function stoppedByReason(reason: unknown): StoppedBy | null {
  if (typeof reason !== "object" || reason === null) return null;
  const kind = (reason as { kind?: unknown }).kind;
  if (typeof kind !== "string") return null;
  return kind === "requested" || kind === "credential_reset" ? "person" : "daemon";
}

/** A long-running operation in progress. `id` is the rowid of MixEngine's `jobs` row. */
export interface JobRow {
  id: number;
  kind: string;
  percent: number;
  message: string;
}

/**
 * The running jobs, after a message.
 *
 * `job_progress` and `job_finished` carry the value **as written down**, not a second description
 * of it — so a job that ends without surviving its transaction is never reported. Progress is the
 * only thing on the stream allowed to repeat.
 */
export function applyJob(jobs: JobRow[], raw: string): JobRow[] {
  let event: {
    type?: unknown;
    job?: unknown;
    kind?: unknown;
    percent?: unknown;
    message?: unknown;
  };
  try {
    event = JSON.parse(raw) as typeof event;
  } catch {
    return jobs;
  }

  const id = typeof event.job === "number" ? event.job : null;
  if (id === null) return jobs;

  if (event.type === "job_finished") return jobs.filter((job) => job.id !== id);
  if (event.type !== "job_progress") return jobs;

  const row: JobRow = {
    id,
    kind: typeof event.kind === "string" ? event.kind : "",
    percent: typeof event.percent === "number" ? event.percent : 0,
    message: typeof event.message === "string" ? event.message : "",
  };
  const at = jobs.findIndex((job) => job.id === id);
  if (at === -1) return [...jobs, row];
  // `kind` is only on the first message; do not let a later message erase it.
  return jobs.map((job, i) => (i === at ? { ...row, kind: row.kind || job.kind } : job));
}
