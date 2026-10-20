import { useEffect, useState } from "react";
import { startSync, SYNC_NOW_EVENT } from "../sync";
import { syncClosingHere, syncStatus } from "../sync/api";

export interface SyncClosing {
  /** The signed-in server and its announced end, when there is one (D4b). */
  closing: { server: string; on: number } | null;
  /** Dismissed for this run; the next launch asks again. */
  dismissed: boolean;
  dismiss: () => void;
}

/** Sync, started with the main window, and the signed-in server's closing date. */
export function useSyncClosing(): SyncClosing {
  // The main window syncs; the tray panel, which never mounts a workspace, does not.
  const [closing, setClosing] = useState<{ server: string; on: number } | null>(null);
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => startSync(), []);

  // The signed-in server's closing date, read at launch and again after a sign-in or a move —
  // `requestSync` is what both of those fire.
  useEffect(() => {
    const read = () => {
      void Promise.all([syncStatus(), syncClosingHere()])
        .then(([status, on]) => setClosing(status.server && on !== null ? { server: status.server, on } : null))
        .catch(() => setClosing(null));
    };
    read();
    window.addEventListener(SYNC_NOW_EVENT, read);
    return () => window.removeEventListener(SYNC_NOW_EVENT, read);
  }, []);

  return { closing, dismissed, dismiss: () => setDismissed(true) };
}
