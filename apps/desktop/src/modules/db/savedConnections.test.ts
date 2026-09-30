import { describe, expect, it } from "vitest";
import { readSecrets } from "./savedConnections";
import type { ConnectionConfig } from "./types";

const config: ConnectionConfig = {
  kind: "mysql",
  host: "127.0.0.1",
  port: 3306,
  username: "root",
  password: "hunter2",
  database: "blog",
};

describe("readSecrets", () => {
  it("carries a typed-in password as before, with no keyringRef", () => {
    expect(readSecrets(config)).toEqual({ password: "hunter2" });
  });

  /* The point of a keyringRef: Save must not go on writing a copy of MixEngine's password into
     MixLab's own credential store just because `config.password` still holds the value resolved
     for display. */
  it("leaves the password out when a keyringRef is given", () => {
    expect(readSecrets(config, "mariadb@main/root")).toEqual({});
  });

  it("still carries the connection string and SSH secrets alongside a keyringRef", () => {
    const withExtras: ConnectionConfig = {
      ...config,
      uri: "mongodb://u:p@h/db",
      ssh: {
        host: "bastion",
        port: 22,
        username: "deploy",
        auth: { type: "password", password: "s3cret" },
      },
    };
    expect(readSecrets(withExtras, "mariadb@main/root")).toEqual({
      uri: "mongodb://u:p@h/db",
      sshPassword: "s3cret",
    });
  });
});
