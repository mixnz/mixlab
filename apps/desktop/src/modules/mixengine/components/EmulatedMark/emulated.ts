import type { Execution } from "@mixengine/api";

/**
 * Whether a release would run under the operating system's emulation — ADR 0023. Only the daemon's
 * own word counts: a daemon from before `execution` reports nothing, which is no claim that
 * anything is emulated (ADR 0019).
 */
export function isEmulated(release: { execution?: Execution | null }): boolean {
  return release.execution === "emulated";
}
