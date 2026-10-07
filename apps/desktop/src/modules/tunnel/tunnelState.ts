import type { StatusTone } from "../../components/StatusPill";

/** One tunnel as the backend reports it — `TunnelInfo` in `src-tauri/src/modules/tunnel/state.rs`. */
export interface TunnelInfo {
  id: number;
  target: string;
  url: string | null;
  state: "connecting" | "open" | "failed";
  detail: string | null;
  hint: "allowedHosts" | "nothingListening" | null;
}

/** The pill's tone: in transition while connecting, good once open, bad once failed. */
export function rowTone(info: TunnelInfo): StatusTone {
  if (info.state === "open") return "success";
  if (info.state === "failed") return "danger";
  return "warning";
}

/** A download's size, as a person reads it before agreeing to it. */
export function formatSize(bytes: number): string {
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
