import type { ConnectionConfig } from "../types";

/**
 * Whether a handed-over connection is dialled the moment it arrives, or shown in the form first.
 *
 * MixEngine's handoff carries the password in the launched process's environment, and a Redis or a
 * MongoDB it manages has no accounts at all: all of them are dialled at once, which is the whole
 * point of a handoff.
 * A `mixlab://` link opened from a browser or a document looks the same on the wire but carries no
 * environment — nobody can set one for a link — so a server that has accounts is not dialled with
 * an empty password only to fail. The form opens filled in, with the caret in the password field,
 * and Connect is one keystroke away.
 *
 * Pure, and apart from `handoff.ts`, which reaches the backend and is therefore not testable here.
 */
export function arrivesConnected(config: ConnectionConfig): boolean {
  if (config.password !== undefined && config.password !== "") return true;
  return config.kind === "redis" || config.kind === "mongo";
}
