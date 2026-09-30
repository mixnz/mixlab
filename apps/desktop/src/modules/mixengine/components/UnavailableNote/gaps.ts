import type { CatalogueGap } from "@mixengine/api";

/**
 * What a list says about the kinds the package index could not be read for (T196): their names
 * for the pill, and each one's reason for its tooltip. `null` when there is nothing to say —
 * everything was read, or the daemon is from before the field existed.
 */
export function describeGaps(
  gaps: CatalogueGap[] | null | undefined,
): { names: string; title: string } | null {
  if (!gaps || gaps.length === 0) return null;
  return {
    names: gaps.map((gap) => gap.name).join(", "),
    title: gaps.map((gap) => `${gap.name}: ${gap.reason}`).join("\n"),
  };
}
