import { describe, expect, it } from "vitest";
import {
  base64Bytes,
  call,
  type ErrorBody,
  newAccount,
  login,
  newEmail,
  register,
  type Session,
  signedUp,
  verify,
} from "./client.js";

describe("POST /v1/auth/login", () => {
  it("hands back a session a client can use", async () => {
    const { session } = await signedUp();
    expect(session.accessToken).toBeTypeOf("string");
    expect(session.refreshToken).toBeTypeOf("string");
    expect(session.deviceId).toBeTypeOf("string");
    expect(session.expiresIn).toBeGreaterThan(0);
  });

  it("names the account with an id a new registration never reuses", async () => {
    // A move A → B → A registers the same address on A again, under the same MK. A client that
    // told the two accounts apart by address alone kept the first one's cursor.
    const { account, session } = await signedUp();
    expect(session.accountId).toMatch(/^[0-9a-f]{32}$/);
    expect((await login(account)).accountId).toBe(session.accountId);

    const deleted = await call("/v1/account/delete", {
      method: "POST",
      token: session.accessToken,
      body: { a: account.a },
    });
    expect(deleted.status).toBe(200);
    const again = newAccount({ email: account.email });
    expect((await register(again)).status).toBe(201);
    await verify(again);
    expect((await login(again)).accountId).not.toBe(session.accountId);
  });

  it("refuses the wrong verifier", async () => {
    const account = newAccount();
    await register(account);
    await verify(account);

    const result = await call<ErrorBody>("/v1/auth/login", {
      body: { email: account.email, a: base64Bytes(32), deviceName: "conformance" },
    });
    expect(result.status).toBe(401);
    expect(result.body.error.code).toBe("invalid-credentials");
  });

  it("answers an unknown address exactly as it answers a wrong verifier", async () => {
    // Registration has to refuse a taken address and therefore leaks one. This route has no such
    // obligation, so it does not leak: same status, same code, for both.
    const result = await call<ErrorBody>("/v1/auth/login", {
      body: { email: newEmail(), a: base64Bytes(32), deviceName: "conformance" },
    });
    expect(result.status).toBe(401);
    expect(result.body.error.code).toBe("invalid-credentials");
  });

  it("signs in whatever the casing of the address", async () => {
    const account = newAccount();
    await register(account);
    await verify(account);

    const result = await call<Session>("/v1/auth/login", {
      body: { email: account.email.toUpperCase(), a: account.a, deviceName: "conformance" },
    });
    expect(result.status).toBe(200);
  });

  it("gives a second machine a session of its own", async () => {
    const account = newAccount();
    await register(account);
    await verify(account);

    const first = await call<Session>("/v1/auth/login", {
      body: { email: account.email, a: account.a, deviceName: "laptop" },
    });
    const second = await call<Session>("/v1/auth/login", {
      body: { email: account.email, a: account.a, deviceName: "desktop" },
    });

    expect(first.status).toBe(200);
    expect(second.status).toBe(200);
    expect(second.body.deviceId).not.toBe(first.body.deviceId);
    expect(second.body.refreshToken).not.toBe(first.body.refreshToken);
  });
});
