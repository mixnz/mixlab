import { describe, expect, it } from "vitest";
import { parseTerminalTabState, tabStateFor } from "./tabState";
import type { LocalShell, SshConfig } from "./types";

const SHELL: LocalShell = { name: "wsl:Ubuntu", path: "wsl.exe", args: ["-d", "Ubuntu"] };
const CONFIG: SshConfig = {
  host: "192.168.50.86",
  port: 22,
  username: "demo",
  auth: { type: "password", password: "demo" },
};

describe("parseTerminalTabState", () => {
  it("reads the ssh branch back", () => {
    expect(parseTerminalTabState({ kind: "ssh", targetId: "t-1" })).toEqual({
      kind: "ssh",
      targetId: "t-1",
    });
  });

  /* State written by the previous version, when the list only held servers and the id was called
     `hostId`. A tab open while the user upgrades does not lose where it was. */
  it("reads the previous session's id, from when it was still called hostId", () => {
    expect(parseTerminalTabState({ kind: "ssh", hostId: "h-1" })).toEqual({
      kind: "ssh",
      targetId: "h-1",
    });
    expect(parseTerminalTabState({ kind: "local", shellName: "pwsh", hostId: "h-1" })).toEqual({
      kind: "local",
      shellName: "pwsh",
      cwd: null,
      targetId: "h-1",
    });
  });

  it("reads the local branch back", () => {
    expect(parseTerminalTabState({ kind: "local", shellName: "pwsh", cwd: "C:\\src" })).toEqual({
      kind: "local",
      shellName: "pwsh",
      cwd: "C:\\src",
      targetId: undefined,
    });
  });

  /* `targetId` is additional: the shell and directory are still what reopen the tab, while the id
     is only for looking up the startup command. So a shell nobody has saved as a row can still be
     remembered. */
  it("keeps the saved target's id alongside the shell, not instead of it", () => {
    expect(
      parseTerminalTabState({ kind: "local", shellName: "pwsh", cwd: null, targetId: "t-1" }),
    ).toEqual({ kind: "local", shellName: "pwsh", cwd: null, targetId: "t-1" });
  });

  it("accepts a shell with no starting directory, however it is written", () => {
    // Compares the whole object rather than `?.cwd`: `TerminalTabState` is a union, and the `ssh`
    // branch has no `cwd`.
    const expected = { kind: "local", shellName: "pwsh", cwd: null, targetId: undefined };
    expect(parseTerminalTabState({ kind: "local", shellName: "pwsh", cwd: null })).toEqual(expected);
    expect(parseTerminalTabState({ kind: "local", shellName: "pwsh" })).toEqual(expected);
  });

  it("says nothing about a tab that never wrote anything", () => {
    expect(parseTerminalTabState(undefined)).toBeNull();
  });

  /* Everything below is a string some version of the app wrote into `localStorage`, so nothing is
     trusted — the shell deliberately passes it through without looking. */
  it("ignores anything that is not a terminal tab's state", () => {
    expect(parseTerminalTabState(null)).toBeNull();
    expect(parseTerminalTabState("ssh")).toBeNull();
    expect(parseTerminalTabState([])).toBeNull();
    expect(parseTerminalTabState({})).toBeNull();
    expect(parseTerminalTabState({ kind: "telnet", targetId: "t-1" })).toBeNull();
    expect(parseTerminalTabState({ kind: "ssh" })).toBeNull();
    expect(parseTerminalTabState({ kind: "ssh", targetId: "" })).toBeNull();
    expect(parseTerminalTabState({ kind: "ssh", targetId: 7 })).toBeNull();
    expect(parseTerminalTabState({ kind: "local" })).toBeNull();
    expect(parseTerminalTabState({ kind: "local", shellName: "pwsh", cwd: 7 })).toBeNull();
  });

  /* `""` is the machine's default shell — T205: the launch queue that hands a tab over cannot know
     this machine's shell names. */
  it("reads an empty shell name as the default shell", () => {
    expect(parseTerminalTabState({ kind: "local", shellName: "" })).toEqual({
      kind: "local",
      shellName: "",
      cwd: null,
      targetId: undefined,
    });
  });

  /* T205, D8: a tab another module hands over runs its lines once and is a plain shell after. */
  it("reads a one-shot local state with its run lines", () => {
    expect(
      parseTerminalTabState({
        kind: "local", shellName: "", cwd: "/p", env: { A: "1" }, pathPrepend: ["/b"],
        run: ["npm install", "npm run dev"], press: false,
      }),
    ).toEqual({
      kind: "local", shellName: "", cwd: "/p", targetId: undefined, env: { A: "1" },
      pathPrepend: ["/b"], run: ["npm install", "npm run dev"], press: false,
    });
  });

  it("never writes run back", () => {
    const state = tabStateFor({
      kind: "local", shell: { name: "pwsh", path: "pwsh", args: [] }, cwd: "/p", targetId: null,
      runOnConnect: "npm run dev", press: true, env: { A: "1" }, pathPrepend: ["/b"],
    });
    expect(state).toEqual({ kind: "local", shellName: "pwsh", cwd: "/p", targetId: undefined, env: { A: "1" }, pathPrepend: ["/b"] });
  });

  /* An unreadable id does not break the whole state: the shell and directory can still reopen the
     tab, there is just no entry to look up the startup command in. */
  it("drops a broken id on the local branch but still keeps the shell", () => {
    expect(parseTerminalTabState({ kind: "local", shellName: "pwsh", targetId: 7 })).toEqual({
      kind: "local",
      shellName: "pwsh",
      cwd: null,
      targetId: undefined,
    });
  });
});

describe("tabStateFor", () => {
  it("keeps the shell name and starting directory, not the path", () => {
    expect(
      tabStateFor({
        kind: "local",
        shell: SHELL,
        cwd: "C:\\src",
        targetId: null,
        runOnConnect: null,
        press: true,
        env: null,
        pathPrepend: null,
      }),
    ).toEqual({
      kind: "local",
      shellName: "wsl:Ubuntu",
      cwd: "C:\\src",
      targetId: undefined,
    });
  });

  it("keeps the saved target's id, nothing from its config", () => {
    expect(
      tabStateFor({ kind: "ssh", config: CONFIG, targetId: "t-1", runOnConnect: null, press: true }),
    ).toEqual({
      kind: "ssh",
      targetId: "t-1",
    });
  });

  /* Including the startup command: it belongs to the entry in `terminal-hosts.json`, and the tab
     only points at the entry. Copying it here would let two copies of the same thing drift apart —
     edit the command, and an old tab still runs the old one. */
  it("does not copy the startup command out of the saved target", () => {
    expect(
      tabStateFor({ kind: "ssh", config: CONFIG, targetId: "t-1", runOnConnect: "cd ~/a", press: true }),
    ).toEqual({ kind: "ssh", targetId: "t-1" });
    expect(
      tabStateFor({
        kind: "local",
        shell: SHELL,
        cwd: null,
        targetId: "t-2",
        runOnConnect: "npm run dev",
        press: true,
        env: null,
        pathPrepend: null,
      }),
    ).toEqual({ kind: "local", shellName: "wsl:Ubuntu", cwd: null, targetId: "t-2" });
  });

  it("remembers nothing about a hand-typed SSH session", () => {
    // There is no id to point at, and the password must not be written out — so nothing is
    // written at all.
    expect(
      tabStateFor({ kind: "ssh", config: CONFIG, targetId: null, runOnConnect: null, press: true }),
    ).toBeUndefined();
  });
});
