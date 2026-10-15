import { beforeEach, describe, expect, it, vi } from "vitest";

/* A writer names what `read` will not return as it was sent, so sync never agrees on it: agreed
   and then left out, it would be pushed as a deletion (T178a, L4). */
vi.mock(import("../saved/savedConnections"), async (original) => ({
  ...(await original()),
  loadSavedConnections: vi.fn(async () => []),
  loadSecrets: vi.fn(async () => ({})),
  saveSecrets: vi.fn(async () => {}),
  deleteSecrets: vi.fn(async () => {}),
}));

vi.mock(import("../saved/savedConnectionsStore"), () => ({
  addConnection: vi.fn(async () => {}),
  updateConnection: vi.fn(async () => {}),
  removeConnection: vi.fn(async () => {}),
  useSavedConnections: vi.fn(),
  useSavedConnectionsLoaded: vi.fn(),
}));

const files = await import("../saved/savedConnections");
const { connectionSecretsSyncable } = await import("./sync");

beforeEach(() => void vi.clearAllMocks());

describe("what sync writes of a connection's credentials", () => {
  it("names those it parked for a connection this machine does not have", async () => {
    const skipped = await connectionSecretsSyncable.write({
      upserts: [{ id: "c-elsewhere", data: { password: "pw" } }],
      removed: [],
    });
    expect(skipped).toEqual(["c-elsewhere"]);
    expect(files.saveSecrets).toHaveBeenCalledWith("c-elsewhere", { password: "pw" });
  });
});
