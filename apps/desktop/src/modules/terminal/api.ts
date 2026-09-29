import { Channel, invoke } from "@tauri-apps/api/core";
import type { LocalShell, TerminalSize, TerminalTarget } from "./types";

/**
 * The only place in this module that talks to native code.
 *
 * Every command rejects with an `AppError` — `{ code, params }` — and the caller passes it through
 * `errorMessage(t, e)` rather than showing it directly.
 */

/** Which shells this machine can open; the order is the suggested order, the first is the
 *  default. */
export function localShells(): Promise<LocalShell[]> {
  return invoke<LocalShell[]>("terminal_local_shells");
}

/** The session ended: the shell exited normally, or the line broke. In round 1 `message` is always
 *  null. */
export interface SessionExit {
  type: "exit";
  code: number | null;
  message: string | null;
}

/** One channel carrying two things: an `ArrayBuffer` is bytes the far end printed, an object means
 *  the session ended. The same channel, so the order is real — `exit` cannot arrive before the
 *  last byte. */
export type SessionMessage = ArrayBuffer | SessionExit;

export function openSession(
  id: string,
  target: TerminalTarget,
  size: TerminalSize,
  onEvent: (message: SessionMessage) => void,
): Promise<void> {
  const channel = new Channel<SessionMessage>();
  channel.onmessage = onEvent;
  return invoke("terminal_open", { id, target, size, onEvent: channel });
}

export function writeSession(id: string, data: string): Promise<void> {
  return invoke("terminal_write", { id, data });
}

export function resizeSession(id: string, cols: number, rows: number): Promise<void> {
  return invoke("terminal_resize", { id, cols, rows });
}

export function closeSession(id: string): Promise<void> {
  return invoke("terminal_close", { id });
}

/** The text on the system clipboard. Rust reads it for us —
 *  `src-tauri/src/modules/terminal/commands.rs`. */
export function terminalClipboardText(): Promise<string> {
  return invoke<string>("terminal_clipboard_text");
}
