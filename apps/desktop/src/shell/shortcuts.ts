import type { ShortcutDef, ShortcutGroup } from "../core/shortcuts";
import type { ModuleDefinition } from "./module";

/** How many modules get a chord of their own: `Ctrl/Cmd+1` to `Ctrl/Cmd+9`, in the order they are
 *  drawn. A tenth module would need a scheme this is not, so it simply goes without — the `[+]`
 *  menu opens it, and `Ctrl/Cmd+T` still opens the default. */
const MAX_NUMBERED_MODULES = 9;

/** The id the chord for one module's new tab is filed under. `app.newTab` opens the default one and
 *  keeps its plain id; these are named after the module so the pair reads together in a log. */
export function newModuleTabId(moduleId: string): string {
  return `app.newTab.${moduleId}`;
}

/**
 * A number key per visible module: `Ctrl/Cmd+1` opens a tab of the first one on the strip, `2` the
 * second.
 *
 * A function of the visible list rather than a constant over the registry — T108. Which modules are
 * drawn is a setting now, and a chord that pointed at a module the user had turned off would be a
 * key nothing answers with a row in the table saying it does.
 *
 * Derived rather than written out, and paired with the module each one opens, because
 * `Workspace.tsx` registers its handlers from this same list. A chord in the table that no handler
 * answers, or a handler under an id the dispatcher has never heard of, are both things nothing
 * would have said out loud — so neither list is written twice. The ids stay named after the module
 * and not after the position, so anything that ever files against one — a remap screen — outlives a
 * profile change.
 *
 * **`Workspace.tsx` may not loop over this.** Its `useShortcut` calls have to be the same count on
 * every render, and this list shrinks with the profile; the loop there is over the registry, and it
 * passes `enabled` instead. See the note beside it.
 */
export function moduleTabShortcuts(
  visible: ModuleDefinition[],
): { moduleId: string; def: ShortcutDef }[] {
  return visible.slice(0, MAX_NUMBERED_MODULES).map((module, i) => ({
    moduleId: module.id,
    def: {
      id: newModuleTabId(module.id),
      chord: { key: String(i + 1) },
      /* A module that holds one tab and no more never opens a second, so its row says the one
         thing its key does: open that tab, or go to it. The other four are always openable and the
         row has said *New … tab* since there were rows. See `Workspace.tsx`, which answers the
         chord the same way round. */
      labelKey: module.singleTab ? "shortcuts.goToModule" : "shortcuts.newModuleTab",
      // The module's own name, so the table says "New REST tab" without this file knowing there is
      // a REST module — the registry is still the only place outside `src/modules/` that may.
      labelVars: { module: module.labelKey },
      // As `app.newTab` is, and for no better reason than that these are the same command with the
      // module chosen. If one of them stops answering from behind a dialog, they all should.
      inModal: true,
    },
  }));
}

/** The chords the app answers wherever you are — the tab bar's, and the reload every pane shares. */
function shellShortcuts(visible: ModuleDefinition[], openable: ModuleDefinition[]): ShortcutGroup[] {
  return [
    {
      scope: "app",
      labelKey: "shortcuts.scope.app",
      defs: [
        /* `inModal` because that is what they do today: `Workspace.tsx` guards neither, so a new tab
           opens and a tab closes from behind an open dialog. The registry is the first thing that
           made the question visible, and a refactor that answers it differently is a refactor
           nobody can trust. Deciding otherwise later is one flag on one line. */
        ...(openable.length > 0
          ? [
              {
                id: "app.newTab",
                chord: { key: "t" },
                labelKey: "shortcuts.newTab",
                inModal: true,
              } satisfies ShortcutDef,
            ]
          : []),
        /* Every visible module, openable or not: a module that cannot take a second tab answers its
           key by going to the first — see `firstTabOfModule`. `app.newTab` above is the only one
           that goes, because "one more" is the only thing it can mean. */
        ...moduleTabShortcuts(visible).map((entry) => entry.def),
        { id: "app.closeTab", chord: { key: "w" }, labelKey: "shortcuts.closeTab", inModal: true },
        /* `ctrl` rather than the platform's modifier, because `Cmd+Tab` never reaches a Mac app —
           see `Chord.ctrl`. Not `inModal`, unlike the three above: those carry the flag because
           the shell never guarded them and a refactor is no place to change what a key does, while
           these two are new and nothing is owed to them. Moving the tab underneath a dialog while
           the dialog stays where it is would be a screen that disagrees with itself. */
        {
          id: "app.nextTab",
          chord: { key: "tab", ctrl: true },
          labelKey: "shortcuts.nextTab",
          unhandled: "swallow",
        },
        {
          id: "app.prevTab",
          chord: { key: "tab", ctrl: true, shift: true },
          labelKey: "shortcuts.prevTab",
          unhandled: "swallow",
        },
        /* Not `inModal`: the pane behind a dialog is not the one in front, and a reload fired from
           behind a confirmation acts on the very thing being asked about.

           `swallow`, because a press no pane answers would otherwise reload the webview and take
           every open connection with it. That refusal is made here and not by a second keydown
           listener: two listeners on `window` run in the order they were added, the dispatcher's
           is re-added whenever the catalogue changes, and a refusal that ran first left the
           dispatcher looking at an event already prevented — `Ctrl+R` reloaded nothing at all. */
        {
          id: "pane.reload",
          chord: { key: "r" },
          labelKey: "shortcuts.reload",
          unhandled: "swallow",
        },
      ],
    },
  ];
}

/**
 * Every chord in the app, the shell's and the visible modules'.
 *
 * Assembled here rather than inside `core/shortcuts/`: that folder is the mechanism and may not
 * import from `shell/` or `modules/` at all — see `docs/architecture/desktop/frontend.md`. The
 * dispatcher is handed this list; it never goes looking for one.
 *
 * A hidden module contributes nothing: its panes are never mounted, so nothing would answer its
 * chords, and the shortcut table draws exactly this list.
 *
 * `openable` is the same list narrowed to the modules a new tab can still be opened of — see
 * `openableModules` in `shell/tabs.ts`. It decides one thing: whether `app.newTab` is here at all.
 * A module's own number key is always here, because a module with nothing left to open answers it
 * by going to the tab it has. Defaults to the whole of `visible`, which is what a window with no
 * tabs in it yet would pass.
 *
 * **The result must be memoized by the caller.** It was a module-level constant until T108 made it
 * a function of the visible modules; `useShortcutDispatcher` rebinds its listener whenever the
 * array's identity changes, and the Settings table is handed the very same value so the two cannot
 * come to disagree.
 */
export function shortcutsFor(
  visible: ModuleDefinition[],
  openable: ModuleDefinition[] = visible,
): ShortcutGroup[] {
  return [...shellShortcuts(visible, openable), ...visible.flatMap((m) => m.shortcuts ?? [])];
}
