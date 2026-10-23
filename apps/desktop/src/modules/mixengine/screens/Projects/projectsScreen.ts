import type { ProjectDetail } from "@mixengine/api";
import type { SiteAddresses } from "../../siteGroups";

export const NO_ADDRESSES: SiteAddresses = { first: null, byDomain: {} };

/**
 * The project whose row is open, and what has been read of it: a row opens in place, under itself,
 * so what someone just clicked is where they are looking. `detail` is `null` until its read answers;
 * `addresses` are its sites', for the steps panel's `open` rows.
 *
 * One value, so a read can only land on the row it was asked for: opened one after the other, two
 * rows' reads may answer in either order.
 */
export interface OpenProject {
  name: string;
  detail: ProjectDetail | null;
  addresses: SiteAddresses;
}

export function opened(name: string): OpenProject {
  return { name, detail: null, addresses: NO_ADDRESSES };
}

/** `name`'s detail, for the open row if that is still `name`'s. */
export function detailRead(
  open: OpenProject | null,
  name: string,
  detail: ProjectDetail,
  addresses: SiteAddresses,
): OpenProject | null {
  return open?.name === name ? { name, detail, addresses } : open;
}

/** The open row follows the list: a project that is gone — deleted here, by `mix`, or forgotten by
 *  a blueprint that failed — takes its panel with it. */
export function stillListed(open: OpenProject | null, names: string[]): OpenProject | null {
  return open !== null && names.includes(open.name) ? open : null;
}

/** The one dialog on screen. */
export type ProjectsDialog =
  | { kind: "create" }
  | { kind: "edit"; detail: ProjectDetail }
  | { kind: "delete"; name: string };
