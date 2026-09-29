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
  it("đọc URI MySQL đầy đủ", () => {
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

  it("nhận cả postgres:// và postgresql://", () => {
    expect(parseConnectionString("postgres://h/d")?.kind).toBe("postgres");
    expect(parseConnectionString("postgresql://h/d")?.kind).toBe("postgres");
  });

  it("nhận rediss:// và đọc số database ở phần path", () => {
    const f = parseConnectionString("rediss://:pw@127.0.0.1:6379/2");
    expect(f?.kind).toBe("redis");
    expect(f?.user).toBe("");
    expect(f?.password).toBe("pw");
    expect(f?.database).toBe("2");
  });

  // The DNS SRV record is what states the port, so a `+srv` URI carrying a port is a wrong URI.
  it("để cổng trống với mongodb+srv", () => {
    const f = parseConnectionString("mongodb+srv://u:p@cluster.example.com/app");
    expect(f?.kind).toBe("mongodb");
    expect(f?.srv).toBe(true);
    expect(f?.port).toBe("");
  });

  // The silent failure: `URL` returns the username and password percent-encoded.
  it("decode mật khẩu đã percent-encode", () => {
    expect(parseConnectionString("mysql://u:p%40ss@h/d")?.password).toBe("p@ss");
  });

  it("không vỡ vì chuỗi phần trăm hỏng", () => {
    expect(parseConnectionString("mysql://u:p%zz@h/d")?.password).toBe("p%zz");
  });

  it("trả null cho scheme lạ và cho chuỗi không phải URI", () => {
    expect(parseConnectionString("ftp://h/d")).toBeNull();
    expect(parseConnectionString("chỉ là chữ")).toBeNull();
  });
});

describe("toUri", () => {
  it("ghép lại đầy đủ", () => {
    expect(toUri({ ...base, password: "pw", params: [{ key: "ssl", value: "true" }] })).toBe(
      "mysql://an:pw@db.example.com:3306/shop?ssl=true",
    );
  });

  it("bỏ phần xác thực khi không có user lẫn mật khẩu", () => {
    expect(toUri({ ...base, user: "", password: "" })).toBe("mysql://db.example.com:3306/shop");
  });

  it("giữ dạng chỉ có mật khẩu của Redis", () => {
    expect(
      toUri({ ...base, kind: "redis", user: "", password: "pw", port: "6379", database: "0" }),
    ).toBe("redis://:pw@db.example.com:6379/0");
  });

  it("dùng scheme mongodb+srv và bỏ cổng khi srv", () => {
    expect(toUri({ ...base, kind: "mongodb", srv: true, port: "", password: "pw" })).toBe(
      "mongodb+srv://an:pw@db.example.com/shop",
    );
  });

  // Without encoding these three characters the string cannot be parsed anywhere.
  it("encode mật khẩu có ký tự phá cú pháp", () => {
    const uri = toUri({ ...base, password: "a/b?c#d" });
    expect(uri).toContain("a%2Fb%3Fc%23d");
  });

  it("đi vòng tròn với mật khẩu chứa cả năm ký tự khó", () => {
    const password = "p@ss:w/o?rd#1";
    const back = parseConnectionString(toUri({ ...base, password }));
    expect(back?.password).toBe(password);
    expect(back?.host).toBe("db.example.com");
    expect(back?.port).toBe("3306");
  });
});

describe("toJdbc", () => {
  it("in chuỗi JDBC cho MySQL", () => {
    expect(toJdbc({ ...base, password: "pw" })).toBe(
      "jdbc:mysql://db.example.com:3306/shop?user=an&password=pw",
    );
  });

  it("in chuỗi JDBC cho PostgreSQL", () => {
    expect(toJdbc({ ...base, kind: "postgres", port: "5432", password: "pw" })).toBe(
      "jdbc:postgresql://db.example.com:5432/shop?user=an&password=pw",
    );
  });

  it("bù cổng mặc định khi ô cổng để trống", () => {
    expect(toJdbc({ ...base, port: "", password: "pw" })).toContain("db.example.com:3306");
  });

  // There is no JDBC standard for these two kinds, and printing a valid-looking string hands the
  // user something that will break somewhere else.
  it("trả null cho MongoDB và Redis", () => {
    expect(toJdbc({ ...base, kind: "mongodb" })).toBeNull();
    expect(toJdbc({ ...base, kind: "redis" })).toBeNull();
  });
});

describe("toEnvPairs", () => {
  it("dựng năm biến DB_*", () => {
    expect(toEnvPairs({ ...base, password: "pw" })).toEqual([
      { key: "DB_HOST", value: "db.example.com" },
      { key: "DB_PORT", value: "3306" },
      { key: "DB_USER", value: "an" },
      { key: "DB_PASSWORD", value: "pw" },
      { key: "DB_NAME", value: "shop" },
    ]);
  });

  it("bù cổng mặc định theo loại DB", () => {
    const pairs = toEnvPairs({ ...base, kind: "mongodb", port: "" });
    expect(pairs.find((p) => p.key === "DB_PORT")?.value).toBe("27017");
  });
});
