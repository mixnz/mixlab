import { describe, expect, it } from "vitest";
/* The split and the merge moved to `core/ssh.ts` — the db module tunnels through the same
   servers and was doing the same thing. These tests came with them unchanged, which is what
   says the move changed no behaviour. */
import { mergeSshSecrets as mergeSecrets, splitSshSecrets as splitSecrets } from "../../core/ssh";
import { parseSavedTarget, withoutSecrets } from "./savedTargets";
import type { SavedTarget, SshConfig } from "./types";

const withPassword: SshConfig = {
  host: "example.com",
  port: 22,
  username: "deploy",
  auth: { type: "password", password: "hunter2" },
};

const withKey: SshConfig = {
  host: "example.com",
  port: 2222,
  username: "deploy",
  auth: { type: "privatekey", key_path: "/home/me/.ssh/id_ed25519", passphrase: "let me in" },
};

describe("splitSecrets", () => {
  it("takes the password out of what goes to the file", () => {
    const { config, secrets } = splitSecrets(withPassword);
    expect(secrets).toEqual({ sshPassword: "hunter2" });
    expect(config.auth).toEqual({ type: "password", password: "" });
  });

  it("keeps the key's path and takes only the passphrase", () => {
    const { config, secrets } = splitSecrets(withKey);
    expect(secrets).toEqual({ sshPassphrase: "let me in" });
    expect(config.auth).toEqual({
      type: "privatekey",
      key_path: "/home/me/.ssh/id_ed25519",
      passphrase: undefined,
    });
  });

  /* A key with no passphrase must not leave an empty entry behind: `secrets_save` deletes the
     entry when handed nothing, which is exactly what should happen. */
  it("makes no secret out of nothing to hide", () => {
    const noPassphrase: SshConfig = {
      ...withKey,
      auth: { type: "privatekey", key_path: "/k", passphrase: "" },
    };
    expect(splitSecrets(noPassphrase).secrets).toEqual({});
  });

  it("leaves the host, the port and the user alone", () => {
    const { config } = splitSecrets(withKey);
    expect(config.host).toBe("example.com");
    expect(config.port).toBe(2222);
    expect(config.username).toBe("deploy");
  });
});

describe("mergeSecrets", () => {
  it("puts the password back where it came from", () => {
    const { config, secrets } = splitSecrets(withPassword);
    expect(mergeSecrets(config, secrets)).toEqual(withPassword);
  });

  it("puts the passphrase back where it came from", () => {
    const { config, secrets } = splitSecrets(withKey);
    expect(mergeSecrets(config, secrets)).toEqual(withKey);
  });

  /* Someone cleared the entry out of Credential Manager, or copied `terminal-hosts.json` to
     another machine: the host still has to open in the form, only with an empty password box. */
  it("leaves the box empty when the credential store has nothing", () => {
    const { config } = splitSecrets(withPassword);
    expect(mergeSecrets(config, {})).toEqual({
      ...withPassword,
      auth: { type: "password", password: "" },
    });
  });
});

