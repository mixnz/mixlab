import type { Press } from "../../core/shortcuts";

/**
 * Whether this key belongs to the shell or to the app.
 *
 * A terminal differs from every other pane of the app in that its default is to *yield*: `Ctrl+A`
 * goes to the start of the line, `Ctrl+R` is reverse history search, `Ctrl+D` is end of input. So
 * the question is not "does the app use this key" but "is anyone in the app really listening to
 * this key right now" — and that list is answered by `isClaimed`, reading exactly the catalogue
 * the dispatcher reads.
 *
 * A pure function, no DOM, no xterm: the rule is worth testing, the hookup to xterm is not.
 */
export function shellKeeps(press: Press, claimed: boolean): boolean {
  /* Paste is the only exception that does not ask the catalogue. There is no handler at all —
     letting go makes the webview paste into xterm's textarea by itself, and xterm already listens
     for the `paste` event there. Catching it and reading the clipboard ourselves would just rewrite
     something already running, with an API the webview is entitled to refuse.

     Tied to `mod` rather than `ctrlOnly`: on a Mac, paste is `⌘V` while `Ctrl+V` inserts
     readline's literal control character — two different keys, and the latter belongs to the
     shell. */
  if (press.mod && press.key === "v") return false;
  /* Holding no shortcut modifier means nothing to contend over: typing is typing. `ctrlOnly` is
     asked too because some chords run on `Ctrl` on every platform — `Ctrl+Tab` — so on a Mac `mod`
     alone would miss it. */
  if (!press.mod && !press.ctrlOnly) return true;
  return !claimed;
}
