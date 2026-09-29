import * as api from "./api";

/**
 * The MixEngine event channel shared by the whole app.
 *
 * `api.watch()`/`api.unwatch()` are a pair of invokes that are global on the daemon side — not a
 * channel per screen. Each screen (Dashboard, Sites, Packages/Languages, Packages/PackageList) used
 * to call `watch()` on mount and `unwatch()` on unmount itself, relying entirely on only one of
 * them ever being mounted at a time. Keeping several screens mounted at once (so job progress is
 * not lost when switching tabs) breaks that assumption: the screen mounted later would take the
 * channel away from the one mounted earlier.
 *
 * The approach of `db/tools.ts` with `tools://progress`: open the channel exactly once for the
 * app's lifetime and never close it — each side only adds/removes its own callback in a local set,
 * without touching the daemon.
 */
/** What `src-tauri/src/modules/mixengine/events.rs` sends when the stream ends. */
const DISCONNECTED = '{"type":"mixdb_disconnected"}';

const listeners = new Set<(raw: string) => void>();
let watching: Promise<void> | null = null;

function ensureWatching(): void {
  watching ??= api.watch((raw) => {
    // A stream that ended is not open any more: the next `ensureDaemonWatch` reopens it (T168 —
    // the tray panel outlives many daemons, and without this it would never hear from the next).
    if (raw === DISCONNECTED) watching = null;
    for (const listener of listeners) listener(raw);
  }).catch(() => {
    // Let the next subscribe try again — a watch that failed while opening the channel should not
    // lock up for good. There is no particular screen to report this error to any more (the
    // channel is shared), so just swallow it and retry.
    watching = null;
  });
}

/** Subscribes to every raw message from the MixEngine channel. Call the returned function to
 *  unsubscribe — that does not close the channel; it only removes this callback from the list. */
export function subscribeDaemonWatch(listener: (raw: string) => void): () => void {
  listeners.add(listener);
  ensureWatching();
  return () => {
    listeners.delete(listener);
  };
}

/** Opens the channel again if it is not open — after the daemon it was open to has gone and another
 *  has started. Harmless when it is open. */
export function ensureDaemonWatch(): void {
  ensureWatching();
}
