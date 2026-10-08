import type { ModuleAction } from "../core/moduleActions";
import type { ComponentType, ReactNode } from "react";
import type { ShortcutGroup } from "../core/shortcuts";
import type { IconProps } from "../icons";
import type { TranslationKey } from "../i18n";
import type { SyncableCollection } from "../core/syncCollection";

/**
 * A mark a module wants on its own tab.
 *
 * The shell draws it and nothing more — it does not know what a `kind-mysql` is, only that
 * something asked for that class. This is what keeps the tab bar from growing a branch per module:
 * the two marks a database tab carries were `tab.kind` and `tab.readOnly` up here, and neither
 * meant anything to a REST client or a terminal.
 */
export interface TabBadge {
  /** Distinct within one tab's badges; the shell keys the list on it. */
  id: string;
  icon: ReactNode;
  /** Read aloud. Already translated — the shell does not know the module's i18n namespace. */
  label: string;
  /** Shown on hover. Left out when the mark needs no tooltip of its own. */
  title?: string;
  /** A class the module defines, put on the badge. */
  className?: string;
  /** A class put on the whole tab rather than on this badge — for a mark that colours the tab
   *  itself, the way a read-only connection turns its accent bar amber. */
  tabClassName?: string;
}

export interface ModuleTabProps {
  /** Whether this is the tab on screen. Every tab that has been on screen once stays mounted
   *  behind it, so the panes below need telling which of them a keyboard shortcut is meant for.
   *  (A tab restored from the last session has not been on screen yet and is not mounted at all —
   *  see `shell/session.ts`. Nothing a module writes can tell the difference: its first render is
   *  its first render either way.) */
  active: boolean;
  /**
   * Whether this window is drawing the module with this id.
   *
   * For a screen that offers a way *into* another module and must not offer it as though it were
   * already there — T110. A question about what is **drawn**, not about what exists: every
   * module's backend commands are registered in every build, and a hidden module's files are on
   * disk (T108's D8).
   *
   * A predicate rather than the visible list, because a module has no business enumerating the
   * others. Which id to ask about is the asking module's own, and it says so out loud — see
   * `modules/mixengine/screens/ServicesDetail/openChoices.ts`.
   */
  isModuleVisible: (moduleId: string) => boolean;
  onTitleChange: (title: string) => void;
  onBadgesChange: (badges: TabBadge[]) => void;
  /**
   * What this module wrote for this tab the last time the app was open, or `undefined`.
   *
   * **Read it once, at mount** — a `useState` initializer — and work from that snapshot. It is a
   * prop rather than an argument only because there is nowhere else to put it: read it reactively
   * and the module overwrites itself the moment it writes. Nothing here can enforce that; it is a
   * rule, and it is in `docs/standards/desktop/adding-a-module.md` too.
   *
   * Reading once is not the same as acting once. A module whose store still has a file to read
   * waits for it before deciding whether what it is pointing at is gone.
   */
  restored?: unknown;
  /** What to hand back next launch. `undefined` means "forget it", not "unchanged". Must be
   *  called from an event handler, or from an effect that does not depend on this callback: the
   *  shell compares by identity, and the loop that catches otherwise is named in `shell/tabs.ts`.
   *  Ids only — see the spec this came from, linked at the top of `shell/session.ts`. */
  onStateChange: (state: unknown) => void;
}

/** A pane a module adds to the app's Settings dialog. */
export interface ModuleSettingsSection {
  labelKey: TranslationKey;
  Icon: ComponentType<IconProps>;
  Section: ComponentType;
}

/** What the tray's frame hands every section it draws. */
export interface TraySectionProps {
  /** The card is on screen. Always true on Linux, where the panel is a window and never slides. */
  shown: boolean;
  /** The panel's window has focus. A section reads its state again when this turns true. */
  focused: boolean;
  /** Slides the panel out and hides it, as Esc and a click elsewhere do. */
  dismiss: () => void;
}

/**
 * One thing MixLab can open a tab of.
 *
 * Deliberately without lifecycle hooks or an event bus between modules: a module cleans up in its
 * own `useEffect` and saves through its own store, and inventing a need nobody has yet is how a
 * shell ends up harder to add to than what it was meant to simplify.
 *
 * The one thing it does keep for a module is a single opaque slot per tab — `restored` and
 * `onStateChange` in {@link ModuleTabProps} — so a tab can come back to what it had open. That is
 * not a persistence API: the shell writes the value and hands it back, and has no idea what it is.
 */
export interface ModuleDefinition {
  id: string;
  /** The module's name in the `[+]` menu. */
  labelKey: TranslationKey;
  Icon: ComponentType<IconProps>;
  /** A tab's title when it is first opened, before the module names it. */
  defaultTitleKey: TranslationKey;
  Tab: ComponentType<ModuleTabProps>;
  /**
   * That a second tab of this module would hold nothing the first one does not.
   *
   * The default is `false`, and for most modules that is the point of a tab: one connection, one
   * session, one request each. A module sets this when its tab is a view onto a single thing that
   * exists once per machine — and the shell then stops offering to open another, on the `[+]`
   * button, in the menu behind it, and on the chords that do the same thing (`shell/tabs.ts`).
   *
   * It says nothing about how many tabs may *exist*: a session restored from a build before the
   * flag, or from a profile where the module was one of several, is drawn and works like any other.
   * This is only about opening one more.
   */
  singleTab?: boolean;
  settings?: ModuleSettingsSection;
  /** The Ctrl/Cmd chords this module's panes answer, for the dispatcher to resolve and for
   *  Settings to list. Contributed exactly the way `settings` is: the shell collects them and
   *  knows nothing about what any of them do. */
  shortcuts?: ShortcutGroup[];
  /**
   * A section of the tray panel — T192, ADR 0058, `src/shell/tray/TrayFrame.tsx`.
   *
   * The shell owns the panel: its window, its slide and a header with Open MixLab and Quit MixLab.
   * A module only lends it what goes under that header, and the panel exists only while a visible
   * module does. Without one, a click on the icon opens the main window.
   */
  TraySection?: ComponentType<TraySectionProps>;
  /** The collections this module lends to sync (the design's D5). The shell offers them and
   *  never learns what an item is — see `core/syncCollection.ts`. */
  syncable?: SyncableCollection[];
  /** What this module lets another one ask of it, by name — `core/moduleActions.ts`. The shell
   *  hands them over and never learns what they do. */
  actions?: Record<string, ModuleAction>;
}
