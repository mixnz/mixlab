export { decide, type Decision, type Press, type ShortcutContext } from "./decide";
/** `modalDepth` travels with `enterModal` for the same reason the dispatcher reads it: a pane that
 *  listens for a gesture itself, rather than through a shortcut def, has to keep the same quiet
 *  while a dialog or a menu holds the keyboard. */
export { enterModal, modalDepth } from "./store";
export {
  isClaimed,
  pressOf,
  useShortcut,
  useShortcutDispatcher,
  useShortcuts,
  type ShortcutEntry,
} from "./useShortcut";
export type { Chord, ShortcutDef, ShortcutGroup } from "./types";
