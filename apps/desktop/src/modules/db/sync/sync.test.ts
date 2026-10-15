import { describe, expect, it } from "vitest";
import {
  connectionFromSync,
  connectionSecretsFromSync,
  connectionSecretsToSync,
  connectionToSync,
} from "./sync";
import type { SavedConnection } from "../types";

const local: SavedConnection = {
  id: "c1",
  name: "Production",
  config: { kind: "postgres", host: "db", port: 5432, username: "app", password: "hunter2" } as never,
  sidebarWidth: 280,
  pinned: true,
  readOnly: true,
};

describe("what travels of a connection", () => {
  it("is what it is, never a credential or a layout", () => {
    const synced = connectionToSync(local);
    expect(synced.id).toBe("c1");
    expect(JSON.stringify(synced.data)).not.toContain("hunter2");
    expect(synced.data).not.toHaveProperty("sidebarWidth");
    expect(synced.data).not.toHaveProperty("pinned");
    expect(synced.data).toMatchObject({ name: "Production", readOnly: true });
  });

  it("keeps this machine's password and layout when another machine renames it", () => {
    const { data } = connectionToSync(local);
    const renamed = connectionFromSync({ id: "c1", data: { ...(data as object), name: "Prod" } }, local);
    expect(renamed?.name).toBe("Prod");
    expect(renamed?.config).toMatchObject({ password: "hunter2" });
    expect(renamed?.sidebarWidth).toBe(280);
    expect(renamed?.pinned).toBe(true);
  });

  it("arrives without a password when this machine never had it", () => {
    const { data } = connectionToSync(local);
    const arrived = connectionFromSync({ id: "new", data }, undefined);
    expect(arrived?.config).not.toHaveProperty("password", "hunter2");
    expect(arrived?.sidebarWidth).toBeUndefined();
  });

  it("refuses what it cannot read", () => {
    expect(connectionFromSync({ id: "x", data: { name: 1 } }, undefined)).toBeNull();
    expect(connectionFromSync({ id: "x", data: { name: "x" } }, undefined)).toBeNull();
  });
});

describe("what travels of a connection's credentials", () => {
  it("is the four fields D5 names, and nothing for a connection with none", () => {
    const withSsh: SavedConnection = {
      ...local,
      id: "c2",
      config: {
        ...local.config,
        ssh: { host: "b", port: 22, user: "u", auth: { type: "password", password: "tunnel" } },
      } as never,
    };
    const bare: SavedConnection = { ...local, id: "c3", config: { ...local.config, password: undefined } as never };
    expect(connectionSecretsToSync([local, withSsh, bare])).toEqual([
      { id: "c1", data: { password: "hunter2" } },
      { id: "c2", data: { password: "hunter2", sshPassword: "tunnel" } },
    ]);
  });

  it("is never a password MixEngine's keyring holds", () => {
    const keyed: SavedConnection = { ...local, keyringRef: "mixengine:db" };
    expect(connectionSecretsToSync([keyed])).toEqual([]);
  });

  it("arrives as those four fields, strings only", () => {
    expect(
      connectionSecretsFromSync({ password: "p", uri: "", sshPassphrase: 7, token: "x", sshPassword: "s" }),
    ).toEqual({ password: "p", sshPassword: "s" });
    expect(connectionSecretsFromSync("nope")).toBeNull();
  });
});

describe("a connection's SSH key path", () => {
  const tunnelled = (key_path: string): SavedConnection => ({
    ...local,
    config: {
      ...local.config,
      ssh: { host: "bastion", port: 22, username: "u", auth: { type: "privatekey", key_path } },
    } as never,
  });

  it("travels home-relative, so the same key opens on macOS and Windows", () => {
    const { data } = connectionToSync(tunnelled("C:\\Users\\haiqu\\.ssh\\id_rsa"));
    expect(JSON.stringify(data)).toContain('"key_path":"~/.ssh/id_rsa"');
  });

  it("stays as this machine had it when the other machine's could not travel", () => {
    const synced = connectionToSync(tunnelled("/opt/keys/id"));
    const arrived = connectionFromSync(synced, tunnelled("D:\\keys\\id"));
    expect(arrived?.config.ssh?.auth).toMatchObject({ key_path: "D:\\keys\\id" });
  });
});
