import { useEffect, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { onTransferProgress, type TransferProgress } from "../api";

/** What the dump/restore tools are doing, if anything — shown over the whole workspace — and how far
 *  the dump or restore behind that overlay has got. Dropping a database has nothing to report, so
 *  `progress` stays null until the first reading arrives. */
export interface Transfer {
  label: string;
  progress: TransferProgress | null;
}

/** `DatabaseActions`' `onBusyChange`: a label puts the overlay up, `""` takes it and its reading
 *  down together. */
export function busyChanged(current: Transfer | null, label: string): Transfer | null {
  return label === "" ? null : { label, progress: current?.progress ?? null };
}

/** A reading, for the transfer that is up; once it is over there is nowhere to show one. */
export function progressHeard(current: Transfer | null, progress: TransferProgress): Transfer | null {
  return current && { ...current, progress };
}

/**
 * The transfer overlay of a SQL or Mongo workspace, and its `onBusyChange`.
 *
 * The listener is registered only while a transfer runs, which is the only time there is anywhere to
 * show a reading: what it is listening for happens a few times in a session and reports four times a
 * second while it does.
 */
export function useTransfer(connectionId: string): {
  transfer: Transfer | null;
  onBusyChange: (label: string) => void;
} {
  const [transfer, setTransfer] = useState<Transfer | null>(null);
  const transferring = transfer !== null;

  useEffect(() => {
    if (!transferring) return;
    let stop: UnlistenFn | null = null;
    let cancelled = false;
    void onTransferProgress(connectionId, (progress) =>
      setTransfer((current) => progressHeard(current, progress)),
    ).then((unlisten) => {
      // The transfer ended before the listener was in place; there is nothing left to hear.
      if (cancelled) void unlisten();
      else stop = unlisten;
    });
    return () => {
      cancelled = true;
      stop?.();
    };
  }, [transferring, connectionId]);

  return { transfer, onBusyChange: (label) => setTransfer((current) => busyChanged(current, label)) };
}
