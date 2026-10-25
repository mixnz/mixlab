import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SavedTarget } from "./types";

/* The writers are checked for *where* they write: through the shared store, which tells every
   open tab, and never around it into the file alone, which tells nobody until the app restarts. */
const config = { host: "h", port: 22, username: "u", auth: { type: "password", password: "pw" } };
const local = { id: "h1", name: "old", kind: "ssh", config } as SavedTarget;

vi.mock(import("./savedTargets"), async (original) => ({
  ...(await original()),
  loadSavedTargets: vi.fn(async () => [local]),
  loadSecrets: vi.fn(async () => ({})),
  saveSecrets: vi.fn(async () => {}),
  deleteSecrets: vi.fn(async () => {}),
  addSavedTarget: vi.fn(async () => []),
  updateSavedTarget: vi.fn(async () => []),
  removeSavedTarget: vi.fn(async () => []),
}));

vi.mock(import("./savedTargetsStore"), () => ({
  addTarget: vi.fn(async () => {}),
  updateTarget: vi.fn(async () => {}),
  removeTarget: vi.fn(async () => {}),
  useSavedTargets: vi.fn(),
  useSavedTargetsLoaded: vi.fn(),
}));

vi.mock(import("./settingsStore"), async (original) => ({
  ...(await original()),
  saveTerminalSettings: vi.fn(async () => {
    throw new Error("disk full");
  }),
}));

const files = await import("./savedTargets");
const shared = await import("./savedTargetsStore");
const { hostSecretsSyncable, hostsSyncable } = await import("./sync");

beforeEach(() => void vi.clearAllMocks());

describe("what sync writes of a host", () => {
  it("goes through the shared list, so an open tab sees it at once", async () => {
    const renamed = { id: "h1", data: { name: "new", kind: "ssh", config } };
    const arrived = { id: "h2", data: { name: "fresh", kind: "ssh", config } };
    await hostsSyncable.write({ upserts: [renamed, arrived], removed: [] });
    await hostsSyncable.write({ upserts: [], removed: ["h1"] });

    expect(shared.updateTarget).toHaveBeenCalledWith(expect.objectContaining({ id: "h1", name: "new" }));
    expect(shared.addTarget).toHaveBeenCalledWith(expect.objectContaining({ id: "h2" }));
    expect(shared.removeTarget).toHaveBeenCalledWith("h1");
    expect(files.addSavedTarget).not.toHaveBeenCalled();
    expect(files.updateSavedTarget).not.toHaveBeenCalled();
    expect(files.removeSavedTarget).not.toHaveBeenCalled();
  });

  it("names a host's credentials it parked for a host this machine does not have", async () => {
    const skipped = await hostSecretsSyncable.write({
      upserts: [{ id: "h-elsewhere", data: { sshPassword: "pw" } }],
      removed: [],
    });
    // `read` returns credentials only for a host that is here, so agreed they would be pushed as a
    // deletion.
    expect(skipped).toEqual(["h-elsewhere"]);
    expect(files.saveSecrets).toHaveBeenCalledWith("h-elsewhere", { sshPassword: "pw" });
  });

  it("terminal settings whose save fails are a failed write", async () => {
    const { settingsSyncable } = await import("./sync");
    await expect(
      settingsSyncable.write({ upserts: [{ id: "settings", data: { fontSize: 16 } }], removed: [] }),
    ).rejects.toThrow("disk full");
  });

  it("puts a host's credentials through the shared list too", async () => {
    await hostSecretsSyncable.write({ upserts: [{ id: "h1", data: { sshPassword: "new" } }], removed: ["h1"] });
    expect(shared.updateTarget).toHaveBeenCalledTimes(2);
    expect(files.updateSavedTarget).not.toHaveBeenCalled();
  });
});
