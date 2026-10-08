import { useSyncExternalStore } from "react";
import type { PackageRelease } from "@mixengine/api";
import * as api from "./api";
import { subscribeDaemonWatch } from "./daemonWatch";
import { applyJob, type JobRow } from "./daemonState";
import { jobFinished } from "./runtimeState";
import { DEVKIT_PACKAGE } from "./screens/Packages/devkit";

/**
 * The devkit's install job, while one runs — one for the whole window (T206a).
 *
 * The Apply dialog starts it beside an apply, and the steps panel that follows would otherwise not
 * know: it would offer *Install devkit* again over a job already downloading. So every place that
 * installs it goes through here, and every place that shows it reads `useDevkitInstall`, which
 * carries the job's percent and message: 430 MB to download and unpack is minutes, and a button
 * that only says *Installing* for minutes reads as one that has hung.
 */
let running: JobRow | null = null;
const listeners = new Set<() => void>();
let unwatch: (() => void) | null = null;

function settle(next: JobRow | null) {
  running = next;
  for (const listener of listeners) listener();
}

/** Starts the install, or answers with the one already under way. Resolves to its job id. */
export async function installDevkit(release: PackageRelease): Promise<number> {
  if (running !== null) return running.id;
  const job = await api.packageInstall({ package: DEVKIT_PACKAGE, version: release.version });
  settle({ id: job.id, kind: job.kind, percent: job.percent, message: job.message });
  unwatch ??= subscribeDaemonWatch((raw) => {
    if (running === null) return;
    const finished = jobFinished(raw);
    if (finished !== null && finished.id === running.id) {
      unwatch?.();
      unwatch = null;
      settle(null);
      return;
    }
    const next = applyJob([running], raw).find((row) => row.id === running?.id) ?? null;
    if (next !== null && (next.percent !== running.percent || next.message !== running.message)) {
      settle(next);
    }
  });
  return job.id;
}

/** The running install — its job id, percent and message — or `null`, kept current. */
export function useDevkitInstall(): JobRow | null {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => running,
  );
}
