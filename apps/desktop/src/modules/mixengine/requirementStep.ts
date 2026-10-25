import type { Need, RedistributableArch, Requirement } from "@mixengine/api";

/**
 * What an install has to ask before it starts.
 *
 * Pure, so the rule is tested without a window. The daemon decided every remedy; this only decides
 * which one question the screen asks: none, "install it too?", or "install this version instead?".
 */
export type RequirementStep =
  | { readonly kind: "proceed" }
  | { readonly kind: "notice"; readonly needs: readonly Need[] }
  | {
      readonly kind: "consent";
      readonly arches: readonly RedistributableArch[];
      readonly needs: readonly Need[];
    }
  | { readonly kind: "choose"; readonly version: string | null; readonly needs: readonly Need[] };

/** The two steps that need a dialog: consent, or a choice that has somewhere to go. */
export type AskingStep =
  | Extract<RequirementStep, { kind: "consent" }>
  | (Extract<RequirementStep, { kind: "choose" }> & { readonly version: string });

/**
 * The one question `unmet` raises.
 *
 * **A lack no installer fixes wins**: agreeing to install the Visual C++ runtime would not make a
 * glibc newer, so the screen never asks for that approval when it would not be enough.
 */
export function requirementStep(unmet: readonly Requirement[]): RequirementStep {
  if (unmet.length === 0) return { kind: "proceed" };

  const needs = unmet.map((requirement) => requirement.need);

  // A library the distribution provides is said, not asked about. It never outranks the two
  // steps below: a plan that also needs consent asks for it, and one that is blocked says so.
  const actionable = unmet.filter(
    (requirement) => requirement.remedy.remedy !== "install_from_distribution",
  );
  if (actionable.length === 0) return { kind: "notice", needs };

  const blocking = actionable.filter(
    (requirement) => requirement.remedy.remedy !== "install_visual_cpp",
  );

  if (blocking.length > 0) {
    // A choice is only worth offering when every blocking need points at the same way out.
    const allChoose = blocking.every((requirement) => requirement.remedy.remedy === "choose_version");
    const first = blocking[0].remedy;
    const version = allChoose && first.remedy === "choose_version" ? first.version : null;
    return { kind: "choose", version, needs };
  }

  const arches = [
    ...new Set(
      actionable.flatMap((requirement) =>
        requirement.remedy.remedy === "install_visual_cpp" ? [requirement.remedy.arch] : [],
      ),
    ),
  ].sort();

  return { kind: "consent", arches, needs };
}

/** A step that needs a dialog, or `null` for one that does not — `proceed`, or a `choose` with nowhere to go. */
export function askingStep(step: RequirementStep): AskingStep | null {
  if (step.kind === "consent") return step;
  if (step.kind === "choose" && step.version !== null) return { ...step, version: step.version };
  return null;
}

/** The few words a table cell shows for one need. Product names, so the same in every language. */
export function needLabel(need: Need): string {
  switch (need.need) {
    case "glibc":
      return `glibc ${need.at_least}+`;
    case "macos":
      return `macOS ${need.at_least}+`;
    case "visual_cpp":
      return `Visual C++ ${need.year} (${need.arch})`;
    case "cpu":
      return `CPU with ${need.feature.toUpperCase()}`;
    case "shared_library":
      return need.soname;
  }
}

/**
 * A row's needs, with every shared library gathered apart.
 *
 * A Linux JDK names eight sonames, which is more than a table cell holds; the screen shows how many
 * there are and puts the names in the cell's title.
 */
export function splitLibraries(needs: readonly Need[]): {
  readonly others: string[];
  readonly libraries: string[];
} {
  const others: string[] = [];
  const libraries: string[] = [];

  for (const need of needs) {
    if (need.need === "shared_library") libraries.push(need.soname);
    else others.push(needLabel(need));
  }

  return { others, libraries };
}

/** Whether a blueprint's apply may be sent, given what its releases lack and what was agreed. */
export function requirementsAllowApply(unmet: readonly Requirement[], agreed: boolean): boolean {
  const step = requirementStep(unmet);
  if (step.kind === "proceed" || step.kind === "notice") return true;
  if (step.kind === "consent") return agreed;
  return false;
}
