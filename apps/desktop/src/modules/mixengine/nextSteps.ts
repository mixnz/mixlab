import type { BlueprintApplied, NextStep } from "@mixengine/api";
import { failedSteps } from "./blueprintPlan";

/**
 * The module the steps hand commands to — the second id this module names, beside `db`
 * (`openChoices.ts`). A bridge between two modules is one naming the other.
 */
export const TERMINAL_MODULE_ID = "terminal";

/** A step is needed when the site cannot answer without it (spec D4's `optional`). */
function needed(step: NextStep): boolean {
  return step.kind !== "open" && !step.optional;
}

/** D7: the browser opens by itself when no step failed and no step is needed. */
export function opensByItself(applied: BlueprintApplied): boolean {
  if (failedSteps(applied).length > 0) return false;
  return !(applied.next_steps?.steps ?? []).some(needed);
}

/** D7: the site's address, plus the first `open` step's path. */
export function openAddress(siteUrl: string, steps: NextStep[]): string {
  const path = steps.find((step) => step.kind === "open" && step.path)?.path;
  if (!path) return siteUrl;
  return `${siteUrl.replace(/\/+$/, "")}${path}`;
}

/** D8: required `once` lines, then the first required `serve`; further required serves apart. */
export function requiredRuns(steps: NextStep[]): { first: string[]; furtherServes: string[] } {
  const required = steps.filter((step) => needed(step) && step.run);
  const once = required.filter((step) => step.kind === "once").map((step) => step.run as string);
  const serves = required.filter((step) => step.kind === "serve").map((step) => step.run as string);
  return { first: [...once, ...serves.slice(0, 1)], furtherServes: serves.slice(1) };
}

/** Where the project's runtimes are: `path.status`'s directory and the daemon's home (D9). */
export interface Toolchain {
  bin: string;
  home: string;
}

function environment(toolchain: Toolchain) {
  return { env: { MIXENGINE_HOME: toolchain.home }, pathPrepend: [toolchain.bin] };
}

/** The Terminal's one-shot local state (Task 12's contract). `shellName: ""` is the default. */
export function oneShotState(lines: string[], root: string, toolchain: Toolchain, press: boolean): unknown {
  return { kind: "local", shellName: "", cwd: root, ...environment(toolchain), run: lines, press };
}

/**
 * The target *Save as Terminal target* asks the Terminal module to save for one `serve` step
 * (D11): its `saveTarget` action, through `core/moduleActions.ts`. Saved there and then, with no
 * tab opened over what the person is reading. `onRestore: "type"` whatever the trust: a tab that
 * comes back with MixLab types the server's line and waits, so a launch never starts one by itself.
 */
export function targetToSave(project: string, root: string, serve: string, toolchain: Toolchain): unknown {
  return {
    name: `${project} · ${serve}`,
    shellName: "",
    cwd: root,
    ...environment(toolchain),
    runOnConnect: serve,
    onRestore: "type",
  };
}
