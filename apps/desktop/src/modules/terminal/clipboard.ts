import { terminalClipboardText } from "./api";
import type { AppError } from "../../core/errors";

/**
 * Fetches the text on the clipboard.
 *
 * Here rather than in `core/clipboard.ts` next to `copyText`, and the boundary is the reason:
 * `core/` does not call Tauri, while the only usable way to read the clipboard has to go through
 * Rust.
 *
 * Because `navigator.clipboard.readText()` is unusable: WebView2 treats *reading* the clipboard as
 * a permission to request, so the first Paste from the menu raises a system strip in the middle of
 * the window asking "this page wants to see what you copied" — the app cannot style it, and the
 * answer is remembered per origin so it shows exactly once, just enough that nobody sees it again
 * to fix it. Writing is not asked about, so `copyText` stays in `core/` with its `execCommand`
 * fallback.
 *
 * Only the right-click menu goes through here. `Ctrl+V` does not: `shellKeeps` lets the key through
 * to the webview and xterm listens to its own `paste` event — a paste key needs nobody's
 * permission.
 */
export async function readText(): Promise<string> {
  try {
    return await terminalClipboardText();
  } catch (e) {
    const error: AppError = { code: "error.terminalClipboardRead", params: { message: String(e) } };
    throw error;
  }
}
