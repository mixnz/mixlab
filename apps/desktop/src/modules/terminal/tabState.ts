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
  | {
      kind: "local";
      /** `""` is the machine's default shell: a tab handed over by another module cannot know
       *  this machine's shell names — T205. */
      shellName: string;
      cwd: string | null;
      targetId?: string;
      /** Variables and a `PATH` prefix for the shell — T205, D9. Written back, like `cwd`. */
      env?: Record<string, string>;
      pathPrepend?: string[];
      /** Lines typed once when the tab opens, then dropped: `tabStateFor` never writes them back,
       *  so the next launch reopens a plain shell — T205, D8. */
      run?: string[];
      /** Whether the last of `run` gets Enter. Absent is `true`. */
      press?: boolean;
    };

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
    if (typeof state.shellName !== "string") return null;
    const targetId = storedTargetId(state) ?? undefined;
    // An absent `cwd` and a null `cwd` are the same thing: open the shell in its default directory.
    let cwd: string | null;
    if (state.cwd === undefined || state.cwd === null) cwd = null;
    else if (typeof state.cwd === "string") cwd = state.cwd;
    else return null;

    const env = stringMap(state.env);
    const pathPrepend = stringList(state.pathPrepend);
    const run = stringList(state.run);
    return {
      kind: "local",
      shellName: state.shellName,
      cwd,
      targetId,
      ...(env === undefined ? {} : { env }),
      ...(pathPrepend === undefined ? {} : { pathPrepend }),
      ...(run === undefined ? {} : { run }),
      ...(typeof state.press === "boolean" ? { press: state.press } : {}),
    };
  }

  return null;
}

/** An object whose every value is a string, or `undefined`. */
function stringMap(value: unknown): Record<string, string> | undefined {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return undefined;
  const entries = Object.entries(value as Record<string, unknown>);
  if (!entries.every(([, item]) => typeof item === "string")) return undefined;
  return Object.fromEntries(entries) as Record<string, string>;
}

/** An array of strings, or `undefined`. */
function stringList(value: unknown): string[] | undefined {
  if (!Array.isArray(value) || !value.every((item) => typeof item === "string")) return undefined;
  return value as string[];
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
    // Never `run` or `press`: what was typed once is not typed again on the next launch.
    return {
      kind: "local",
      shellName: choice.shell.name,
      cwd: choice.cwd,
      targetId: choice.targetId ?? undefined,
      ...(choice.env ? { env: choice.env } : {}),
      ...(choice.pathPrepend ? { pathPrepend: choice.pathPrepend } : {}),
    };
  }
  return choice.targetId === null ? undefined : { kind: "ssh", targetId: choice.targetId };
}
