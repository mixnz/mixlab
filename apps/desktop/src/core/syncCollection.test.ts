import { describe, expect, it } from "vitest";
import { applySyncChanges, asRecord, type SyncItem } from "./syncCollection";

interface Local {
  id: string;
  name: string;
  /** Never travels. */
  width?: number;
}

const idOf = (item: Local) => item.id;
const fromSync = (synced: SyncItem, local: Local | undefined): Local | null => {
  const data = asRecord(synced.data);
  if (!data || typeof data.name !== "string") return null;
  return { ...local, id: synced.id, name: data.name };
};

describe("applying another machine's changes", () => {
  it("replaces in place, appends what is new, and drops what was removed", () => {
    const current: Local[] = [
      { id: "a", name: "A" },
      { id: "b", name: "B" },
      { id: "c", name: "C" },
    ];
    const { items: next } = applySyncChanges(
      current,
      { upserts: [{ id: "b", data: { name: "B2" } }, { id: "d", data: { name: "D" } }], removed: ["c"] },
      idOf,
      fromSync,
    );
    expect(next.map((item) => `${item.id}:${item.name}`)).toEqual(["a:A", "b:B2", "d:D"]);
  });

  it("keeps what never travels", () => {
    const { items: next } = applySyncChanges(
      [{ id: "a", name: "A", width: 320 }],
      { upserts: [{ id: "a", data: { name: "A2" } }], removed: [] },
      idOf,
      fromSync,
    );
    expect(next[0]).toEqual({ id: "a", name: "A2", width: 320 });
  });

  it("leaves an item alone rather than overwrite it with data it cannot read, and names it", () => {
    const { items, skipped } = applySyncChanges(
      [{ id: "a", name: "A" }],
      { upserts: [{ id: "a", data: { name: 7 } }, { id: "b", data: { name: "B" } }], removed: [] },
      idOf,
      fromSync,
    );
    expect(items).toEqual([
      { id: "a", name: "A" },
      { id: "b", name: "B" },
    ]);
    // Named so sync does not agree on it: agreed, and then left out by `read`, it would be pushed
    // as a deletion.
    expect(skipped).toEqual(["a"]);
  });

  it("does not touch the list it was given", () => {
    const current: Local[] = [{ id: "a", name: "A" }];
    applySyncChanges(current, { upserts: [], removed: ["a"] }, idOf, fromSync);
    expect(current).toEqual([{ id: "a", name: "A" }]);
  });
});

describe("asRecord", () => {
  it("is a plain object or nothing", () => {
    expect(asRecord({ a: 1 })).toEqual({ a: 1 });
    expect(asRecord(null)).toBeNull();
    expect(asRecord([1])).toBeNull();
    expect(asRecord("x")).toBeNull();
  });
});
