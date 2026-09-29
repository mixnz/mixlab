import { shellLabel } from "./shells";
import type { TerminalChoice, TerminalTarget } from "./types";

/** A mark this tab should carry. `TerminalTab` turns it into a `TabBadge` because that is where
 *  `t` is. */
export type TerminalBadgeMark = { type: "local" } | { type: "ssh" } | { type: "ended" };

/** What the user chose, reduced to what Rust needs. The display label stays here. */
export function terminalTarget(choice: TerminalChoice): TerminalTarget {
  if (choice.kind === "local") {
    return { type: "local", shell: choice.shell.path, args: choice.shell.args, cwd: choice.cwd };
  }
  return { type: "ssh", ...choice.config };
}

/**
 * The tab name: the shell name, or `user@host` — not a path, because the tab bar is only a few
 * characters wide.
 *
 * `savedName` is the name of the saved target this session came from, and it beats both when
 * present. Passing it down is the caller's job: `TerminalTab` only passes it when the *tab name is
 * the target name* setting is on and `choice.targetId` resolves to an entry. So this function knows
 * nothing about that setting, and `null` is still the behaviour it has always had.
 */
export function terminalTitle(choice: TerminalChoice, savedName: string | null): string {
  // An entry not yet named has nothing to show, not a request for the tab to have no name.
  if (savedName !== null && savedName.trim() !== "") return savedName;
  return choice.kind === "local"
    ? shellLabel(choice.shell.name)
    : `${choice.config.username}@${choice.config.host}`;
}

/**
 * Which marks the tab bar should show.
 *
 * No mark before a session opens: the form on screen may be choosing a completely different target
 * than the one the tab will run, just as `dbBadgeMarks` does not mark a tab still on the connection
 * form.
 */
export function terminalBadgeMarks(
  choice: TerminalChoice | null,
  ended: boolean,
): TerminalBadgeMark[] {
  if (!choice) return [];
  const marks: TerminalBadgeMark[] = [{ type: choice.kind === "local" ? "local" : "ssh" }];
  if (ended) marks.push({ type: "ended" });
  return marks;
}

/**
 * The *Run on connect* field turned into exactly the keys to be typed on the user's behalf, or
 * `null` when there are none.
 *
 * `\r` rather than `\n`, and one at the end of the last line: the pty takes the Enter key, and a
 * command nobody pressed Enter on sits there waiting rather than running. Empty lines are dropped —
 * in the field they are breathing room; down in the shell they are a stray Enter printing an extra
 * prompt.
 *
 * Here rather than in `TerminalView` because it is pure: the same reason `terminalTarget` is here.
 */
export function openingKeystrokes(text: string | null | undefined): string | null {
  if (!text) return null;
  const lines = text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line !== "");
  return lines.length === 0 ? null : lines.map((line) => `${line}\r`).join("");
}
