import type { TerminalChoice } from "./types";

/**
 * What a terminal tab remembers between two app launches: which saved target, or which local
 * shell, it is on.
 *
 * The `ssh` branch has only a uuid — the host, port, user name and secrets live in
 * `terminal-hosts.json` plus the OS credential store, and none of them are copied here. The
 * `local` branch keeps the shell's `name` (`powershell`, `wsl:Ubuntu`) rather than its path,
 * because `name` is a stable identifier while the path changes from machine to machine. `cwd` is
 * the only thing in this file that is not an id: it is a path on the user's machine, not a secret.
 * The line is drawn there — §4 of `docs/specs/2026-08-23-tab-session-context-design.md`.
 *
 * The `local` branch's `targetId` is **additional** to `shellName`/`cwd`, not a replacement: it is
 * only there to look up the startup command on the live entry, so a deleted entry still lets the
 * tab reopen its shell just as before that field existed. And the startup command is never copied
 * here — editing it once makes every tab pointing at that entry follow.
 */
export type TerminalTabState =
  | { kind: "ssh"; targetId: string }
  | { kind: "local"; shellName: string; cwd: string | null; targetId?: string };

/** The target's id in a saved state, whatever name it was written under. Before this version it
 *  was called `hostId`, when the list only held servers — a tab open during an upgrade does not
 *  lose where it was. */
function storedTargetId(state: Record<string, unknown>): string | null {
  const id = typeof state.targetId === "string" ? state.targetId : state.hostId;
  return typeof id === "string" && id !== "" ? id : null;
}

/**
 * The saved value, if it is one, otherwise `null`.
 *
 * Validation lives here — the shell deliberately passes the state slot through without looking,
 * because only this module knows its shape. Everything arriving here is a string some older
 * version of the app wrote, so nothing is trusted.
 */
export function parseTerminalTabState(value: unknown): TerminalTabState | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const state = value as Record<string, unknown>;

  if (state.kind === "ssh") {
    const targetId = storedTargetId(state);
    if (targetId === null) return null;
    return { kind: "ssh", targetId };
  }

  if (state.kind === "local") {
    if (typeof state.shellName !== "string" || state.shellName === "") return null;
    const targetId = storedTargetId(state) ?? undefined;
    // An absent `cwd` and a null `cwd` are the same thing: open the shell in its default directory.
    if (state.cwd === undefined || state.cwd === null) {
      return { kind: "local", shellName: state.shellName, cwd: null, targetId };
    }
    if (typeof state.cwd !== "string") return null;
    return { kind: "local", shellName: state.shellName, cwd: state.cwd, targetId };
  }

  return null;
}

/**
 * The part of a choice worth remembering, or `undefined` when nothing can be pointed at.
 *
 * A hand-typed SSH has no `targetId`, and the only thing that could reopen it is the password —
 * which never goes into `localStorage`. So it remembers nothing, and the next time that tab opens
 * it is the form. That is the line held correctly, not a gap. A shell on this machine has no
 * secrets, so it can be remembered even when nobody has saved it as a row in the list.
 */
export function tabStateFor(choice: TerminalChoice): TerminalTabState | undefined {
  if (choice.kind === "local") {
    return {
      kind: "local",
      shellName: choice.shell.name,
      cwd: choice.cwd,
      targetId: choice.targetId ?? undefined,
    };
  }
  return choice.targetId === null ? undefined : { kind: "ssh", targetId: choice.targetId };
}
