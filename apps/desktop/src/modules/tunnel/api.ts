import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { TunnelInfo } from "./tunnelState";

/** The `cloudflared` a tunnel would run, and what downloading one would fetch — `BinaryStatus`. */
export interface BinaryStatus {
  found: { path: string; origin: "settings" | "path" | "downloaded" } | null;
  download: { version: string; size: number } | null;
}

export function tunnelBinary(configured: string): Promise<BinaryStatus> {
  return invoke<BinaryStatus>("tunnel_binary", { configured });
}

/** Resolves with the downloaded program's path; progress arrives on {@link onDownload}. */
export function tunnelDownload(): Promise<string> {
  return invoke<string>("tunnel_download");
}

export function tunnelStart(target: string, configured: string): Promise<TunnelInfo> {
  return invoke<TunnelInfo>("tunnel_start", { target, configured });
}

export function tunnelStop(id: number): Promise<void> {
  return invoke("tunnel_stop", { id });
}

export function tunnelList(): Promise<TunnelInfo[]> {
  return invoke<TunnelInfo[]>("tunnel_list");
}

/** The list changed — a tunnel opened, failed, started or stopped. Read `tunnelList` again. */
export function onChanged(handler: () => void): Promise<UnlistenFn> {
  return listen("tunnel://changed", handler);
}

/** Bytes of cloudflared fetched so far, and how many in all (`0` when the server never said). */
export function onDownload(handler: (progress: { done: number; total: number }) => void): Promise<UnlistenFn> {
  return listen<{ done: number; total: number }>("tunnel://download", (event) => handler(event.payload));
}
