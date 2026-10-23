import { describe, expect, it } from "vitest";
import type { RedisKeyInfo } from "./api";
import { SCAN_IDLE, scanReducer, type Scan } from "./scanState";

const key = (name: string): RedisKeyInfo => ({ name, type: "string" });

describe("scanReducer", () => {
  it("starts a sweep from nothing, with no page of an older sweep still counted", () => {
    const busy: Scan = { keys: [key("a")], cursor: "9", done: true, loading: false, loadingMore: true };
    expect(scanReducer(busy, { type: "started" })).toEqual({ keys: [], cursor: "0", done: false, loading: true, loadingMore: false });
  });

  it("shows what a sweep has so far, and stops loading when it ends", () => {
    const shown = scanReducer(scanReducer(SCAN_IDLE, { type: "started" }), {
      type: "published",
      keys: [key("a"), key("b")],
      cursor: "4",
      done: false,
    });
    expect(shown).toMatchObject({ keys: [key("a"), key("b")], cursor: "4", done: false, loading: true });
    expect(scanReducer(shown, { type: "finished" }).loading).toBe(false);
  });

  it("appends one more page without a name twice", () => {
    const before: Scan = { keys: [key("a"), key("b")], cursor: "4", done: false, loading: false, loadingMore: false };
    const asked = scanReducer(before, { type: "moreAsked" });
    expect(asked.loadingMore).toBe(true);
    const landed = scanReducer(asked, { type: "moreLanded", keys: [key("b"), key("c")], cursor: "0", done: true });
    expect(landed.keys.map((k) => k.name)).toEqual(["a", "b", "c"]);
    expect([landed.cursor, landed.done, landed.loadingMore]).toEqual(["0", true, true]);
    expect(scanReducer(landed, { type: "moreEnded" }).loadingMore).toBe(false);
  });

  it("drops deleted keys and keeps the rest of what was read", () => {
    const before: Scan = { keys: [key("a"), key("b"), key("c")], cursor: "4", done: false, loading: false, loadingMore: false };
    expect(scanReducer(before, { type: "keysDeleted", names: ["b"] }).keys.map((k) => k.name)).toEqual(["a", "c"]);
  });
});
