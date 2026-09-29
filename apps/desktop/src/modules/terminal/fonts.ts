import { DEFAULT_FONT_FAMILY } from "./settings";

/**
 * Monospace fonts worth offering to a terminal.
 *
 * A hand-written list rather than every font the machine has, and on purpose: this picker chooses
 * a font *for a terminal*, and a terminal needs exactly one thing — every character equally wide.
 * Offering all the machine's thousand fonts invites the user to pick a proportional one, and the
 * screen misaligns its columns from the very first line.
 *
 * `fontProbe.ts` filters this list down to the ones the machine really has.
 */
export const TERMINAL_FONTS: readonly string[] = [
  "Anonymous Pro",
  "Cascadia Code",
  "Cascadia Mono",
  "Consolas",
  "Courier New",
  "Cousine",
  "DejaVu Sans Mono",
  "Fira Code",
  "Fira Mono",
  // Bundled with the app (see `main.tsx`) and the default, so it is always offered.
  "Geist Mono Variable",
  "Hack",
  "IBM Plex Mono",
  "Inconsolata",
  "Iosevka",
  "JetBrains Mono",
  "Liberation Mono",
  "Lucida Console",
  "Menlo",
  "MesloLGS NF",
  "Monaco",
  "Noto Sans Mono",
  "PT Mono",
  "Roboto Mono",
  "SF Mono",
  "Source Code Pro",
  "Space Mono",
  "Ubuntu Mono",
  "Victor Mono",
];

/**
 * A font name as a whole font stack to hand to xterm.
 *
 * Always has `monospace` at the end, and this is the most important part of the whole file. xterm
 * measures a cell's width with `ctx.font = "<size>px <stack>"` on a canvas; with an empty or
 * unparseable stack that assignment is ignored *silently* and the old measurement stays, while the
 * CSS `font-size` xterm injects into the screen still changes. The result is text growing while the
 * cells stay put, and every line cut across.
 */
export function fontStack(family: string): string {
  const name = family.trim();
  if (name === "") return DEFAULT_FONT_FAMILY;
  return `"${name.replace(/["\\]/g, "")}", monospace`;
}

/** The font name at the head of a stack — the reverse of {@link fontStack}, so the picker knows
 *  which one is chosen. An unreadable stack returns the default font's name rather than an empty
 *  string: the picker has to point at some entry. */
export function familyOf(stack: string): string {
  const first = stack.split(",")[0]?.trim() ?? "";
  const bare = first.replace(/^["']|["']$/g, "").trim();
  if (bare === "") return familyOf(DEFAULT_FONT_FAMILY);
  return bare;
}
