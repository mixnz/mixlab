/** The only `invoke` calls for removing MixLab: T182a. The Rust side is `src-tauri/src/uninstall/`. */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** One row of the daemon's uninstall plan or report, as the wire spells it (`bindings/Residue.ts`). */
export interface PlanRow {
  id: string;
  what: string;
  location: string;
  outcome: { removal: string; how?: string; by?: string; because?: string; what?: string };
}

export interface Plan {
  items: PlanRow[];
}

/** What a removal that did not end the window says: the report, and what is still on the Mac. */
export interface Outcome {
  report: Plan;
  gone: boolean;
  bundleLeft: boolean;
  receiptLeft: boolean;
  left: string[];
}

export const uninstallPlan = (keepHome: boolean, keepRelocated: boolean) =>
  invoke<Plan>("uninstall_plan", { keepHome, keepRelocated });
/** Ends the process when the removal finished; returns only when the window is still needed. */
export const uninstallRun = (keepHome: boolean, keepRelocated: boolean) =>
  invoke<Outcome>("uninstall_run", { keepHome, keepRelocated });
export const uninstallMenuLabel = (label: string) => invoke<void>("uninstall_menu_label", { label });
/** MixLab ▸ Remove MixLab from this Mac… was chosen. */
export const onUninstallRequest = (callback: () => void): Promise<UnlistenFn> =>
  listen("uninstall://request", () => callback());
