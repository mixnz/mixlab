import { logError } from "../../core/log";
import { SYNCABLE } from "../registry";
import { tauriSync } from "./api";
import { readEnabled } from "./enabled";
import { syncActivity } from "./activity";
import { startSyncLoop } from "./loop";
import { noteReplaced } from "./replaced";

/** Fired on `window` to ask for a run now: after signing in, or turning a row on. */
export const SYNC_NOW_EVENT = "mixlab:sync-now";

export function requestSync(): void {
  window.dispatchEvent(new Event(SYNC_NOW_EVENT));
}

/**
 * Sync for the main window: every collection that is on, at D8's moments — and whenever
 * {@link requestSync} asks. With every row off, which is how a machine starts (D5), it runs and
 * asks nothing of anybody. Returns the stop.
 */
export function startSync(): () => void {
  return startSyncLoop({
    backend: tauriSync,
    collections: () => {
      const on = readEnabled(localStorage);
      return SYNCABLE.filter((collection) => on.has(collection.id));
    },
    onFocus: (listener) => {
      window.addEventListener("focus", listener);
      return () => window.removeEventListener("focus", listener);
    },
    onRequest: (listener) => {
      window.addEventListener(SYNC_NOW_EVENT, listener);
      return () => window.removeEventListener(SYNC_NOW_EVENT, listener);
    },
    onReplaced: noteReplaced,
    onDownloading: syncActivity.downloading,
    onUploading: syncActivity.uploading,
    onRunEnd: syncActivity.runEnded,
    // An `AppError` is a plain object, which `String()` would print as `[object Object]`.
    onError: (collection, error) =>
      void logError("sync", error instanceof Error ? error : JSON.stringify(error), collection),
  });
}
