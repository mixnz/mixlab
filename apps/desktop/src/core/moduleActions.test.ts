import { describe, expect, it } from "vitest";
import { callModuleAction, provideModuleActions } from "./moduleActions";

describe("module actions", () => {
  it("calls what a module lent, by its id and the action's name, and hands back its answer", async () => {
    provideModuleActions("echo", { say: async (payload) => ({ said: payload }) });
    await expect(callModuleAction("echo", "say", "hi")).resolves.toEqual({ said: "hi" });
  });

  it("refuses, naming both, an action nobody lent", async () => {
    await expect(callModuleAction("nobody", "save", {})).rejects.toThrow(/nobody.*save/);
  });
});
