/** What a release changed, as the corner panel lists it: T188 D4, MixDB's rule from before T106. */

/** How many entries the panel shows before it counts the rest. Three is what fits in a corner
 *  without the panel becoming something to read rather than to glance at. */
export const MAX_HIGHLIGHTS = 3;

/** How long one entry may run before it is cut. Changelog lines are one short sentence, so this
 *  only catches the occasional long one. */
const MAX_HIGHLIGHT_LENGTH = 120;

/** Markdown as plain text: raw `code`, **bold** and link syntax read worse than the words in them. */
function stripMarkdown(text: string): string {
  return text
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replace(/[`*_]/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

/**
 * One entry per line.
 *
 * The notes are the changelog section for the version, so they open with `### Added` or
 * `### Changed` and the substance is the list under it: headings are skipped, and taking the first
 * line instead would announce every release as "Added". Entries wrap across lines in the changelog,
 * so a line that is not itself a bullet continues the one above it. Notes written as prose rather
 * than as a list still give their text.
 */
export function highlights(notes: string): string[] {
  const bullets: string[] = [];
  const prose: string[] = [];

  for (const line of notes.replace(/\r\n/g, "\n").split("\n")) {
    const trimmed = line.trim();
    if (trimmed === "" || /^#{1,6}\s/.test(trimmed)) continue;

    const bullet = /^[-*+]\s+(.*)$/.exec(trimmed);
    if (bullet) {
      bullets.push(bullet[1]);
    } else if (bullets.length > 0) {
      bullets[bullets.length - 1] += ` ${trimmed}`;
    } else {
      prose.push(trimmed);
    }
  }

  const source = bullets.length > 0 ? bullets : prose.length > 0 ? [prose.join(" ")] : [];

  return source
    .map(stripMarkdown)
    .filter((entry) => entry !== "")
    .map((entry) => (entry.length > MAX_HIGHLIGHT_LENGTH ? `${entry.slice(0, MAX_HIGHLIGHT_LENGTH - 1)}…` : entry));
}
