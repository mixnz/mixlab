import { createStore, jsonFile, useStore } from "../../core/jsonStore";

/**
 * The Tools module's choices that persist between sessions.
 *
 * Only furniture, not data: the time zone a person works in is the same in every tab, and picking
 * it again on every app launch is pointless. **The contents of the input and output fields are
 * still never saved** — people paste tokens and connection strings with passwords into these
 * tools.
 */
export interface ToolsWorkspace {
  /** IANA's current name — see `tools/timestamp/zones.ts`. `null` means use the machine's zone. */
  timeZone: string | null;
}

const DEFAULTS: ToolsWorkspace = { timeZone: null };

/* Spread over the defaults rather than replacing them: a file written by an old version is still
   the user's choice, and a field it has never heard of takes the default value. */
const file = jsonFile<Partial<ToolsWorkspace>>("tools-workspace.json", "workspace", {});
const store = createStore<ToolsWorkspace>({
  defaults: DEFAULTS,
  load: async () => ({ ...DEFAULTS, ...(await file.load()) }),
  persist: file.persist,
});

/** Writes quietly in the background. Nothing here is worth an error message in front of the user:
 *  a time zone that did not make it to disk is a time zone back at the default on the next
 *  launch. */
function write(next: ToolsWorkspace): void {
  void store.save(next).catch(() => {});
}

export function useToolsWorkspace(): ToolsWorkspace {
  return useStore(store);
}

export function setTimeZone(timeZone: string | null): void {
  write({ ...store.get(), timeZone });
}
