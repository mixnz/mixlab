import type { ProjectPin } from "@mixengine/api";
import type { RuntimeKind } from "@mixengine/api";

/**
 * A `ProjectPin` as something drawable — `resolvedVersion` is not inferred from `constraint` on the
 * client side; only what the daemon computed is read back.
 */
export interface FormattedPin {
  kind: RuntimeKind;
  constraint: string;
  sourceLabel: "manifest" | "row";
  sourcePath?: string;
  resolvedVersion?: string;
  hint?: string;
}

export function formatPins(pins: ProjectPin[]): FormattedPin[] {
  return pins.map((pin) => ({
    kind: pin.kind,
    constraint: pin.constraint,
    sourceLabel: pin.source.from === "manifest" ? "manifest" : "row",
    sourcePath: pin.source.from === "manifest" ? pin.source.path : undefined,
    resolvedVersion: pin.resolved ?? undefined,
    hint: pin.hint ?? undefined,
  }));
}
