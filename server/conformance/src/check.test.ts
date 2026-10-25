import { describe, expect, it } from "vitest";
import { base64Bytes, call, type ErrorBody, signedUp } from "./client.js";

// A move re-registers on another server with the password just typed; this is how it learns the
// password is right before it builds on it. It needs a session, as `/v1/account/delete` does, and
// a wrong answer is counted where a wrong password is counted.

const check = (token: string, a: string) =>
  call<Partial<ErrorBody>>("/v1/account/check", { method: "POST", token, body: { a } });

describe("checking the password", () => {
  it("answers 204 for the account's verifier", async () => {
    const { account, session } = await signedUp();
    const result = await check(session.accessToken, account.a);
    expect(result.status).toBe(204);
  });

  it("answers 401 for any other", async () => {
    const { session } = await signedUp();
    const result = await check(session.accessToken, base64Bytes(32));
    expect(result.status).toBe(401);
    expect(result.body.error?.code).toBe("invalid-credentials");
  });

  it("is not a password on its own", async () => {
    const { account } = await signedUp();
    const result = await check("not-a-token", account.a);
    expect(result.status).toBe(401);
    expect(result.body.error?.code).toBe("invalid-token");
  });

  it("refuses a body with no verifier in it", async () => {
    const { session } = await signedUp();
    const result = await call<ErrorBody>("/v1/account/check", {
      method: "POST",
      token: session.accessToken,
      body: {},
    });
    expect(result.status).toBe(400);
    expect(result.body.error.code).toBe("invalid-request");
  });
});
