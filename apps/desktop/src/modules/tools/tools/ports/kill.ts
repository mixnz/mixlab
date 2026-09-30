/**
 * The command to kill a process, printed for copying.
 *
 * **The tool runs none of the commands in here.** That is the safety boundary of the whole module:
 * a tool that prints `kill -9` is a tool the user reads before running, while a "Kill" button is a
 * misclick waiting to happen.
 */

export type KillOs = "macos" | "linux" | "windows";

export function killByPid(os: KillOs, pid: number): string {
  return os === "windows" ? `taskkill /PID ${pid} /F` : `kill -9 ${pid}`;
}

export function killByPort(os: KillOs, port: number): string {
  if (os === "windows") {
    // `%a` rather than `%%a`: this string is pasted straight at the cmd prompt, not into a .bat
    // file.
    return `for /f "tokens=5" %a in ('netstat -ano ^| findstr :${port}') do taskkill /PID %a /F`;
  }
  return `lsof -ti:${port} | xargs kill -9`;
}

/**
 * Which OS the machine running MixLab is — only to set the picker's **default value**.
 *
 * The picker can still be changed by hand, and that is deliberate: people on Windows often need
 * the kill command for a Linux server open in the Terminal tab next door.
 */
export function hostOs(): KillOs {
  const platform = navigator.platform.toLowerCase();
  if (platform.startsWith("win")) return "windows";
  if (platform.startsWith("mac")) return "macos";
  return "linux";
}
