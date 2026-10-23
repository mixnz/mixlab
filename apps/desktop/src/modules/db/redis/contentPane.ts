/** The panes the content area can show: the selected key's value, or the keys under one group of
 * the sidebar with a way to delete them. The header is a tab strip, so a third one (a command
 * console, server info, …) is a case here and a branch in `RedisWorkspace` rather than a reshuffle
 * of the layout — same as the other two workspaces. */
export type ContentMode = "data" | "group";

/**
 * What the content area holds: the key whose value is open, the group of the sidebar whose keys the
 * delete pane lists (by its prefix), and which of the two tabs is in front. The group tab is only
 * there once a group has been opened, so `mode` is `"group"` only while `group` is set.
 */
export interface ContentPane {
  key: string | null;
  group: string | null;
  mode: ContentMode;
}

export const CONTENT_START: ContentPane = { key: null, group: null, mode: "data" };

export type ContentAction =
  /** Picking a key is a request to see its value, so it brings that pane back to the front — the
   *  delete pane stays open behind its tab rather than swallowing the click. */
  | { type: "selectKey"; name: string }
  | { type: "openGroup"; prefix: string }
  | { type: "closeGroup" }
  | { type: "showTab"; mode: ContentMode }
  /** A key name means nothing outside the database it was read from, and neither does a prefix. */
  | { type: "databaseChanged" }
  | { type: "keysDeleted"; names: string[] };

export function contentPane(pane: ContentPane, action: ContentAction): ContentPane {
  switch (action.type) {
    case "selectKey":
      return { ...pane, key: action.name, mode: "data" };
    case "openGroup":
      return { ...pane, group: action.prefix, mode: "group" };
    case "closeGroup":
      return { ...pane, group: null, mode: "data" };
    case "showTab":
      return action.mode === "group" && pane.group === null ? pane : { ...pane, mode: action.mode };
    case "databaseChanged":
      return CONTENT_START;
    case "keysDeleted":
      return pane.key !== null && action.names.includes(pane.key) ? { ...pane, key: null } : pane;
  }
}
