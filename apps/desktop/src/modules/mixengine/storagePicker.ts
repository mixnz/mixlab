import type { StorageReport } from "@mixengine/api";

import { joinPath, PATH_STYLE, type PathStyle } from "../../core/paths";
import type { ChosenPaths } from "./api";

/**
 * The gate turns the `--storage` answer into rows to draw, and those rows into start-up flags.
 *
 * Pure and calls nothing — for the same reason `daemonState.ts` lives here and not in a component:
 * the rule worth testing is *which keys go onto the command line*, and a `useState` cannot be
 * tested. Sending an extra key breaks nothing (the daemon treats an equal value as a silent no-op),
 * but it says three things we do not mean, and it makes `config.toml` get rewritten for a click
 * that changed nothing.
 */

/** The four movable `[paths]` keys, in the order the config file lists them. */
export const KEYS = ["runtimes", "packages", "data", "logs"] as const;

export type StorageKey = (typeof KEYS)[number];

/** One row of the picker table. Only what the table draws. */
export interface StorageRow {
  key: StorageKey;
  /** Where it currently is, according to the daemon. */
  current: string;
  /** Whether it lies outside the home — the daemon answers; this side does not compare strings. */
  relocated: boolean;
  /** Where the user just chose, or `null` when they have not chosen anything. */
  picked: string | null;
}

/** Four rows from an answer, with nothing chosen yet. */
export function rowsFrom(report: StorageReport): StorageRow[] {
  return KEYS.map((key) => ({
    key,
    current: report.paths[key].path,
    relocated: report.paths[key].relocated,
    picked: null,
  }));
}

/** One row after the user has chosen a directory for it. */
export function pick(rows: StorageRow[], key: StorageKey, directory: string): StorageRow[] {
  return rows.map((row) => (row.key === key ? { ...row, picked: directory } : row));
}

/**
 * The four rows after the user has chosen **one** directory for all four.
 *
 * `<directory>\runtimes`, `<directory>\packages`, … — the common case is "put everything on the
 * other drive", and making people click four times for one intention is making them do the
 * machine's work. Joined with the operating system's separator, like the value the per-row button
 * sends: the daemon writes `config.toml` through `toml_edit`, which escapes backslashes itself
 * (T191).
 */
export function oneFolderFor(
  rows: StorageRow[],
  directory: string,
  style: PathStyle = PATH_STYLE,
): StorageRow[] {
  return rows.map((row) => ({ ...row, picked: joinPath(directory, row.key, style) }));
}

/**
 * The keys that need to be sent, and only those.
 *
 * A row nobody has chosen is not sent. Nor is a row chosen to exactly where it already is — the
 * daemon would treat it as a no-op, but we do not rely on that: what gets sent should be *what
 * changed*, so what we tell the daemon matches exactly what we mean.
 *
 * `undefined` when nothing changed, because that is what `startDaemon` takes for "nothing chosen"
 * — and the frontend would read an empty object as "something chosen" when nothing was.
 */
export function chosenFrom(rows: StorageRow[]): ChosenPaths | undefined {
  const chosen: ChosenPaths = {};
  let any = false;

  for (const row of rows) {
    if (row.picked !== null && row.picked !== row.current) {
      chosen[row.key] = row.picked;
      any = true;
    }
  }

  return any ? chosen : undefined;
}

/** Whether the choice is still open — the daemon answers; this side does not infer it from the
 *  path. */
export function isFree(report: StorageReport): boolean {
  return report.changeable.changeable === "free";
}

/**
 * The daemon's sentence about what has been installed, or `null` when nothing has.
 *
 * The daemon's sentence rather than one built on this side: *what has been installed* is a
 * measurement it just made, and a client rewriting that sentence is a second answer to the same
 * question — the same rule the Doctor and Uninstall screens follow.
 */
export function explanationOf(report: StorageReport): string | null {
  return report.changeable.changeable === "taken" ? report.changeable.explanation : null;
}
