import { describe, expect, it } from "vitest";

import type { SavedConnection } from "../types";
import { connectionsToFollow, followed } from "./followPort";

function saved(id: string, port: number, keyringRef?: string): SavedConnection {
  return {
    id,
    name: id,
    config: { kind: "mysql", host: "127.0.0.1", port, username: "root" },
    keyringRef,
  };
}

describe("connectionsToFollow", () => {
  it("finds the connections MixEngine made for that service, still on the old port", () => {
    const list = [saved("a", 3307, "mysql@8/root"), saved("b", 3307, "mysql@8/app")];

    expect(connectionsToFollow(list, "mysql@8", 3307).map((one) => one.id)).toEqual(["a", "b"]);
  });

  it("does not take another instance whose name starts the same way", () => {
    expect(connectionsToFollow([saved("c", 3307, "mysql@80/root")], "mysql@8", 3307)).toEqual([]);
  });

  it("leaves a connection typed by hand alone", () => {
    expect(connectionsToFollow([saved("d", 3307)], "mysql@8", 3307)).toEqual([]);
  });

  it("leaves one already on another port alone", () => {
    expect(connectionsToFollow([saved("e", 3306, "mysql@8/root")], "mysql@8", 3307)).toEqual([]);
  });
});

describe("followed", () => {
  it("changes the port and nothing else", () => {
    const before = saved("a", 3307, "mysql@8/root");
    const after = followed(before, 3306);

    expect(after).toEqual({ ...before, config: { ...before.config, port: 3306 } });
    expect(before.config.port).toBe(3307);
  });
});
