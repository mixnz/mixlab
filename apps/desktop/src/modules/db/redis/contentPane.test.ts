import { describe, expect, it } from "vitest";
import { CONTENT_START, contentPane, type ContentPane } from "./contentPane";

describe("contentPane", () => {
  it("opens a group on its own tab, and picking a key brings the value back to the front", () => {
    const group = contentPane(CONTENT_START, { type: "openGroup", prefix: "user" });
    expect(group).toEqual({ key: null, group: "user", mode: "group" });
    expect(contentPane(group, { type: "selectKey", name: "user:1" })).toEqual({ key: "user:1", group: "user", mode: "data" });
  });

  it("never shows the group tab without a group", () => {
    expect(contentPane(CONTENT_START, { type: "showTab", mode: "group" })).toEqual(CONTENT_START);
    const open: ContentPane = { key: "a", group: "user", mode: "data" };
    expect(contentPane(open, { type: "showTab", mode: "group" }).mode).toBe("group");
  });

  it("closes the group and its tab together", () => {
    const open: ContentPane = { key: "a", group: "user", mode: "group" };
    expect(contentPane(open, { type: "closeGroup" })).toEqual({ key: "a", group: null, mode: "data" });
  });

  it("lets go of the key and the group when the database changes", () => {
    const open: ContentPane = { key: "a", group: "user", mode: "group" };
    expect(contentPane(open, { type: "databaseChanged" })).toEqual(CONTENT_START);
  });

  it("lets go of the key only when it was one of those deleted", () => {
    const open: ContentPane = { key: "a", group: "user", mode: "group" };
    expect(contentPane(open, { type: "keysDeleted", names: ["b"] })).toBe(open);
    expect(contentPane(open, { type: "keysDeleted", names: ["a"] })).toEqual({ key: null, group: "user", mode: "group" });
  });
});
