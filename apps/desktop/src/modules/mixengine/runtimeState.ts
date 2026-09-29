import type { AppError } from "../../core/errors";
import type { PoolOutcome } from "@mixengine/api";
import type { JobRow } from "./daemonState";

/** `"php@8.3.12"` — the same string serves as the React key and as the `installingJob` lookup
 *  key. */
export type VersionKey = string;

export function versionKey(kind: string, version: string): VersionKey {
  return `${kind}@${version}`;
}

/** How to draw an `ExtensionChange.pool` — three values, three different banners, none of them an
 *  error. */
export type PoolBanner = "none" | "restartRequired" | "appliesNextStart";

export function poolBanner(outcome: PoolOutcome): PoolBanner {
  switch (outcome) {
    case "reloaded":
      return "none";
    case "restart_required":
      return "restartRequired";
    case "pool_not_running":
      return "appliesNextStart";
  }
}

/** The job being followed for a row, from the `JobRow[]` `daemonState.applyJob` computed — no
 *  second job map of its own; it only looks up what is already there. */
export function jobFor(jobs: JobRow[], jobId: number | undefined): JobRow | undefined {
  return jobId === undefined ? undefined : jobs.find((job) => job.id === jobId);
}

/** A job that just ended: which job, and what it failed on. */
export interface JobFinished {
  id: number;
  /** `null` when the job completed or was cancelled on request — only `ending: "failed"` has
   *  something to tell. */
  error: AppError | null;
}

/**
 * What this `job_finished` says, if this message is a `job_finished` — otherwise `null`.
 *
 * `applyJob` has already removed that job from `JobRow[]`, but removing it alone does not take a
 * just-installed version out of the "available" table — that needs rereading
 * `installed`/`available` from the daemon. Kept apart from `applyJob` because this is a decision
 * about "should the API be called again", not the job table's state.
 *
 * **`ending` is the other half of the sentence, not a side detail.** A failed job also sends
 * `job_finished`, and its `error` is the *only* place that says why: `runtime.install` answered
 * "accepted" long ago, so there is no failing call left to catch. Reading only `job` and then
 * rereading the list leaves the user watching the progress bar vanish, the list stay the same, and
 * guessing.
 */
export function jobFinished(raw: string): JobFinished | null {
  let event: { type?: unknown; job?: unknown; ending?: unknown; error?: unknown };
  try {
    event = JSON.parse(raw) as typeof event;
  } catch {
    // Not valid JSON — reporting that is not this function's job.
    return null;
  }
  if (event.type !== "job_finished" || typeof event.job !== "number") return null;
  return {
    id: event.job,
    error: event.ending === "failed" ? refusal(event.error) : null,
  };
}

/**
 * The daemon's `Error`, as something `errorMessage` can draw.
 *
 * The same `code` and the same parameters `map_rpc_error` (`mixengine/rpc.rs`) builds for a refused
 * call, on purpose: **a failed job is not a second error vocabulary**. The same sentence the daemon
 * wrote, whether it arrives through a call's answer or through the event stream, has to show up
 * the same way.
 */
function refusal(value: unknown): AppError {
  const wire = (typeof value === "object" && value !== null ? value : {}) as {
    code?: unknown;
    message?: unknown;
    hint?: unknown;
  };
  const params: Record<string, string> = {
    code: typeof wire.code === "string" ? wire.code : "internal",
    message: typeof wire.message === "string" ? wire.message : "",
  };
  // Absent rather than empty, the same rule `map_rpc_error` sets.
  if (typeof wire.hint === "string") params.hint = wire.hint;
  return { code: "error.mixengineRefused", params };
}

/** `RuntimeSummary.installed_at`/`PackageSummary.installed_at` are epoch milliseconds
 *  (`Timestamp`), not strings — drawn the same way `UpdateSection.tsx` draws `checked_at`: in the
 *  user's own machine's time zone and format, not a fixed standard. */
export function formatInstalledAt(ms: number): string {
  return new Date(ms).toLocaleString();
}

/**
 * The installed versions of one kind, **newest first** — what a pin field suggests.
 *
 * Re-sorted rather than trusting the daemon's order: `runtime.list_installed` reads `ORDER BY kind,
 * version`, which sorts *strings*, so `8.10.0` comes before `8.9.0`. A suggestion list that gets
 * wrong which version is newer is worse than no sorting at all.
 *
 * Compared segment by segment: the leading numeric part compares as a number, the remaining tail
 * compares as a string, and **the shorter tail is the later release** — `8.5.0` after `8.5.0RC1`,
 * matching `VersionConstraint`'s rule that "a constraint that does not mention a pre-release never
 * picks a pre-release". This is an order for *display*; choosing which version really matches a
 * constraint is still the daemon's job.
 */
export function installedVersions(
  runtimes: readonly { kind: string; version: string }[],
  kind: string,
): string[] {
  return runtimes
    .filter((runtime) => runtime.kind === kind)
    .map((runtime) => runtime.version)
    .sort((left, right) => compareVersions(right, left));
}

/**
 * `rows` with each name's versions **newest first**, for the installed and available tables.
 *
 * Names keep the order the daemon gave them in — the first row of a name decides where its group
 * sits — and only the versions within one name are re-ordered, by the same comparison as
 * `installedVersions`. Two names are never interleaved.
 */
export function newestFirst<Row extends { version: string }>(
  rows: readonly Row[],
  nameOf: (row: Row) => string,
): Row[] {
  const groups = new Map<string, Row[]>();
  for (const row of rows) {
    const name = nameOf(row);
    const group = groups.get(name);
    if (group === undefined) groups.set(name, [row]);
    else group.push(row);
  }
  return [...groups.values()].flatMap((group) =>
    group.sort((left, right) => compareVersions(right.version, left.version)),
  );
}

/** Negative when `left` comes before `right`. */
function compareVersions(left: string, right: string): number {
  const ours = left.split(".");
  const theirs = right.split(".");
  for (let i = 0; i < Math.max(ours.length, theirs.length); i++) {
    // A missing segment is the shorter version — `20.11` before `20.11.1`.
    if (ours[i] === undefined) return -1;
    if (theirs[i] === undefined) return 1;
    const decided = compareSegments(ours[i], theirs[i]);
    if (decided !== 0) return decided;
  }
  return 0;
}

function compareSegments(left: string, right: string): number {
  const ourNumber = Number.parseInt(left, 10);
  const theirNumber = Number.parseInt(right, 10);
  if (ourNumber !== theirNumber) {
    // A segment not starting with a digit gives `NaN`; comparing as strings is the only thing that
    // still means anything then.
    if (Number.isNaN(ourNumber) || Number.isNaN(theirNumber)) return left < right ? -1 : 1;
    return ourNumber - theirNumber;
  }

  // Same leading number: an empty tail (`0`) is the release, a lettered tail (`0RC1`) is the one
  // before it.
  const ourTail = left.slice(String(ourNumber).length);
  const theirTail = right.slice(String(theirNumber).length);
  if (ourTail === theirTail) return 0;
  if (ourTail === "") return 1;
  if (theirTail === "") return -1;
  return ourTail < theirTail ? -1 : 1;
}
