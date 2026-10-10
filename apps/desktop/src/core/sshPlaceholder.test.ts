import { describe, expect, it, vi } from "vitest";

vi.mock("./platform", () => ({ IS_WINDOWS: true, IS_MAC: false }));

describe("the private key placeholder on Windows", () => {
  // A bare "\U" or "\." in a string literal is just the letter, so the path once lost every
  // separator and read `C:Usersyou.sshid_ed25519` in the form.
  it("keeps its backslashes", async () => {
    const { PRIVATE_KEY_PLACEHOLDER } = await import("./ssh");
    expect(PRIVATE_KEY_PLACEHOLDER).toBe(String.raw`C:\Users\you\.ssh\id_rsa`);
  });
});
