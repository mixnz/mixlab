import { Channel, invoke } from "@tauri-apps/api/core";
import type { SyncChanges, SyncItem } from "../../core/syncCollection";

/**
 * The account and the loop's commands (`src-tauri/src/sync/commands.rs`), typed. `MK` never comes
 * this way; the recovery key does, once, from `syncRegister`.
 */

export interface SyncStatus {
  signedIn: boolean;
  server: string | null;
  email: string | null;
  deviceId: string | null;
  /** The day the server said it closes, in seconds — advisory, and `null` until this run has synced. */
  closingOn: number | null;
  /** The address a code was sent to, while sign-up waits for it. */
  verifying: string | null;
}

export interface SyncDevice {
  id: string;
  name: string;
  createdAt: number;
  lastSeenAt: number;
  current: boolean;
}

/** A page for a module to write; `token` goes back once it has. */
export interface PulledPage {
  token: string;
  changes: SyncChanges;
  more: boolean;
}

/** `replaced` is this machine's edits that lost to newer ones (D4). No token, nothing to write. */
export interface PushedChanges {
  accepted: number;
  replaced: SyncChanges;
  token: string | null;
  /** The first refusal, of one entry or of a whole request (T178c, C2). What landed is already agreed. */
  error: { code: string; params?: Record<string, string> } | null;
  /** This account's collection was never pulled, so nothing was sent: sync it in full instead. */
  needsPull: boolean;
}

/** What the loop needs of the backend, so a test can hand it a fake. */
export interface SyncBackend {
  /** Stamps this machine's changes before a pull, so the pull can weigh them (D4). Sends nothing. */
  notice(collection: string, items: SyncItem[]): Promise<void>;
  /**
   * Which of these collections a full run must pull, in one request (T189). Every other one's
   * cursor moves past this machine's own writes, so it is only pushed.
   */
  heads(collections: string[]): Promise<string[]>;
  pullPage(collection: string): Promise<PulledPage>;
  /** `skipped`: what the module did not write, which sync must not agree on (T178a, L4). */
  commitPull(collection: string, token: string, skipped: string[]): Promise<void>;
  /** `onSending`: called just before anything leaves for the server — never when nothing changed. */
  push(collection: string, items: SyncItem[], onSending?: () => void): Promise<PushedChanges>;
  commitPush(collection: string, token: string, skipped: string[]): Promise<void>;
}

export const tauriSync: SyncBackend = {
  notice: (collection, items) => invoke("sync_notice", { collection, items }),
  heads: (collections) => invoke("sync_heads", { collections }),
  pullPage: (collection) => invoke("sync_pull_page", { collection }),
  commitPull: (collection, token, skipped) => invoke("sync_commit_pull", { collection, token, skipped }),
  push: (collection, items, onSending) => {
    const channel = new Channel<null>();
    if (onSending) channel.onmessage = () => onSending();
    return invoke("sync_push", { collection, items, onSending: channel });
  },
  commitPush: (collection, token, skipped) => invoke("sync_commit_push", { collection, token, skipped }),
};

export function syncStatus(): Promise<SyncStatus> {
  return invoke("sync_status");
}

/** What this machine calls itself, offered as its name in the device list. */
export function syncDeviceName(): Promise<string> {
  return invoke("sync_device_name");
}

/** Resolves to the recovery key, which is shown once and never asked for again. */
export function syncRegister(server: string, access: string | null, email: string, password: string): Promise<string> {
  return invoke("sync_register", { server, access, email, password });
}

export function syncVerify(code: string, deviceName: string): Promise<SyncStatus> {
  return invoke("sync_verify", { code, deviceName });
}

export function syncLogin(
  server: string,
  access: string | null,
  email: string,
  password: string,
  deviceName: string,
): Promise<SyncStatus> {
  return invoke("sync_login", { server, access, email, password, deviceName });
}

export function syncLogout(): Promise<void> {
  return invoke("sync_logout");
}

export function syncDevices(): Promise<SyncDevice[]> {
  return invoke("sync_devices");
}

export function syncRevokeDevice(id: string): Promise<void> {
  return invoke("sync_revoke_device", { id });
}

export function syncChangePassword(current: string, next: string): Promise<void> {
  return invoke("sync_change_password", { current, next });
}

/** Asks for the reset letter. Resolves the same whether or not the address has an account. */
export function syncResetAsk(server: string, access: string | null, email: string): Promise<void> {
  return invoke("sync_reset_ask", { server, access, email });
}

/** D6 case 2, first step: spends the code. Rust holds the ticket for {@link syncResetKeep}. */
export function syncResetOpen(server: string, access: string | null, email: string, code: string): Promise<void> {
  return invoke("sync_reset_open", { server, access, email, code });
}

/** D6 case 2: the recovery key keeps the records; signs this machine in. */
export function syncResetKeep(recoveryKey: string, password: string, deviceName: string): Promise<SyncStatus> {
  return invoke("sync_reset_keep", { recoveryKey, password, deviceName });
}

/** D6 case 3, before anything is deleted: resolves to the new recovery key, to be shown once. */
export function syncResetPrepare(
  server: string,
  access: string | null,
  email: string,
  password: string,
): Promise<string> {
  return invoke("sync_reset_prepare", { server, access, email, password });
}

/** D6 case 3: spends the code, which deletes every record, and signs in under the new key. */
export function syncResetStartOver(code: string, deviceName: string): Promise<SyncStatus> {
  return invoke("sync_reset_start_over", { code, deviceName });
}

/** Whether the account holds still for a copy to another server (D4b), and since when, in seconds. */
export interface SyncFreeze {
  state: "active" | "frozen";
  frozenAt: number | null;
}

export interface SyncMoved {
  copied: number;
}

/** Deletes the account and everything in it; resolves to how many records went. */
export function syncDeleteAccount(password: string): Promise<number> {
  return invoke("sync_delete_account", { password });
}

export function syncFreezeState(): Promise<SyncFreeze> {
  return invoke("sync_freeze_state");
}

/** Ends a freeze. Any signed-in machine may, and nothing else ever will. */
export function syncThaw(): Promise<SyncFreeze> {
  return invoke("sync_thaw");
}

/** D4b step 1: registers on the new server, which sends its own letter. */
export function syncMoveBegin(server: string, access: string | null, password: string): Promise<void> {
  return invoke("sync_move_begin", { server, access, password });
}

/** Confirms there, freezes the old account, copies and compares. Safe to run again after a failure. */
export function syncMoveConfirm(code: string, deviceName: string): Promise<SyncMoved> {
  return invoke("sync_move_confirm", { code, deviceName });
}

/** Deletes the old account or thaws it; this machine then syncs with the new server. */
export function syncMoveFinish(deleteOld: boolean): Promise<SyncStatus> {
  return invoke("sync_move_finish", { deleteOld });
}

/** Thaws the old account and forgets the move. */
export function syncMoveAbandon(): Promise<void> {
  return invoke("sync_move_abandon");
}

/** The signed-in server's closing date in seconds, or `null`. Opens the session if needed. */
export function syncClosingHere(): Promise<number | null> {
  return invoke("sync_closing_here");
}

/** Any server's closing date, asked before signing in to it. */
export function syncServerClosing(server: string, access: string | null): Promise<number | null> {
  return invoke("sync_server_closing", { server, access });
}
