import { runRegex, type RegexRun } from "./match";

export interface RegexRequest {
  pattern: string;
  flags: string;
  subject: string;
  replacement: string;
}

/**
 * Exactly the two things this file needs from the worker's scope.
 *
 * `self` is declared as `Window` by the `dom` lib, while `DedicatedWorkerGlobalScope` belongs to
 * the `webworker` lib — and turning that lib on clashes with `dom`'s declarations across the whole
 * project. Declaring just the part used is far cheaper than that.
 */
interface WorkerScope {
  onmessage: ((event: MessageEvent<RegexRequest>) => void) | null;
  postMessage: (message: RegexRun) => void;
}

const scope = self as unknown as WorkerScope;

scope.onmessage = (event: MessageEvent<RegexRequest>) => {
  const { pattern, flags, subject, replacement } = event.data;
  scope.postMessage(runRegex(pattern, flags, subject, replacement));
};
