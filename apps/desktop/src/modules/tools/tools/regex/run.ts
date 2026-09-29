import type { RegexRun } from "./match";
import type { RegexRequest } from "./worker";

/**
 * Runs a regex in a Worker, with a time limit.
 *
 * `(a+)+$` meeting a 30-character string is a loop with no way out. On the main thread that means
 * losing the whole app window — not a slow tool, but a window that can no longer redraw. This is
 * the only tool in the module where user input can do that, so it is the only tool that needs a
 * Worker.
 *
 * One run at a time: the Panel calls this function from a button.
 */

const TIMEOUT_MS = 1000;

let worker: Worker | null = null;

function ensure(): Worker {
  // The way Vite understands out of the box — no extra configuration needed.
  worker ??= new Worker(new URL("./worker.ts", import.meta.url), { type: "module" });
  return worker;
}

export function runInWorker(request: RegexRequest): Promise<RegexRun | "timeout"> {
  const active = ensure();
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      // The Worker will never answer: kill it and let the next run build a new one.
      active.onmessage = null;
      active.terminate();
      worker = null;
      resolve("timeout");
    }, TIMEOUT_MS);

    active.onmessage = (event: MessageEvent<RegexRun>) => {
      clearTimeout(timer);
      resolve(event.data);
    };
    active.postMessage(request);
  });
}
