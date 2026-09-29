/**
 * What a Tools tab remembers between two app runs: which tool is open.
 *
 * The id and only the id. The contents of the input and output fields never pass through here —
 * this is `localStorage`, and people paste tokens and connection strings with passwords into these
 * tools all the time.
 */
export interface ToolsTabState {
  toolId: string;
}

/**
 * The saved value, if it is one, or `null`.
 *
 * Checks the shape and only the shape. Whether `toolId` is still in the registry is `ToolsTab`'s
 * question at mount: a tool removed between two app runs is not an error to report.
 */
export function parseToolsTabState(value: unknown): ToolsTabState | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const state = value as Record<string, unknown>;
  if (typeof state.toolId !== "string" || state.toolId === "") return null;
  return { toolId: state.toolId };
}
