import type { Enforcement } from "@mixengine/api";

export type EnforcementDisplay = "hard" | "unsupported" | "unavailable" | "advisory";

export function enforcementKind(enforcement: Enforcement): EnforcementDisplay {
  return enforcement.kind;
}

/** Only `unavailable` and `advisory` carry a reason; `null` means there is nothing more to say, not
 *  a place to make up a sentence. */
export function enforcementReason(enforcement: Enforcement): string | null {
  if (enforcement.kind === "unavailable") return enforcement.why;
  if (enforcement.kind === "advisory") return enforcement.why;
  return null;
}
