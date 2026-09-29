/**
 * A one-way bridge from Projects to Sites: "open Sites, already filtered by project X".
 *
 * Sites keeps its own local `projectFilter`, read neither from the URL nor from tab state — so
 * there is nowhere for Projects to put that value directly other than waiting for Sites to read it
 * when it becomes `active`. This module is exactly one variable in between, in the same mould as
 * `daemonWatch.ts` (module-level state, not a React context), because only one Sites exists in the
 * MixEngine tab at a time.
 *
 * **`take` rather than `peek`** — read once, then cleared, so that the next time the user changes
 * the filter by hand it is not overwritten again by a stale navigation request.
 */
let pendingProjectFilter: string | null = null;

export function requestSitesFilter(project: string): void {
  pendingProjectFilter = project;
}

export function takePendingSitesFilter(): string | null {
  const project = pendingProjectFilter;
  pendingProjectFilter = null;
  return project;
}
