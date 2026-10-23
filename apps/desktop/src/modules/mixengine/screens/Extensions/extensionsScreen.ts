import type { ExtensionOffer, ExtensionOrigin, ExtensionSummary, ServiceSummary, SiteSummary } from "@mixengine/api";

/** Everything one read of the screen answers. */
export interface ExtensionLists {
  installed: ExtensionSummary[];
  available: ExtensionOffer[];
  sites: SiteSummary[];
  /** Registry entries this build could not read. */
  unreadable: number;
  stale: boolean;
  /** Every service, for why an add-on's process could not start. */
  allServices: ServiceSummary[];
}

export const EMPTY_LISTS: ExtensionLists = {
  installed: [],
  available: [],
  sites: [],
  unreadable: 0,
  stale: false,
  allServices: [],
};

/** A read that has answered: what it read, or, when it failed, what an earlier one said — or
 *  nothing, the first time. `null` is "not read yet": both lists being empty then is not "nothing
 *  installed" or "an empty registry". */
export function listsAnswered(current: ExtensionLists | null): ExtensionLists {
  return current ?? EMPTY_LISTS;
}

/** The one dialog on screen: the plan of an install, or the uninstall question with its box. */
export type ExtensionsDialog =
  | { kind: "install"; source: ExtensionOrigin }
  | { kind: "uninstall"; row: ExtensionSummary; deleteData: boolean };

export function deleteDataTicked(dialog: ExtensionsDialog | null, deleteData: boolean): ExtensionsDialog | null {
  return dialog?.kind === "uninstall" ? { ...dialog, deleteData } : dialog;
}
