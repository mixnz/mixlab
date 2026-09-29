import type { ShortcutGroup } from "../../core/shortcuts";

/**
 * The terminal module's shortcuts, handed to the shell through `ModuleDefinition.shortcuts` — just
 * like `REST_SHORTCUTS`.
 *
 * Short on purpose: every chord not named here belongs to the shell. Paste is absent because it has
 * no handler — see `keys.ts`.
 */
export const TERMINAL_SHORTCUTS: ShortcutGroup[] = [
  {
    scope: "terminal",
    labelKey: "terminal.shortcutScope",
    defs: [
      /* The same `Ctrl/Cmd+C` as the shell's interrupt, and the arbitration is that `TerminalView`
         only registers it while there is a selection: with one it copies, without one the key falls
         through to the shell intact. On macOS the question does not arise — `Cmd+C` is this chord,
         and `Ctrl+C` carries no shortcut at all. */
      { id: "terminal.copy", chord: { key: "c" }, labelKey: "terminal.shortcutCopy" },
      /* `Ctrl/Cmd+C` again, and it collides with nobody either: it is only registered once the
         session has ended, when there is no shell left to interrupt. The same key, on purpose —
         once the screen has frozen, what people type by reflex to get out is still that key. */
      { id: "terminal.dismiss", chord: { key: "c" }, labelKey: "terminal.shortcutDismiss" },
      /* `Ctrl/Cmd` with `+` and `-`, exactly where every browser and every other terminal puts it.
         Three ways of typing one gesture, hence `alias` — see `ShortcutDef.alias`; here `+` without
         shift is the numeric keypad's key, while `+` with shift is the `=` key when shift really is
         held. */
      {
        id: "terminal.zoomIn",
        chord: { key: "=" },
        alias: [{ key: "+" }, { key: "+", shift: true }],
        labelKey: "terminal.shortcutZoomIn",
      },
      {
        id: "terminal.zoomOut",
        chord: { key: "-" },
        alias: [{ key: "_", shift: true }],
        labelKey: "terminal.shortcutZoomOut",
      },
      /* No `whenTyping: "ignore"`: the cursor sitting in the find field also counts as "typing",
         and `Ctrl+F` then has to reselect the field's contents rather than fall through to the
         webview. Inside the terminal screen `isTextEntry` also returns `true` — xterm's hidden
         textarea — so with that flag this def would be skipped entirely, and the shortcut would
         never run. */
      { id: "terminal.find", chord: { key: "f" }, labelKey: "terminal.shortcutFind" },
    ],
  },
];
