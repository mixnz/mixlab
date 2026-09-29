import { describe, expect, it } from "vitest";
import { claimTimes, decodeJwt } from "./jwt";

// header {"alg":"HS256","typ":"JWT"}, payload {"sub":"1","exp":1756339200}, a fake signature.
const TOKEN =
  "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9" +
  ".eyJzdWIiOiIxIiwiZXhwIjoxNzU2MzM5MjAwfQ" +
  ".c2lnbmF0dXJl";

describe("decodeJwt", () => {
  it("tách ba phần và đọc header với payload", () => {
    const result = decodeJwt(TOKEN);
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.parts.header).toEqual({ alg: "HS256", typ: "JWT" });
    expect(result.parts.payload).toEqual({ sub: "1", exp: 1756339200 });
    expect(result.parts.signature).toBe("c2lnbmF0dXJl");
  });

  it("từ chối token không có đúng ba phần", () => {
    expect(decodeJwt("a.b")).toEqual({ ok: false, reason: "shape" });
    expect(decodeJwt("a.b.c.d")).toEqual({ ok: false, reason: "shape" });
    expect(decodeJwt("")).toEqual({ ok: false, reason: "shape" });
  });

  it("phân biệt base64 hỏng với JSON hỏng", () => {
    expect(decodeJwt("!!!.eyJhIjoxfQ.sig")).toEqual({ ok: false, reason: "base64" });
    // "bm90IGpzb24" decodes to "not json" — valid base64, invalid JSON.
    expect(decodeJwt("bm90IGpzb24.eyJhIjoxfQ.sig")).toEqual({ ok: false, reason: "json" });
  });

  it("bỏ qua khoảng trắng hai đầu, thứ luôn dính theo khi chép", () => {
    expect(decodeJwt(`  ${TOKEN}\n`).ok).toBe(true);
  });
});

describe("claimTimes", () => {
  it("nói token đã hết hạn hay chưa, so với `now` truyền vào", () => {
    expect(claimTimes({ exp: 1000 }, 2000 * 1000).expired).toBe(true);
    expect(claimTimes({ exp: 1000 }, 500 * 1000).expired).toBe(false);
  });

  it("trả null cho `expired` khi payload không có exp", () => {
    expect(claimTimes({ sub: "1" }, 0).expired).toBeNull();
  });

  it("nhặt cả iat và nbf, bỏ qua khoá không phải số", () => {
    expect(claimTimes({ exp: 3, iat: 1, nbf: "hai" }, 0)).toMatchObject({
      exp: 3,
      iat: 1,
      nbf: undefined,
    });
  });

  it("không ngã với payload không phải object", () => {
    expect(claimTimes("chuỗi", 0).expired).toBeNull();
    expect(claimTimes(null, 0).expired).toBeNull();
  });
});
