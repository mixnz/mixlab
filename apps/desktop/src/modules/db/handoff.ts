import { invoke } from "@tauri-apps/api/core";
import type { ConnectionConfig } from "./types";

/**
 * A connection another program handed to MixLab — MixEngine's `mix database open` — as it is
 * taken by the tab opened for it.
 *
 * The backend read it off a `mixlab://connect?…` URL and the password off one environment
 * variable, and keeps it until this call; see
 * `docs/specs/2026-09-03-mixengine-connection-handoff-design.md`. From here it is a
 * config in the form like any typed by hand: `connect_db`, the workspace, Save if the user wants.
 */
export interface Handoff {
  config: ConnectionConfig;
  /** The tab's name and the name pre-filled for saving — the launcher's own id for the server. */
  label: string;
  /** The address to save instead of a copy of the password — see `SavedConnection.keyringRef`.
   *  `null`/absent for a `mixlab://` link that arrived without proof MixEngine started this
   *  process, even when the URL itself named one. */
  keyring_ref?: string | null;
}

/** Takes the handoff under `id`. Rejects with `error.handoffExpired` when it was already taken,
 *  or when the id is from a session an earlier run wrote — in which case the tab is simply empty. */
export function takeHandoff(id: string): Promise<Handoff> {
  return invoke("handoff_take", { id });
}
