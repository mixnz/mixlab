import { describe, expect, it } from "vitest";
import {
  fileDocuments,
  rememberedDocuments,
  sameRequest,
  type DocumentCache,
  type DocumentRequest,
  type RememberedDocuments,
} from "./request";

const filters: DocumentRequest["filters"] = [];

function request(overrides: Partial<DocumentRequest> = {}): DocumentRequest {
  return {
    connectionId: "c1",
    db: "shop",
    collection: "orders",
    page: 0,
    pageSize: 50,
    filters,
    reloadToken: 0,
    schemaToken: 0,
    ...overrides,
  };
}

describe("sameRequest", () => {
  it("answers a repeat of the same read from what is in hand", () => {
    expect(sameRequest(request(), request())).toBe(true);
  });

  it("owes a read when nothing has been read yet", () => {
    expect(sameRequest(null, request())).toBe(false);
  });

  it.each([
    ["another collection", { collection: "customers" }],
    ["another database", { db: "warehouse" }],
    ["another connection", { connectionId: "c2" }],
    ["another page", { page: 1 }],
    ["another page size", { pageSize: 200 }],
  ])("owes a read for %s", (_name, overrides) => {
    expect(sameRequest(request(), request(overrides))).toBe(false);
  });

  it("owes a read when the reload button has asked for one", () => {
    expect(sameRequest(request(), request({ reloadToken: 1 }))).toBe(false);
  });

  // A collection dropped and made again under the same name is a different collection, and the
  // documents read from the one it replaced must never be handed back for it.
  it("owes a read when the database's shape has changed since", () => {
    expect(sameRequest(request(), request({ schemaToken: 1 }))).toBe(false);
  });

  // Replaced wholesale rather than edited, so identity is the comparison: applying conditions that
  // spell out the same thing is still the user asking to see them run.
  it("owes a read for conditions that are equal but not the same array", () => {
    expect(sameRequest(request({ filters: [] }), request({ filters: [] }))).toBe(false);
  });
});

function remembered(overrides: Partial<DocumentRequest> = {}): RememberedDocuments {
  return {
    documents: [{ _id: "1" }],
    total: 1,
    request: request(overrides),
    scrollTop: 0,
  };
}

describe("rememberedDocuments", () => {
  const key = "shop :: orders";

  it("hands back what was filed for the shape the database has now", () => {
    const cache: DocumentCache = new Map([[key, remembered()]]);
    expect(rememberedDocuments(cache, key, 0)).toBe(cache.get(key));
  });

  it("has nothing for a collection never visited", () => {
    expect(rememberedDocuments(new Map(), key, 0)).toBeUndefined();
  });

  // Emptying the cache when the shape changes does not settle this on its own: the list on screen
  // holds its own copy of the documents and files them straight back on the way out, under the
  // token they were read with. Reading is where the entry has to be turned down.
  it("has nothing filed before the app last changed the database's shape", () => {
    const cache: DocumentCache = new Map([[key, remembered()]]);
    expect(rememberedDocuments(cache, key, 1)).toBeUndefined();
  });
});

describe("fileDocuments", () => {
  /** Fills the cache with `count` collections, oldest first, named `c0` upwards. */
  function filled(count: number): DocumentCache {
    const cache: DocumentCache = new Map();
    for (let i = 0; i < count; i += 1) fileDocuments(cache, `shop :: c${i}`, remembered());
    return cache;
  }

  it("lets the collection left longest ago go once it is full", () => {
    const cache = filled(40);
    expect(cache.size).toBe(20);
    expect(cache.has("shop :: c19")).toBe(false);
    expect(cache.has("shop :: c20")).toBe(true);
    expect(cache.has("shop :: c39")).toBe(true);
  });

  it("puts a collection filed again at the back of the queue", () => {
    const cache = filled(20);
    fileDocuments(cache, "shop :: c0", remembered());
    fileDocuments(cache, "shop :: c20", remembered());
    expect(cache.has("shop :: c0")).toBe(true);
    expect(cache.has("shop :: c1")).toBe(false);
  });
});
