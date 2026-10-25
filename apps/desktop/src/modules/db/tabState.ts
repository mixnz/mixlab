/**
 * What a database tab remembers between launches: the saved connection it was on, whether it was
 * still open when the app closed, and nothing else.
 *
 * An id and a boolean. The connection itself lives in `connections.json` with its password in the
 * OS credential store, both of them guarded; a uuid in `localStorage` says that a connection was
 * once open in this tab and not where it goes. Adding a field here is a decision about that line —
 * see §4 of `docs/specs/2026-08-23-tab-session-context-design.md`. `connected` passes
 * it because it says nothing about the server: it is a fact about this tab, and it is the
 * difference between a tab that dials on its own next launch and one that comes back holding the
 * form.
 *
 * A connection typed into the form and never saved has no id to point at, so it is not restored.
 * That is the line holding, not a gap in it.
 */
/** A tab pointing at a saved connection, connected or back at the form. */
export interface SavedTabState {
  savedId: string;
  /** Whether the tab was in the workspace, rather than back at the form, when it was last written.
   *  Deciding what restoring does, not whether it happens. */
  connected: boolean;
}

/**
 * A tab opened for a connection another program handed over, before it has taken it.
 *
 * The backend keeps the connection — password included — under this id until the tab asks for it
 * once (`handoff.ts`); the tab forgets the id the moment it does. Still an id and nothing else, and
 * one that stops meaning anything within a second of being written: a session that somehow keeps
 * it comes back to an empty form.
 */
export interface HandoffTabState {
  handoffId: string;
}

/**
 * A tab MixEngine asked for after moving one of its services to another port, so the connections
 * saved for it can be offered the new one. Acted on once and never written back: the tab replaces
 * it with its own state as soon as it opens.
 */
export interface ServiceMovedTabState {
  serviceMoved: { service: string; from: number; to: number };
}

export type DbTabState = SavedTabState | HandoffTabState | ServiceMovedTabState;

/**
 * The stored value, if it is one, or `null`.
 *
 * This is where validation lives — the shell passes the slot through untouched on purpose, because
 * only this module knows the shape. Everything arriving here is a string some older version of the
 * app wrote, so nothing in it is trusted.
 *
 * `connected` is the one field with a fallback rather than a rejection: a session from before it
 * existed only ever wrote a state while connected, so its absence means `true` and not "unusable".
 */
export function parseDbTabState(value: unknown): DbTabState | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const state = value as Record<string, unknown>;
  if (typeof state.savedId === "string" && state.savedId !== "") {
    return { savedId: state.savedId, connected: state.connected !== false };
  }
  if (typeof state.handoffId === "string" && state.handoffId !== "") {
    return { handoffId: state.handoffId };
  }
  const moved = state.serviceMoved;
  if (typeof moved === "object" && moved !== null && !Array.isArray(moved)) {
    const { service, from, to } = moved as Record<string, unknown>;
    if (typeof service === "string" && service !== "" && isPort(from) && isPort(to)) {
      return { serviceMoved: { service, from, to } };
    }
  }
  return null;
}

function isPort(value: unknown): value is number {
  return typeof value === "number" && Number.isInteger(value) && value >= 1 && value <= 65535;
}
