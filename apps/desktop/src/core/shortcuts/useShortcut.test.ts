import { describe, expect, it } from "vitest";
import { enabledKeyOf } from "./useShortcut";

const noop = () => {};

/* What `useShortcuts` registers again on: the ids that are on, in the order they were given. */
describe("enabledKeyOf", () => {
  it("keeps the enabled ids, in order", () => {
    expect(
      enabledKeyOf([
        { id: "a", handler: noop, enabled: true },
        { id: "b", handler: noop, enabled: false },
        { id: "c", handler: noop, enabled: true },
      ]),
    ).toBe("a\nc");
  });

  it("is empty when nothing is on", () => {
    expect(enabledKeyOf([{ id: "a", handler: noop, enabled: false }])).toBe("");
    expect(enabledKeyOf([])).toBe("");
  });
});
