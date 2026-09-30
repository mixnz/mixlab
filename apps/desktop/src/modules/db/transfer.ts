import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/**
 * How far a running dump or restore has got, as the Rust side works it out.
 *
 * The two are not measured the same way and the numbers should not be read as if they were. A
 * restore's is a count: the file is of a known size and every byte of it is fed to the client by
 * MixLab. A dump's is an estimate, built from the tables mysqldump says it has reached and the size
 * of the file it is writing — the tool keeps no count of its own to ask for.
 */
export interface TransferProgress {
  /** The connection this is about: two tabs can be transferring at once, and each overlay wants
   *  its own. */
  id: string;
  /** 0 to 99, or null when there is no honest percentage to show. Never 100: what says a transfer
   *  is finished is the call that started it returning. */
  percent: number | null;
  /** What is being written out at this moment — a table of a MySQL database, a collection of a
   *  MongoDB one. A restore has none: what it is replaying is a stream, not a list of anything. */
  part: string | null;
  /** Which of them that is, counted from one: the one in hand, not the ones behind it. Zero when
   *  there is none to be on, which is a restore and the moment before a dump reaches its first. */
  atPart: number;
  parts: number;
}

/** Reports every reading of `id`'s transfer until the returned function is called. */
export function onTransferProgress(
  id: string,
  onProgress: (progress: TransferProgress) => void
): Promise<UnlistenFn> {
  return listen<TransferProgress>("transfer://progress", ({ payload }) => {
    if (payload.id === id) onProgress(payload);
  });
}

/**
 * Stops the tool a connection is running, if it is running one.
 *
 * The tool is killed rather than asked to stop: `mysqldump` and the rest have no protocol for
 * finishing early. So a cancelled dump leaves a partial file behind — the command reports being
 * stopped rather than reporting success, which is what says the file is not a dump.
 *
 * Never rejects for a connection that is transferring nothing, which is what lets a tab closing
 * call it without first working out whether it needs to.
 */
export function cancelTransfer(id: string): Promise<void> {
  return invoke("cancel_db_transfer", { id });
}
