import type {
  CatalogueGap,
  PackageRelease,
  RuntimeRelease,
  RuntimeSummary,
  RuntimeUpdate,
  UpgradePlan,
} from "@mixengine/api";
import type { OnDiskRow } from "../../onDisk";
import type { AskingStep } from "../../requirementStep";
import type { DevkitState } from "./devkit";

/** What is installed, read from this machine. */
export interface LocalRead {
  installed: RuntimeSummary[];
  onDisk: OnDiskRow[];
}

/** What the package index offers, which may wait on its network. */
export interface CatalogueRead {
  available: RuntimeRelease[];
  stale: boolean;
  unavailable: CatalogueGap[];
  /** What the index has newer in each installed version's line. */
  updates: RuntimeUpdate[];
}

export const EMPTY_LOCAL: LocalRead = { installed: [], onDisk: [] };
export const EMPTY_CATALOGUE: CatalogueRead = { available: [], stale: false, unavailable: [], updates: [] };

/** The devkit a Windows Ruby needs to build native gems. `offer` is what the catalogue has and
 *  whether it says one is installed; `here` is read from `package.list`, where a devkit recorded
 *  with its folder gone can be told from none. Each is set by its own read. */
export interface Devkit {
  offer: { release: PackageRelease | null; installed: boolean };
  here: DevkitState;
}

export const NO_DEVKIT: Devkit = { offer: { release: null, installed: false }, here: { state: "absent" } };

/** The one question the screen is asking, with what it is about. */
export type LanguagesDialog =
  /** `forceHint` is the daemon's refusal, naming the project that pins it, once it asked. */
  | { kind: "uninstall"; target: RuntimeSummary; forceHint: string | null }
  /** What this machine lacks for an install. */
  | { kind: "requirements"; release: RuntimeRelease; step: AskingStep }
  | { kind: "upgradePlan"; update: RuntimeUpdate; plan: UpgradePlan }
  /** What the new version lacks, once the plan was agreed. */
  | { kind: "upgradeRequirements"; update: RuntimeUpdate; keep: boolean; step: AskingStep }
  | { kind: "removeDevkit"; version: string };

/** A read that has answered: what it read, or, when it failed, what an earlier one said — or
 *  nothing, the first time. `null` is "not read yet", and an empty list is not that. */
export function readAnswered<T>(current: T | null, empty: T): T {
  return current ?? empty;
}

/** The uninstall question asked again with `force`, showing the daemon's own sentence. */
export function forceAsked(dialog: LanguagesDialog | null, hint: string): LanguagesDialog | null {
  return dialog?.kind === "uninstall" ? { ...dialog, forceHint: hint } : dialog;
}
