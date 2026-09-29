import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AppError } from "../../core/errors";

/**
 * What is happening to a connection's SSH tunnel.
 *
 * Only connections going through a tunnel have this event — a direct connection emits nothing, so
 * no separate flag is needed to know whether to draw the banner.
 */
export interface TunnelState {
  /** Which tab this connection belongs to: two tabs may drop at the same moment and each one only
   *  listens to its own. */
  id: string;
  state: "reconnecting" | "reconnected" | "failed";
  /** Only present with `failed`: why it could not be reopened — a wrong key, an unreachable
   *  host. */
  error?: AppError;
}

/** Listens to all tunnel news for `id` until the returned function is called. */
export function onTunnelState(
  id: string,
  onState: (state: TunnelState) => void
): Promise<UnlistenFn> {
  return listen<TunnelState>("tunnel://state", ({ payload }) => {
    if (payload.id === id) onState(payload);
  });
}

/** Reopens the SSH session right away, instead of waiting out the backoff of the Rust-side
 *  watcher. */
export function tunnelReconnect(id: string): Promise<void> {
  return invoke("tunnel_reconnect", { id });
}
