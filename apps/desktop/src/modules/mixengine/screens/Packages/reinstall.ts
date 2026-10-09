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
