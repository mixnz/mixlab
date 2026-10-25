import { beforeEach, describe, expect, it, vi } from "vitest";

/* What sync agrees on must be on disk: a write whose save fails has to say so, or
   sync records a hash the disk never held and later pushes the old copy back as an edit. */
vi.stubGlobal("window", globalThis);

const disk = { fail: false };
vi.mock("@tauri-apps/plugin-store", () => ({
  Store: {
    load: vi.fn(async () => ({
      get: async () => null,
      set: async () => {},
      save: async () => {
        if (disk.fail) throw new Error("disk full");
      },
    })),
  },
}));

vi.mock(import("./api"), async (original) => ({
  ...(await original()),
  envSecretsLoad: vi.fn(async () => ({})),
  envSecretsSave: vi.fn(async () => {}),
  envSecretsDelete: vi.fn(async () => {}),
}));

const { requestsSyncable, environmentsSyncable } = await import("./sync");

const request = {
  id: "r1",
  data: { name: "n", method: "GET", url: "https://x", params: [], headers: [], body: { kind: "none" } },
};

beforeEach(() => {
  disk.fail = false;
});

describe("what sync writes of the REST module", () => {
  it("a request list whose save fails is a failed write", async () => {
    disk.fail = true;
    await expect(requestsSyncable.write({ upserts: [request], removed: [] })).rejects.toThrow("disk full");
  });

  it("a request list resolves once it is saved", async () => {
    await expect(requestsSyncable.write({ upserts: [request], removed: [] })).resolves.toEqual([]);
  });

  it("an environment list whose save fails is a failed write", async () => {
    disk.fail = true;
    await expect(
      environmentsSyncable.write({ upserts: [{ id: "e1", data: { name: "dev", vars: [] } }], removed: [] }),
    ).rejects.toThrow("disk full");
  });
});
