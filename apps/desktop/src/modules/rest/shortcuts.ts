import type { ShortcutGroup } from "../../core/shortcuts";

/**
 * The chords this module's panes answer, handed to the shell through
 * `ModuleDefinition.shortcuts` — the same way `DB_SHORTCUTS` is.
 *
 * Labelled from this module's own dictionary: the `shortcuts.*` group belongs to the shell, and a
 * second dictionary claiming it stops the build.
 */
export const REST_SHORTCUTS: ShortcutGroup[] = [
  {
    scope: "rest",
    labelKey: "rest.shortcutScope",
    defs: [
      /* No `whenTyping: "ignore"`: sending from inside the body editor is the whole reason this
         chord exists, and a body is a textarea.

         `Ctrl/Cmd+R` sends as well, as an alias rather than a chord of its own — it is the same
         gesture spelled the other way round, and the key is free here: `pane.reload` is registered
         by panes that carry a reload button and a REST pane carries none, while `App.tsx` already
         keeps the webview from acting on it. */
      {
        id: "rest.send",
        chord: { key: "enter" },
        alias: [{ key: "r" }],
        labelKey: "rest.shortcutSend",
      },
      { id: "rest.newRequest", chord: { key: "n" }, labelKey: "rest.shortcutNewRequest" },
      /* Shares `Ctrl/Cmd+W` with the shell's `app.closeTab`. `decide()` resolves a clash in favour
         of whichever handler started listening last, which is this one — and it is registered only
         while a request tab is open, so an empty REST workspace still closes the MixLab tab. */
      { id: "rest.closeRequest", chord: { key: "w" }, labelKey: "rest.shortcutCloseRequest" },
      { id: "rest.history", chord: { key: "h" }, labelKey: "rest.shortcutHistory" },
    ],
  },
];
