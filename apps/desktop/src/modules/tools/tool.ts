import type { ComponentType } from "react";
import type { TranslationKey } from "../../i18n";

/** The sidebar's five sections, in the order they appear. */
export type ToolGroup = "data" | "encode" | "time" | "infra" | "text";

export const TOOL_GROUPS: ToolGroup[] = ["data", "encode", "time", "infra", "text"];

/**
 * One tool in the module.
 *
 * `Panel` takes no props, and that is deliberate: a tool does not need to know which tab it is in
 * or whether the tab is showing — it is one input and one output. A contract this small is what
 * keeps adding the 15th tool down to one file plus one line in `registry.ts`.
 */
export interface ToolDefinition {
  id: string;
  labelKey: TranslationKey;
  group: ToolGroup;
  Panel: ComponentType;
}
