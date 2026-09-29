import { describe, expect, it } from "vitest";
import {
  parseConnectionString,
  toEnvPairs,
  toJdbc,
  toUri,
  type ConnectionFields,
} from "./connection";

const base: ConnectionFields = {
  kind: "mysql",
  srv: false,
  host: "db.example.com",
  port: "3306",
  user: "an",
  password: "bí mật",
  database: "shop",
  params: [],
};

describe("parseConnectionString", () => {
  it("reads a full MySQL URI", () => {
    expect(parseConnectionString("mysql://an:pw@db:3306/shop?ssl=true")).toEqual({
      kind: "mysql",
      srv: false,
      host: "db",
      port: "3306",
      user: "an",
      password: "pw",
      database: "shop",
      params: [{ key: "ssl", value: "true" }],
    });
  });

  it("accepts both postgres:// and postgresql://", () => {
    expect(parseConnectionString("postgres://h/d")?.kind).toBe("postgres");
    expect(parseConnectionString("postgresql://h/d")?.kind).toBe("postgres");
  });

  it("accepts rediss:// and reads the database number from the path", () => {
    const f = parseConnectionString("rediss://:pw@127.0.0.1:6379/2");
    expect(f?.kind).toBe("redis");
    expect(f?.user).toBe("");
    expect(f?.password).toBe("pw");
    expect(f?.database).toBe("2");
  });

  // The DNS SRV record is what states the port, so a `+srv` URI carrying a port is a wrong URI.
  it("leaves the port empty for mongodb+srv", () => {
    const f = parseConnectionString("mongodb+srv://u:p@cluster.example.com/app");
    expect(f?.kind).toBe("mongodb");
    expect(f?.srv).toBe(true);
    expect(f?.port).toBe("");
  });

  // The silent failure: `URL` returns the username and password percent-encoded.
  it("decodes a percent-encoded password", () => {
    expect(parseConnectionString("mysql://u:p%40ss@h/d")?.password).toBe("p@ss");
  });

  it("does not break on a malformed percent sequence", () => {
    expect(parseConnectionString("mysql://u:p%zz@h/d")?.password).toBe("p%zz");
  });

  it("returns null for an unknown scheme and for a string that is not a URI", () => {
    expect(parseConnectionString("ftp://h/d")).toBeNull();
    expect(parseConnectionString("chỉ là chữ")).toBeNull();
  });
});

describe("toUri", () => {
  it("builds the full string back", () => {
    expect(toUri({ ...base, password: "pw", params: [{ key: "ssl", value: "true" }] })).toBe(
      "mysql://an:pw@db.example.com:3306/shop?ssl=true",
    );
  });

  it("drops the credentials part when there is neither user nor password", () => {
    expect(toUri({ ...base, user: "", password: "" })).toBe("mysql://db.example.com:3306/shop");
  });

  it("keeps Redis's password-only form", () => {
    expect(
      toUri({ ...base, kind: "redis", user: "", password: "pw", port: "6379", database: "0" }),
    ).toBe("redis://:pw@db.example.com:6379/0");
  });

  it("uses the mongodb+srv scheme and drops the port with srv", () => {
    expect(toUri({ ...base, kind: "mongodb", srv: true, port: "", password: "pw" })).toBe(
      "mongodb+srv://an:pw@db.example.com/shop",
    );
  });

  // Without encoding these three characters the string cannot be parsed anywhere.
  it("encodes a password with syntax-breaking characters", () => {
    const uri = toUri({ ...base, password: "a/b?c#d" });
    expect(uri).toContain("a%2Fb%3Fc%23d");
  });

  it("round-trips a password containing all five tricky characters", () => {
    const password = "p@ss:w/o?rd#1";
    const back = parseConnectionString(toUri({ ...base, password }));
    expect(back?.password).toBe(password);
    expect(back?.host).toBe("db.example.com");
    expect(back?.port).toBe("3306");
  });
});

describe("toJdbc", () => {
  it("prints a JDBC string for MySQL", () => {
    expect(toJdbc({ ...base, password: "pw" })).toBe(
      "jdbc:mysql://db.example.com:3306/shop?user=an&password=pw",
    );
  });

  it("prints a JDBC string for PostgreSQL", () => {
    expect(toJdbc({ ...base, kind: "postgres", port: "5432", password: "pw" })).toBe(
      "jdbc:postgresql://db.example.com:5432/shop?user=an&password=pw",
    );
  });

  it("fills in the default port when the port field is empty", () => {
    expect(toJdbc({ ...base, port: "", password: "pw" })).toContain("db.example.com:3306");
  });

  // There is no JDBC standard for these two kinds, and printing a valid-looking string hands the
  // user something that will break somewhere else.
  it("returns null for MongoDB and Redis", () => {
    expect(toJdbc({ ...base, kind: "mongodb" })).toBeNull();
    expect(toJdbc({ ...base, kind: "redis" })).toBeNull();
  });
});

describe("toEnvPairs", () => {
  it("builds the five DB_* variables", () => {
    expect(toEnvPairs({ ...base, password: "pw" })).toEqual([
      { key: "DB_HOST", value: "db.example.com" },
      { key: "DB_PORT", value: "3306" },
      { key: "DB_USER", value: "an" },
      { key: "DB_PASSWORD", value: "pw" },
      { key: "DB_NAME", value: "shop" },
    ]);
  });

  it("fills in the default port by DB kind", () => {
    const pairs = toEnvPairs({ ...base, kind: "mongodb", port: "" });
    expect(pairs.find((p) => p.key === "DB_PORT")?.value).toBe("27017");
  });
});
