/**
 * The one setting this module has: a `cloudflared` the person chose, by path — T203, D2.
 *
 * In `localStorage` because it is about this machine and nothing else: it is not synced, and it is
 * not in a tab's session slot, which holds ids only. Empty means *use one on PATH, or the one MixLab
 * downloads*.
 */
const KEY = "mixlab-tunnel-cloudflared";

export function readCloudflaredPath(): string {
  try {
    return localStorage.getItem(KEY) ?? "";
  } catch {
    return "";
  }
}

export function writeCloudflaredPath(path: string): void {
  try {
    if (path.trim() === "") localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, path.trim());
  } catch {
    // A full or blocked storage loses the setting, and the module falls back to PATH.
  }
}
