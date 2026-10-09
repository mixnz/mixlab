import type { RuntimeSummary } from "@mixengine/api";
import { describe, expect, it } from "vitest";
import { createPinRegistry } from "./pinRegistry";

const php = (version: string, isDefault = false): RuntimeSummary => ({
  kind: "php",
  version,
  channel: "stable",
  path: `/home/runtimes/php/${version}`,
  installed_at: 0,
  bytes: 1,
  default: isDefault,
});
const RUNTIMES = [php("8.4.26", true), php("8.1.34"), php("8.1.9")];

describe("pinRegistry", () => {
  it("lists what is installed, filtered by kind", () => {
    const registry = createPinRegistry(RUNTIMES);
    expect(registry.installed().runtimes).toHaveLength(3);
    expect(registry.installed("node").runtimes).toEqual([]);
  });

  it("starts with no pins", () => {
    expect(createPinRegistry(RUNTIMES).pins("legacy")).toEqual([]);
  });

  it("resolves a prefix to the newest installed version that agrees with it", () => {
    const registry = createPinRegistry(RUNTIMES);
    registry.replace("legacy", { php: "8.1" });
    expect(registry.pins("legacy")).toEqual([
      { kind: "php", constraint: "8.1", source: { from: "registered" }, resolved: "8.1.34", hint: null },
    ]);
  });

  it("resolves an exact version to itself and leaves other projects alone", () => {
    const registry = createPinRegistry(RUNTIMES);
    registry.replace("legacy", { php: "8.1.34" });
    expect(registry.pins("legacy")[0].resolved).toBe("8.1.34");
    expect(registry.pins("blog")).toEqual([]);
  });

  it("replaces rather than merges, and says when nothing installed matches", () => {
    const registry = createPinRegistry(RUNTIMES);
    registry.replace("legacy", { php: "8.1" });
    registry.replace("legacy", { php: "7.4" });
    const [pin] = registry.pins("legacy");
    expect(pin.constraint).toBe("7.4");
    expect(pin.resolved).toBeNull();
    expect(pin.hint).toBe("mix runtime available");
  });
});
