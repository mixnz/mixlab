import { DEFAULT_FONT_SIZE, stepFontSize } from "./fontSize";

/** The cursor style. Exactly the three values xterm's `ITerminalOptions.cursorStyle` takes — not a
 *  list of the app's own, so it must not be any wider. */
export type CursorStyle = "block" | "underline" | "bar";

/**
 * How the terminal is displayed, shared by every tab and remembered between launches.
 *
 * One set rather than one per tab, for the same reason `rest/workspace.ts` keeps its four switches
 * in one place: the font size is about the user's eyes, and eyes do not change when switching
 * tabs.
 */
export interface TerminalSettings {
  /** A whole CSS font stack, not a font name: this goes straight into xterm's `fontFamily`, and a
   *  machine missing the first font still needs a way to fall back. */
  fontFamily: string;
  fontSize: number;
  scrollback: number;
  cursorStyle: CursorStyle;
  cursorBlink: boolean;
  /** `LocalShell.name` — `pwsh`, `git-bash`, `wsl:Ubuntu` — rather than a path. The same shell's
   *  path changes with the installation, while the name the backend generates is stable. */
  defaultShell: string | null;
  defaultCwd: string | null;
  /** Right-click pastes straight away instead of opening a menu — PuTTY's convention, and people
   *  used to it are very used to it. */
  rightClickPastes: boolean;
  /**
   * The tab carries the saved target's name instead of `user@host` or the shell name.
   *
   * For someone opening five tabs to five servers of the same system, `deploy@10.0.0.7` cannot
   * tell which is which, while "Prod DB" can. For someone opening only one or two sessions it is
   * the other way round — so this is a switch rather than a decision. A session not coming from a
   * saved target keeps its old name: there is nothing to show. See `terminalTitle` in
   * `session.ts`.
   *
   * On by default: someone who named a target has said which name they read, and the tab is the
   * only place that name is read back. Turning it off leaves the naming effort visible only in the
   * form.
   */
  titleShowsTargetName: boolean;
}

/** The app's own mono face, bundled, so a new terminal matches the rest of the window. */
export const DEFAULT_FONT_FAMILY = '"Geist Mono Variable", monospace';

/** Defaults earlier builds wrote into the settings file. They name fonts the app no longer bundles,
 *  so a file carrying one is carrying a default rather than a choice. */
const RETIRED_DEFAULT_FONT_FAMILIES: readonly string[] = ['"Fira Code", monospace'];

export const DEFAULT_SCROLLBACK = 5000;

/* Below 100 lines scrolling back no longer means anything, while above 100k each session holds a
   few hundred MB and a machine with ten terminal tabs is a machine out of memory. */
export const MIN_SCROLLBACK = 100;
export const MAX_SCROLLBACK = 100_000;

/** The `localStorage` key the previous round stored the font size under. It only still exists to
 *  be read one last time and then deleted — see {@link withLegacyFontSize} and
 *  `settingsStore.ts`. */
export const LEGACY_FONT_SIZE_KEY = "mixdb-terminal-font-size";

export const DEFAULT_SETTINGS: TerminalSettings = {
  fontFamily: DEFAULT_FONT_FAMILY,
  fontSize: DEFAULT_FONT_SIZE,
  scrollback: DEFAULT_SCROLLBACK,
  cursorStyle: "block",
  cursorBlink: true,
  defaultShell: null,
  defaultCwd: null,
  rightClickPastes: false,
  titleShowsTargetName: true,
};

const CURSOR_STYLES: readonly CursorStyle[] = ["block", "underline", "bar"];

/** The number of lines kept, clamped to the range. Clamped rather than refused, just like
 *  `stepFontSize`. */
export function clampScrollback(lines: number): number {
  if (!Number.isFinite(lines)) return DEFAULT_SCROLLBACK;
  return Math.min(MAX_SCROLLBACK, Math.max(MIN_SCROLLBACK, Math.round(lines)));
}

function text(value: unknown, fallback: string): string {
  return typeof value === "string" && value.trim() !== "" ? value : fallback;
}

function optionalText(value: unknown): string | null {
  return typeof value === "string" && value.trim() !== "" ? value : null;
}

/**
 * A record read from disk turned into usable settings.
 *
 * No field is trusted: `terminal-settings.json` is a file the user can open and edit, and the
 * app's previous version wrote a file missing exactly the fields this version added. Both cases
 * come to the same job — keep the fields that make sense, and take the default for those that do
 * not.
 */
export function sanitizeSettings(raw: unknown): TerminalSettings {
  const record = typeof raw === "object" && raw !== null ? (raw as Record<string, unknown>) : {};
  const cursorStyle = record.cursorStyle;
  return {
    fontFamily: RETIRED_DEFAULT_FONT_FAMILIES.includes(record.fontFamily as string)
      ? DEFAULT_FONT_FAMILY
      : text(record.fontFamily, DEFAULT_FONT_FAMILY),
    // `?? Number.NaN` rather than leaving `Number(undefined)` alone: `Number(null)` gives 0, and 0
    // clamps to the smallest font size instead of to the default.
    fontSize: stepFontSize(Number(record.fontSize ?? Number.NaN), 0),
    scrollback: clampScrollback(Number(record.scrollback ?? Number.NaN)),
    cursorStyle: CURSOR_STYLES.includes(cursorStyle as CursorStyle)
      ? (cursorStyle as CursorStyle)
      : DEFAULT_SETTINGS.cursorStyle,
    cursorBlink:
      typeof record.cursorBlink === "boolean" ? record.cursorBlink : DEFAULT_SETTINGS.cursorBlink,
    defaultShell: optionalText(record.defaultShell),
    defaultCwd: optionalText(record.defaultCwd),
    rightClickPastes: record.rightClickPastes === true,
    /* `typeof` rather than `=== true`, unlike the line above: this one defaults to on, so a file
       missing it has to take the default, while `false` written in the file is something the user
       said and has to survive. The same shape as `cursorBlink`. */
    titleShowsTargetName:
      typeof record.titleShowsTargetName === "boolean"
        ? record.titleShowsTargetName
        : DEFAULT_SETTINGS.titleShowsTargetName,
  };
}

/**
 * The settings, plus the font size the previous round left in `localStorage`.
 *
 * Where the font size is kept moves from `localStorage` to `terminal-settings.json` in this round.
 * Someone who raised the font size to 20 has no reason to see it back at 14 after updating, so the
 * old value is read exactly once more — while the file has no font size of its own yet — and then
 * the old key is deleted.
 *
 * A pure function, taking `legacy` as a parameter rather than reading `localStorage` itself:
 * `settingsStore.ts` does the reading, while the rule lives here, where tests can reach it without
 * a browser.
 */
export function withLegacyFontSize(raw: unknown, legacy: string | null): TerminalSettings {
  const settings = sanitizeSettings(raw);
  const record = typeof raw === "object" && raw !== null ? (raw as Record<string, unknown>) : {};
  if (record.fontSize !== undefined && record.fontSize !== null) return settings;
  if (legacy === null) return settings;
  const size = Number(legacy);
  if (!Number.isFinite(size)) return settings;
  return { ...settings, fontSize: stepFontSize(size, 0) };
}
