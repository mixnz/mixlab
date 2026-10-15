import { describe, expect, it } from "vitest";
import { formFrom } from "./connectionForm";
import { connectionPlace, connectionString, engineCounts, filterConnections, maskMongoPassword } from "./connectionString";
import type { ConnectionConfig, SavedConnection } from "../types";

function formOf(config: ConnectionConfig) {
  return formFrom(config);
}

describe("connectionString", () => {
  it("writes a server engine as scheme://user@host:port/database", () => {
    const form = formOf({
      kind: "postgres",
      host: "db.example",
      port: 5432,
      username: "app",
      password: "hunter2",
      database: "shop",
    });
    expect(connectionString(form)).toBe("postgresql://app@db.example:5432/shop");
  });

  it("never carries the password", () => {
    const form = formOf({ kind: "mysql", host: "127.0.0.1", port: 3306, username: "root", password: "hunter2" });
    expect(connectionString(form)).not.toContain("hunter2");
    expect(connectionString(form)).toBe("mysql://root@127.0.0.1:3306");
  });

  it("leaves out a user and a database the form does not have", () => {
    const form = formOf({ kind: "redis", host: "cache.local", port: 6379 });
    expect(connectionString(form)).toBe("redis://cache.local:6379");
  });

  it("names SQL Server's scheme as its drivers do", () => {
    const form = formOf({ kind: "mssql", host: "sql", port: 1433, username: "sa", database: "master" });
    expect(connectionString(form)).toBe("sqlserver://sa@sql:1433/master");
  });

  it("is the file's path for SQLite", () => {
    const form = formOf({ kind: "sqlite", host: "", port: 0, path: "C:\\data\\app.db" });
    expect(connectionString(form)).toBe("C:\\data\\app.db");
  });

  it("is the Mongo URI with its password covered", () => {
    const form = formOf({ kind: "mongo", host: "", port: 0, uri: "mongodb://admin:s3cret@mongo:27017/app?authSource=admin" });
    expect(connectionString(form)).toBe("mongodb://admin:***@mongo:27017/app?authSource=admin");
  });
});

describe("maskMongoPassword", () => {
  it("covers the password of an SRV URI", () => {
    expect(maskMongoPassword("mongodb+srv://u:p@cluster.example/db")).toBe("mongodb+srv://u:***@cluster.example/db");
  });

  it("leaves a URI with a user and no password as it is", () => {
    expect(maskMongoPassword("mongodb://reader@mongo/app")).toBe("mongodb://reader@mongo/app");
  });

  it("leaves a URI with no credentials as it is", () => {
    expect(maskMongoPassword("mongodb://localhost:27017")).toBe("mongodb://localhost:27017");
  });
});

function saved(id: string, name: string, kind: ConnectionConfig["kind"]): SavedConnection {
  return { id, name, config: { kind, host: "h", port: 1 } };
}

const list = [saved("1", "Shop production", "mysql"), saved("2", "Cache", "redis"), saved("3", "Shop staging", "mysql")];

describe("filterConnections", () => {
  it("keeps everything when nothing is asked", () => {
    expect(filterConnections(list, { query: "", kinds: new Set() })).toHaveLength(3);
  });

  it("narrows by name, ignoring case", () => {
    expect(filterConnections(list, { query: "shop", kinds: new Set() }).map((c) => c.id)).toEqual(["1", "3"]);
  });

  it("narrows by engine, and by both together", () => {
    expect(filterConnections(list, { query: "", kinds: new Set(["redis"]) }).map((c) => c.id)).toEqual(["2"]);
    expect(filterConnections(list, { query: "staging", kinds: new Set(["mysql"]) }).map((c) => c.id)).toEqual(["3"]);
  });
});

describe("engineCounts", () => {
  it("counts only the engines present, in the order first met", () => {
    expect(engineCounts(list)).toEqual([
      ["mysql", 2],
      ["redis", 1],
    ]);
  });
});

describe("connectionPlace", () => {
  it("is host:port for a server", () => {
    expect(connectionPlace({ kind: "mysql", host: "db.example", port: 3306 })).toBe("db.example:3306");
  });

  it("is the path for SQLite", () => {
    expect(connectionPlace({ kind: "sqlite", host: "", port: 0, path: "/data/app.db" })).toBe("/data/app.db");
  });

  it("is the hosts of a Mongo URI, without its credentials", () => {
    expect(connectionPlace({ kind: "mongo", host: "", port: 0, uri: "mongodb://u:p@a:27017,b:27017/app?rs=x" })).toBe(
      "a:27017,b:27017",
    );
    expect(connectionPlace({ kind: "mongo", host: "", port: 0, uri: "mongodb+srv://cluster.example" })).toBe(
      "cluster.example",
    );
  });
});