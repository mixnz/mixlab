import { describe, expect, it } from "vitest";
import { arrivesConnected } from "./handoffArrival";
import type { ConnectionConfig } from "../types";

const mysql = (rest: Partial<ConnectionConfig>): ConnectionConfig => ({
  kind: "mysql",
  host: "127.0.0.1",
  port: 3306,
  username: "root",
  ...rest,
});

describe("arrivesConnected", () => {
  /* What MixEngine hands over: the password came in the environment, so there is nothing to ask. */
  it("dials at once when the password came along", () => {
    expect(arrivesConnected(mysql({ password: "s3cret" }))).toBe(true);
    expect(arrivesConnected({ ...mysql({ password: "s3cret" }), kind: "postgres" })).toBe(true);
  });

  /* A Redis MixEngine manages has no accounts; there is no password to wait for. */
  it("dials a Redis with nothing to sign in as", () => {
    expect(arrivesConnected({ kind: "redis", host: "127.0.0.1", port: 6379 })).toBe(true);
  });

  /* Nor has a MongoDB MixEngine manages, which arrives as one connection string. */
  it("dials a MongoDB with nothing to sign in as", () => {
    expect(
      arrivesConnected({
        kind: "mongo",
        host: "127.0.0.1",
        port: 27017,
        uri: "mongodb://127.0.0.1:27017/?directConnection=true",
      }),
    ).toBe(true);
  });

  /* A `mixlab://` link from a browser: the same URL, no environment behind it. Dialling `root`
     with an empty password would only fail, so the form waits for the one thing it lacks. */
  it("waits for a password a server with accounts was not given", () => {
    expect(arrivesConnected(mysql({}))).toBe(false);
    expect(arrivesConnected(mysql({ password: "" }))).toBe(false);
    expect(arrivesConnected({ ...mysql({}), kind: "postgres" })).toBe(false);
  });
});
