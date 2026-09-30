import { describe, expect, it, vi } from "vitest";
import { preferencesCollection, type PreferenceStorage } from "./preferencesSync";

function memory(initial: Record<string, string> = {}): PreferenceStorage & { data: Map<string, string> } {
  const data = new Map(Object.entries(initial));
  return {
    data,
    getItem: (key) => data.get(key) ?? null,
    setItem: (key, value) => void data.set(key, value),
    removeItem: (key) => void data.delete(key),
  };
}

describe("preferences in sync", () => {
  it("are the four keys that are set, and nothing else in storage", async () => {
    const storage = memory({ "mixlab-theme": "dark", "mixlab-lang": "vi", "mixlab-session": "{}" });
    const items = await preferencesCollection(() => storage, () => {}).read();
    expect(items).toEqual([
      { id: "theme", data: "dark" },
      { id: "language", data: "vi" },
    ]);
  });

  it("write through, forget what was removed, and say so once", async () => {
    const storage = memory({ "mixlab-accent": "red" });
    const announce = vi.fn();
    await preferencesCollection(() => storage, announce).write({
      upserts: [{ id: "theme", data: "light" }],
      removed: ["accent"],
    });
    expect(storage.data.get("mixlab-theme")).toBe("light");
    expect(storage.data.has("mixlab-accent")).toBe(false);
    expect(announce).toHaveBeenCalledOnce();
  });

  it("ignore a key it does not know and a value that is not a string, name both, and say nothing", async () => {
    const storage = memory();
    const announce = vi.fn();
    const skipped = await preferencesCollection(() => storage, announce).write({
      upserts: [{ id: "mixlab-session", data: "{}" }, { id: "theme", data: 3 }],
      removed: [],
    });
    expect(skipped).toEqual(["mixlab-session", "theme"]);
    expect(storage.data.size).toBe(0);
    expect(announce).not.toHaveBeenCalled();
  });
});
