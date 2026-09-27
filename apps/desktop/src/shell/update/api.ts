/** The only `invoke` calls for MixLab's updater: T187, split into download and install by T188. The Rust side is `src-tauri/src/updater/commands.rs`. */
import { Channel, invoke } from "@tauri-apps/api/core";
import type { PlacementKind } from "./view";

export type Placement =
  | { kind: Extract<PlacementKind, "development"> }
  | { kind: Extract<PlacementKind, "swap">; directory: string }
  | { kind: Extract<PlacementKind, "installer">; installer: "pkg" | "deb" | "rpm"; package: string | null }
  | { kind: Extract<PlacementKind, "elsewhere"> };

export interface FeedSummary {
  version: string;
  notes: string;
  notesUrl: string | null;
  /** Bytes to download for this kind of install; null when the release has no build for it. */
  size: number | null;
  hasBuild: boolean;
}

export interface UpdateStatus {
  current: string;
  placement: Placement;
  feed: FeedSummary | null;
  skipped: string | null;
  automatic: boolean;
  installing: boolean;
  /** The offered version when its download is on disk and proved; null when none. */
  downloaded: string | null;
  checkedAt: string | null;
  /** Why a check somebody asked for failed. Never set by the automatic check. */
  failure: string | null;
}

export interface Progress {
  received: number;
  total: number;
}

export interface HandedOver {
  path: string;
  command: string;
  opened: boolean;
}

export const updateStatus = () => invoke<UpdateStatus>("update_status");
export const updateCheck = (force: boolean) => invoke<UpdateStatus>("update_check", { force });
export const updateSetAutomatic = (on: boolean) => invoke<void>("update_set_automatic", { on });
export const updateSkip = (version: string) => invoke<void>("update_skip", { version });
export const updateVersionOnDisk = () => invoke<string | null>("update_version_on_disk");
/** Stops and restarts a running MixEngine, then relaunches the window: it only returns on failure. */
export const updateFinish = () => invoke<void>("update_finish");

function withProgress<T>(command: string, onProgress: (progress: Progress) => void): Promise<T> {
  const channel = new Channel<Progress>();
  channel.onmessage = onProgress;
  return invoke<T>(command, { onProgress: channel });
}

/** Download and prove the offered release; rejects with `error.updateCancelled` when stopped. */
export const updateDownload = (onProgress: (progress: Progress) => void) =>
  withProgress<void>("update_download", onProgress);
/** Stops a running download and keeps what arrived, so the next one resumes. */
export const updateCancelDownload = () => invoke<void>("update_cancel_download");
/** Windows: swap in the downloaded release and relaunch. Success ends this process, so it only returns on failure. */
export const updateInstall = () => invoke<void>("update_install");
/** macOS and Linux: open the downloaded installer. */
export const updateOpenInstaller = () => invoke<HandedOver>("update_open_installer");
/** How many services an install would restart; null when MixEngine is not running. */
export const updateRestarts = () => invoke<number | null>("update_restarts");
