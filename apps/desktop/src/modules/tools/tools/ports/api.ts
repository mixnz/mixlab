import { invoke } from "@tauri-apps/api/core";

/** A port being listened on. Matches `ListeningPort` in `src-tauri/src/modules/tools/ports.rs`. */
export interface ListeningPort {
  port: number;
  /** `0.0.0.0`, `127.0.0.1`, `::` — tells "open to the outside" apart from "localhost only". */
  address: string;
  pid: number;
  /** `null` when the process name could not be looked up, usually for lack of permissions. */
  process: string | null;
}

/** The only place in the module that calls `invoke`, kept apart from the Panel just as
 *  `src/modules/db/tools.ts` does. */
export function listeningPorts(): Promise<ListeningPort[]> {
  return invoke<ListeningPort[]>("tools_listening_ports");
}
