import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** The three sets of tools, one per SQL-or-document engine: each carries the pair its database
 *  needs. Only two of them are ever downloadable — PostgreSQL publishes no client-only archive. */
export type ToolSuite = "mysql" | "postgres" | "mongo";

/** Where a tool was found. `custom` is a path picked in Settings, `downloaded` a copy MixLab
 *  fetched for itself, and `system` something already installed on the machine. */
export type ToolSource = "custom" | "downloaded" | "system";

export interface ToolStatus {
  /** `mysqldump`, `mysql`, `mongodump` or `mongorestore`. */
  name: string;
  suite: ToolSuite;
  /** Where it is, or null when it is nowhere to be found. */
  path: string | null;
  source: ToolSource | null;
  /** Whether MixLab can fetch this tool for itself here. The same answer for every tool of a
   *  suite — MySQL publishes a plain archive for Windows only, so on macOS and Linux its tools
   *  have to come from the machine. */
  downloadable: boolean;
}

export function toolsStatus(): Promise<ToolStatus[]> {
  return invoke<ToolStatus[]>("dumptools_status");
}

/** Whether both of a suite's tools can be found — what the dump and restore buttons ask first. */
export function toolsReady(suite: ToolSuite): Promise<boolean> {
  return invoke<boolean>("dumptools_ready", { suite });
}

/** Whether there is anywhere to download this suite from on this platform — asked before a button
 *  offers to, since a suite with no archive can only be answered with an error. */
export function toolsDownloadable(suite: ToolSuite): Promise<boolean> {
  return invoke<boolean>("dumptools_downloadable", { suite });
}

/** The stages an install goes through, in the order they happen. */
export type ToolStage = "downloading" | "verifying" | "unpacking" | "installing";

/** How far an install has got. `total` is 0 when the server never said how big the archive is,
 *  which is the difference between a bar with a percentage and one that only shows movement. */
export interface ToolProgress {
  suite: ToolSuite;
  stage: ToolStage;
  done: number;
  total: number;
}

/** Which suites are being fetched, and which have just been. */
export interface ToolInstallState {
  /** Every suite being fetched right now, against how far it has got — a suite with no progress
   *  yet is in the map with `null`, which is what tells "starting" from "not running". */
  running: ReadonlyMap<ToolSuite, ToolProgress | null>;
  /** When each suite's install finished, for the line that says it did. */
  finished: ReadonlyMap<ToolSuite, number>;
}

/*
 * An install runs for minutes behind a modal dialog, which is to say behind the whole app: the
 * user will close Settings and get on with something else. So how far it has got is kept here,
 * out of the screen showing it, and the screen reads it back on the way in. Otherwise closing the
 * dialog would lose the download from sight and leave the button ready to start a second one.
 *
 * Keyed by suite rather than held one at a time: the two suites are downloaded to staging
 * directories of their own and unpacked into directories of their own, so fetching both at once is
 * two errands that never meet, and there is no reason to make the user wait out the first.
 */
let state: ToolInstallState = { running: new Map(), finished: new Map() };
const listeners = new Set<() => void>();
let listening: Promise<UnlistenFn> | null = null;

function setState(next: ToolInstallState) {
  state = next;
  for (const listener of listeners) listener();
}

/** The state as it stands with one suite's entries changed. `undefined` removes the entry. */
function withSuite(
  running?: [ToolSuite, ToolProgress | null | undefined],
  finished?: [ToolSuite, number | undefined],
): ToolInstallState {
  const nextRunning = new Map(state.running);
  const nextFinished = new Map(state.finished);
  if (running) {
    const [suite, progress] = running;
    if (progress === undefined) nextRunning.delete(suite);
    else nextRunning.set(suite, progress);
  }
  if (finished) {
    const [suite, at] = finished;
    if (at === undefined) nextFinished.delete(suite);
    else nextFinished.set(suite, at);
  }
  return { running: nextRunning, finished: nextFinished };
}

/** Registers the progress listener, once, and resolves when it is in place. */
function startListening(): Promise<UnlistenFn> {
  listening ??= listen<ToolProgress>("tools://progress", ({ payload }) => {
    // An install that has already ended has nothing left to report; ignore anything still in
    // flight behind it rather than reviving the bar.
    if (!state.running.has(payload.suite)) return;
    setState(withSuite([payload.suite, payload]));
  });
  return listening;
}

/** How the install is going, for `useSyncExternalStore`. The same object until something changes,
 *  which is what stops it from re-rendering on every read. */
export function toolInstallState(): ToolInstallState {
  return state;
}

export function subscribeToolInstall(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Downloads a suite from its vendor. Tens of megabytes at best, so it reports its way through
 *  {@link toolInstallState} — and resolving is what says it finished. */
export async function toolsInstall(suite: ToolSuite): Promise<void> {
  // Before the download starts, so the first bytes are not counted against an unregistered
  // listener.
  await startListening();
  setState(withSuite([suite, null], [suite, undefined]));
  try {
    await invoke<void>("dumptools_install", { suite });
    setState(withSuite([suite, undefined], [suite, Date.now()]));
  } catch (e) {
    setState(withSuite([suite, undefined]));
    throw e;
  }
}

/** Deletes MixLab's own copy. Tools found on the machine itself are left alone. */
export function toolsUninstall(suite: ToolSuite): Promise<void> {
  // "Downloaded and ready to use" stops being true the moment the user asks for it to go, so the
  // line saying so goes with the click rather than waiting out the rest of its few seconds.
  if (state.finished.has(suite)) setState(withSuite(undefined, [suite, undefined]));
  return invoke<void>("dumptools_uninstall", { suite });
}

/** Points one tool at a copy chosen by hand, or forgets that choice when given null. */
export function toolsSetPath(tool: string, path: string | null): Promise<void> {
  return invoke<void>("dumptools_set_path", { tool, path });
}
