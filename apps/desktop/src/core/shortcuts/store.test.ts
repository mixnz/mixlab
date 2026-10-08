import { describe, expect, it } from "vitest";
import { enterModal, modalDepth } from "./store";

describe("modalDepth", () => {
  it("counts a dialog while it is up, and stops counting when it goes", () => {
    const before = modalDepth();
    const leave = enterModal();
    expect(modalDepth()).toBe(before + 1);
    leave();
    leave();
    expect(modalDepth()).toBe(before);
  });

  /* A dialog belongs to the tab that opened it: one in a tab out of sight does not hold the
     keyboard of the tab on screen. */
  it("does not count a dialog that is out of sight", () => {
    const before = modalDepth();
    let visible = false;
    const leave = enterModal(() => visible);
    expect(modalDepth()).toBe(before);
    visible = true;
    expect(modalDepth()).toBe(before + 1);
    leave();
    expect(modalDepth()).toBe(before);
  });
});
