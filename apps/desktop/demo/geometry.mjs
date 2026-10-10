/**
 * The window a scene or clip is filmed in. Pure, so `clips.test.mjs` can check a clip's scale
 * without starting anything. The design is docs/specs/2026-10-10-demo-clip-camera-design.md.
 */

/** The stills' window: what `npm run screenshots` always uses. */
export const VIEWPORT = { width: 1440, height: 900 };
export const SCALE = 2;

/**
 * The interface `uiScale` times larger in the same frame: a smaller viewport at a higher device
 * scale, so Chromium draws every glyph at the larger size rather than scaling a picture up.
 */
export function geometryFor(uiScale = 1) {
  return {
    viewport: { width: VIEWPORT.width / uiScale, height: VIEWPORT.height / uiScale },
    scale: SCALE * uiScale,
  };
}

/** A scale from 1 to 2 whose viewport is whole CSS pixels. */
export function isWholeGeometry(uiScale) {
  if (typeof uiScale !== "number" || uiScale < 1 || uiScale > 2) return false;
  const { viewport } = geometryFor(uiScale);
  return Number.isInteger(viewport.width) && Number.isInteger(viewport.height);
}
