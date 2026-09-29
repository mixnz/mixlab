/**
 * Which fonts in the list really exist on this machine.
 *
 * No API can ask this directly. `document.fonts.check` returns `true` even for names that do not
 * exist, and `queryLocalFonts` demands a permission prompt the app has nowhere to ask for. So the
 * only way left is the one everyone uses: measure a sample string in that font, then compare it
 * with the same string measured in a fallback font. Equal means the browser has fallen back — the
 * font is not here.
 *
 * Three fallback fonts rather than one: a monospace font may be exactly as wide as the machine's
 * default `monospace` — it *is* that font — but almost never as wide as both `serif` and
 * `sans-serif`.
 *
 * Here rather than in `fonts.ts` because it needs a canvas; `fonts.ts` keeps the pure rules that
 * tests can reach.
 */

/** Mixes narrow and wide characters: two different fonts are unlikely to give the same width for
 *  the whole run. */
const PROBE_TEXT = "mmmmmmmmmmlliWW0O";

/** Much larger: every small difference in glyph shape becomes a few pixels rather than a fraction
 *  of a pixel lost to rounding. */
const PROBE_SIZE = 72;

const FALLBACKS = ["monospace", "serif", "sans-serif"] as const;

export async function installedFonts(candidates: readonly string[]): Promise<string[]> {
  /* Wait for fonts to load before measuring. Geist Mono is not a system font but a webfont bundled
     with the app — see `main.tsx` — so measuring one beat early finds the browser still using the
     fallback, and the terminal's own default font is judged "not on this machine". */
  await document.fonts.ready;
  const ctx = document.createElement("canvas").getContext("2d");
  // If it cannot be measured, do not guess: return an empty list, and the picker will only have
  // the font in use.
  if (!ctx) return [];

  const baseline = FALLBACKS.map((fallback) => {
    ctx.font = `${PROBE_SIZE}px ${fallback}`;
    return ctx.measureText(PROBE_TEXT).width;
  });

  return candidates.filter((name) =>
    FALLBACKS.some((fallback, i) => {
      ctx.font = `${PROBE_SIZE}px "${name}", ${fallback}`;
      return ctx.measureText(PROBE_TEXT).width !== baseline[i];
    }),
  );
}
