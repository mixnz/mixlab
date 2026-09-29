import { describe, expect, it } from "vitest";
import { parseToolsTabState } from "./tabState";

describe("parseToolsTabState", () => {
  it("accepts a toolId that is a non-empty string", () => {
    expect(parseToolsTabState({ toolId: "timestamp" })).toEqual({ toolId: "timestamp" });
  });

  it("returns null for a value that is not an object", () => {
    expect(parseToolsTabState(undefined)).toBeNull();
    expect(parseToolsTabState("timestamp")).toBeNull();
    expect(parseToolsTabState(null)).toBeNull();
    expect(parseToolsTabState(["timestamp"])).toBeNull();
  });

  it("returns null when toolId is missing, empty or not a string", () => {
    expect(parseToolsTabState({})).toBeNull();
    expect(parseToolsTabState({ toolId: "" })).toBeNull();
    expect(parseToolsTabState({ toolId: 7 })).toBeNull();
  });

  it("ignores unknown keys instead of rejecting the whole object", () => {
    expect(parseToolsTabState({ toolId: "jwt", input: "bí mật" })).toEqual({ toolId: "jwt" });
  });
});
