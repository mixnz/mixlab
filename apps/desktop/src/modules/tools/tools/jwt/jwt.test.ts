import { describe, expect, it } from "vitest";
import { claimTimes, decodeJwt } from "./jwt";

// header {"alg":"HS256","typ":"JWT"}, payload {"sub":"1","exp":1756339200}, a fake signature.
const TOKEN =
  "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9" +
  ".eyJzdWIiOiIxIiwiZXhwIjoxNzU2MzM5MjAwfQ" +
  ".c2lnbmF0dXJl";

describe("decodeJwt", () => {
  it("splits the three parts and reads the header and payload", () => {
    const result = decodeJwt(TOKEN);
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.parts.header).toEqual({ alg: "HS256", typ: "JWT" });
    expect(result.parts.payload).toEqual({ sub: "1", exp: 1756339200 });
    expect(result.parts.signature).toBe("c2lnbmF0dXJl");
  });

  it("refuses a token without exactly three parts", () => {
    expect(decodeJwt("a.b")).toEqual({ ok: false, reason: "shape" });
    expect(decodeJwt("a.b.c.d")).toEqual({ ok: false, reason: "shape" });
    expect(decodeJwt("")).toEqual({ ok: false, reason: "shape" });
  });

  it("tells broken base64 apart from broken JSON", () => {
    expect(decodeJwt("!!!.eyJhIjoxfQ.sig")).toEqual({ ok: false, reason: "base64" });
    // "bm90IGpzb24" decodes to "not json" — valid base64, invalid JSON.
    expect(decodeJwt("bm90IGpzb24.eyJhIjoxfQ.sig")).toEqual({ ok: false, reason: "json" });
  });

  it("ignores surrounding whitespace, which always sticks along when copying", () => {
    expect(decodeJwt(`  ${TOKEN}\n`).ok).toBe(true);
  });
});

describe("claimTimes", () => {
  it("says whether the token has expired, compared with the `now` passed in", () => {
    expect(claimTimes({ exp: 1000 }, 2000 * 1000).expired).toBe(true);
    expect(claimTimes({ exp: 1000 }, 500 * 1000).expired).toBe(false);
  });

  it("returns null for `expired` when the payload has no exp", () => {
    expect(claimTimes({ sub: "1" }, 0).expired).toBeNull();
  });

  it("picks up iat and nbf too, ignoring keys that are not numbers", () => {
    expect(claimTimes({ exp: 3, iat: 1, nbf: "hai" }, 0)).toMatchObject({
      exp: 3,
      iat: 1,
      nbf: undefined,
    });
  });

  it("does not fall over on a payload that is not an object", () => {
    expect(claimTimes("chuỗi", 0).expired).toBeNull();
    expect(claimTimes(null, 0).expired).toBeNull();
  });
});
