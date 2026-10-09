/** The release that puts back an install whose folder is gone — T206d. The same version, because
 *  an install of that version restores its row in place; `null` when the catalogue no longer
 *  offers it, and the button says so rather than installing something else. */
export function releaseToRestore<R extends { version: string }>(
  releases: R[],
  matches: (release: R) => boolean,
  version: string,
): R | null {
  return releases.find((release) => matches(release) && release.version === version) ?? null;
}

/** What a Reinstall button shows: busy while its own install runs, so a second click starts nothing,
 *  disabled when the catalogue no longer offers the version, and ready otherwise — T206d. */
export function reinstallState(
  restore: { version: string } | null,
  running: boolean,
): "running" | "unavailable" | "ready" {
  if (running) return "running";
  return restore === null ? "unavailable" : "ready";
}
