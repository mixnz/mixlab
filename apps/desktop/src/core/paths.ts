/**
 * Paths as a person on this machine reads them — roadmap task T191.
 *
 * The daemon already answers in this system's spelling. What is left for the window are the joins
 * it makes itself from what a folder dialog returned, and the one split that names a SQLite tab.
 * A style is passed in rather than read inside, so the Windows cases are tested on every runner.
 */
import { IS_WINDOWS } from "./platform";

/** Which separator a path is written with. */
export type PathStyle = "windows" | "posix";

/** This machine's. */
export const PATH_STYLE: PathStyle = IS_WINDOWS ? "windows" : "posix";

/**
 * `path` with this system's separators. On Windows `/` becomes `\`; elsewhere nothing changes,
 * because a backslash there is a character of a file name, not a separator.
 */
export function nativePath(path: string, style: PathStyle = PATH_STYLE): string {
  return style === "windows" ? path.replace(/\//g, "\\") : path;
}

/** `base` and `relative` with exactly one separator between them, in this system's spelling. */
export function joinPath(base: string, relative: string, style: PathStyle = PATH_STYLE): string {
  const head = base.replace(/[\\/]+$/, "");
  const tail = relative.replace(/^[\\/]+/, "");
  return nativePath(`${head}/${tail}`, style);
}

/**
 * The last segment of a path, whichever separator it was written with. The whole path when it has
 * no segment of its own, so a tab title is never empty.
 */
export function fileName(path: string): string {
  const trimmed = path.trim();
  const segments = trimmed.split(/[\\/]/).filter((segment) => segment !== "");
  return segments[segments.length - 1] ?? trimmed;
}
