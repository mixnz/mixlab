import { beforeEach, describe, expect, it, vi } from "vitest";

/* A launch reads MixLab's own vault and nothing of MixEngine's (ADR 0056): listing saved
   connections, and sync reading them, must never resolve a keyringRef. Only connecting does. */
vi.stubGlobal("window", globalThis);

const saved = [
  { id: "plain", name: "Plain", config: { kind: "mysql", host: "db", port: 3306, username: "u" } },
  {
    id: "engine",
    name: "From MixEngine",
    config: { kind: "mysql", host: "127.0.0.1", port: 3306, username: "root" },
    keyringRef: "mariadb@main/root",
  },
];

vi.mock("@tauri-apps/plugin-store", () => ({
  Store: {
    load: vi.fn(async () => ({
      get: async () => saved,
      set: async () => {},
      save: async () => {},
    })),
  },
}));

const resolved: { value: string | null } = { value: "from-engine" };
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string) => {
    if (command === "secrets_load") return { password: "kept" };
    if (command === "secrets_resolve_mixengine") return resolved.value;
    return undefined;
  }),
}));

const { invoke } = await import("@tauri-apps/api/core");
const { loadSavedConnections, reconnectsOnRestore, withResolvedPassword } = await import("./savedConnections");
const { connectionsSyncable, connectionSecretsSyncable } = await import("./sync");

function resolveCalls(): number {
  return vi.mocked(invoke).mock.calls.filter(([command]) => command === "secrets_resolve_mixengine").length;
}

beforeEach(() => {
  vi.mocked(invoke).mockClear();
  resolved.value = "from-engine";
});

describe("reading the saved connection list", () => {
  it("never asks MixEngine's keyring, and leaves a referenced password as the vault has it", async () => {
    const list = await loadSavedConnections();
    expect(resolveCalls()).toBe(0);
    expect(list.find((c) => c.id === "engine")?.keyringRef).toBe("mariadb@main/root");
  });

  it("is not where sync reaches MixEngine either", async () => {
    await connectionsSyncable.read();
    await connectionSecretsSyncable.read();
    expect(resolveCalls()).toBe(0);
  });
});

describe("withResolvedPassword", () => {
  const config = { kind: "mysql", host: "127.0.0.1", port: 3306, username: "root" } as const;

  it("resolves a reference with an empty password, once", async () => {
    expect((await withResolvedPassword(config, "mariadb@main/root")).password).toBe("from-engine");
    expect(resolveCalls()).toBe(1);
  });

  it("resolves to no password when MixEngine no longer has the entry", async () => {
    resolved.value = null;
    expect((await withResolvedPassword(config, "mariadb@main/root")).password).toBeUndefined();
  });

  it("a typed password wins, and nothing is asked", async () => {
    expect((await withResolvedPassword({ ...config, password: "typed" }, "mariadb@main/root")).password).toBe("typed");
    expect(resolveCalls()).toBe(0);
  });

  it("asks nothing without a reference", async () => {
    expect(await withResolvedPassword(config, null)).toEqual(config);
    expect(resolveCalls()).toBe(0);
  });
});

describe("reconnectsOnRestore", () => {
  it("reconnects an entry whose secrets are in the vault, not one that needs MixEngine's", () => {
    expect(reconnectsOnRestore(saved[0] as never)).toBe(true);
    expect(reconnectsOnRestore(saved[1] as never)).toBe(false);
  });
});