describe("parseSavedTarget", () => {
  it("reads a shell on this machine", () => {
    expect(
      parseSavedTarget({
        id: "t-1",
        name: "frontend",
        kind: "local",
        shellName: "wsl:Ubuntu",
        cwd: "D:\work",
        runOnConnect: "cd ~/a",
      }),
    ).toEqual({
      id: "t-1",
      name: "frontend",
      kind: "local",
      shellName: "wsl:Ubuntu",
      cwd: "D:\work",
      runOnConnect: "cd ~/a",
    });
  });

  /* The same rule as `tabState.ts`: no starting directory means opening in the shell's default
     directory. */
  it("reads an absent working directory as none", () => {
    const entry = parseSavedTarget({ id: "t-1", name: "a", kind: "local", shellName: "pwsh" });
    expect(entry).toEqual({
      id: "t-1",
      name: "a",
      kind: "local",
      shellName: "pwsh",
      cwd: null,
      runOnConnect: undefined,
    });
  });

  /* An entry from the old version, when the list only held servers. Nothing is guessed: there was
     no other kind back then, so `ssh` is what it always was. */
  it("reads an entry written before there were kinds as a server", () => {
    const entry = parseSavedTarget({ id: "h-1", name: "prod", config: withPassword });
    expect(entry).toEqual({
      id: "h-1",
      name: "prod",
      kind: "ssh",
      config: withPassword,
      runOnConnect: undefined,
    });
  });

  it("gives up on anything it cannot draw a row for", () => {
    expect(parseSavedTarget(null)).toBeNull();
    expect(parseSavedTarget("t-1")).toBeNull();
    expect(parseSavedTarget([])).toBeNull();
    expect(parseSavedTarget({ name: "a", config: withPassword })).toBeNull();
    expect(parseSavedTarget({ id: "", name: "a", config: withPassword })).toBeNull();
    expect(parseSavedTarget({ id: "t-1", config: withPassword })).toBeNull();
    // A server without `config` has nothing to connect to.
    expect(parseSavedTarget({ id: "t-1", name: "a", kind: "ssh" })).toBeNull();
    // A shell without a name cannot be found again in the machine's shell list.
    expect(parseSavedTarget({ id: "t-1", name: "a", kind: "local" })).toBeNull();
    // A kind added by a later version: this version cannot draw it.
    expect(parseSavedTarget({ id: "t-1", name: "a", kind: "serial" })).toBeNull();
    // Without an address there is no row to draw, even though everything else is there.
    expect(
      parseSavedTarget({ id: "t-1", name: "a", config: { port: 22, username: "u", auth: withPassword.auth } }),
    ).toBeNull();
  });

  /* This used to be an `as SshConfig`, i.e. no check at all. A hand-edited entry missing `auth`
     slipped through, then `mergeSecrets` read `config.auth.type` and threw in the middle of
     `loadSavedTargets`' `Promise.all` — the whole list empty for the entire session because of one
     row. */
  it("reads a hand-edited server without dropping the list it is in", () => {
    const entry = parseSavedTarget({ id: "t-1", name: "prod", config: { host: "example.com" } });
    expect(entry).toEqual({
      id: "t-1",
      name: "prod",
      kind: "ssh",
      config: { host: "example.com", port: 22, username: "", auth: { type: "password", password: "" } },
      runOnConnect: undefined,
    });
    // And what was just read passes through `mergeSecrets`, which used to throw.
    expect(() => mergeSecrets((entry as { config: SshConfig }).config, {})).not.toThrow();
  });

  it("falls back to the ssh port on one it cannot believe", () => {
    const port = (value: unknown) =>
      (parseSavedTarget({ id: "t-1", name: "a", config: { ...withPassword, port: value } }) as {
        config: SshConfig;
      }).config.port;
    expect(port(2222)).toBe(2222);
    expect(port("2222")).toBe(22);
    expect(port(0)).toBe(22);
    expect(port(70000)).toBe(22);
    expect(port(22.5)).toBe(22);
  });

  /** A private key reads as a private key, even when the path has been removed from the file. */
  it("keeps a key entry a key entry", () => {
    const entry = parseSavedTarget({
      id: "t-1",
      name: "a",
      config: { host: "h", port: 22, username: "u", auth: { type: "privatekey" } },
    }) as { config: SshConfig };
    expect(entry.config.auth).toEqual({ type: "privatekey", key_path: "", passphrase: undefined });
  });
});

describe("withoutSecrets", () => {
  it("takes the password out of a server on its way to the file", () => {
    const target: SavedTarget = { id: "h-1", name: "prod", kind: "ssh", config: withPassword };
    const stored = withoutSecrets(target);
    expect(stored.kind === "ssh" && stored.config.auth).toEqual({ type: "password", password: "" });
  });

  /* A shell on this machine has nothing to hide, so it takes no detour at all — and
     `savedTargets.ts` does not call the keyring for it either. */
  it("passes a local shell through untouched", () => {
    const target: SavedTarget = {
      id: "t-1",
      name: "frontend",
      kind: "local",
      shellName: "pwsh",
      cwd: null,
      runOnConnect: "npm run dev",
    };
    expect(withoutSecrets(target)).toBe(target);
  });
});
