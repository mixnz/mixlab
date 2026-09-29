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
  it("đọc lại được nhánh ssh", () => {
    expect(parseTerminalTabState({ kind: "ssh", targetId: "t-1" })).toEqual({
      kind: "ssh",
      targetId: "t-1",
    });
  });

  /* State written by the previous version, when the list only held servers and the id was called
     `hostId`. A tab open while the user upgrades does not lose where it was. */
  it("đọc được id của phiên trước, hồi nó còn tên là hostId", () => {
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

  it("đọc lại được nhánh local", () => {
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
  it("giữ id của đích đã lưu bên cạnh shell, không thay nó", () => {
    expect(
      parseTerminalTabState({ kind: "local", shellName: "pwsh", cwd: null, targetId: "t-1" }),
    ).toEqual({ kind: "local", shellName: "pwsh", cwd: null, targetId: "t-1" });
  });

  it("nhận shell không có thư mục bắt đầu, viết cách nào cũng được", () => {
    // Compares the whole object rather than `?.cwd`: `TerminalTabState` is a union, and the `ssh`
    // branch has no `cwd`.
    const expected = { kind: "local", shellName: "pwsh", cwd: null, targetId: undefined };
    expect(parseTerminalTabState({ kind: "local", shellName: "pwsh", cwd: null })).toEqual(expected);
    expect(parseTerminalTabState({ kind: "local", shellName: "pwsh" })).toEqual(expected);
  });

  it("không nói gì về tab chưa từng ghi", () => {
    expect(parseTerminalTabState(undefined)).toBeNull();
  });

  /* Everything below is a string some version of the app wrote into `localStorage`, so nothing is
     trusted — the shell deliberately passes it through without looking. */
  it("bỏ qua mọi thứ không phải state của tab terminal", () => {
    expect(parseTerminalTabState(null)).toBeNull();
    expect(parseTerminalTabState("ssh")).toBeNull();
    expect(parseTerminalTabState([])).toBeNull();
    expect(parseTerminalTabState({})).toBeNull();
    expect(parseTerminalTabState({ kind: "telnet", targetId: "t-1" })).toBeNull();
    expect(parseTerminalTabState({ kind: "ssh" })).toBeNull();
    expect(parseTerminalTabState({ kind: "ssh", targetId: "" })).toBeNull();
    expect(parseTerminalTabState({ kind: "ssh", targetId: 7 })).toBeNull();
    expect(parseTerminalTabState({ kind: "local" })).toBeNull();
    expect(parseTerminalTabState({ kind: "local", shellName: "" })).toBeNull();
    expect(parseTerminalTabState({ kind: "local", shellName: "pwsh", cwd: 7 })).toBeNull();
  });

  /* An unreadable id does not break the whole state: the shell and directory can still reopen the
     tab, there is just no entry to look up the startup command in. */
  it("bỏ id hỏng của nhánh local mà vẫn giữ được shell", () => {
    expect(parseTerminalTabState({ kind: "local", shellName: "pwsh", targetId: 7 })).toEqual({
      kind: "local",
      shellName: "pwsh",
      cwd: null,
      targetId: undefined,
    });
  });
});

describe("tabStateFor", () => {
  it("giữ tên shell và thư mục bắt đầu, không giữ đường dẫn", () => {
    expect(
      tabStateFor({
        kind: "local",
        shell: SHELL,
        cwd: "C:\\src",
        targetId: null,
        runOnConnect: null,
      }),
    ).toEqual({
      kind: "local",
      shellName: "wsl:Ubuntu",
      cwd: "C:\\src",
      targetId: undefined,
    });
  });

  it("giữ id của đích đã lưu, không giữ gì trong config", () => {
    expect(
      tabStateFor({ kind: "ssh", config: CONFIG, targetId: "t-1", runOnConnect: null }),
    ).toEqual({
      kind: "ssh",
      targetId: "t-1",
    });
  });

  /* Including the startup command: it belongs to the entry in `terminal-hosts.json`, and the tab
     only points at the entry. Copying it here would let two copies of the same thing drift apart —
     edit the command, and an old tab still runs the old one. */
  it("không chép lệnh mở màn ra khỏi đích đã lưu", () => {
    expect(
      tabStateFor({ kind: "ssh", config: CONFIG, targetId: "t-1", runOnConnect: "cd ~/a" }),
    ).toEqual({ kind: "ssh", targetId: "t-1" });
    expect(
      tabStateFor({
        kind: "local",
        shell: SHELL,
        cwd: null,
        targetId: "t-2",
        runOnConnect: "npm run dev",
      }),
    ).toEqual({ kind: "local", shellName: "wsl:Ubuntu", cwd: null, targetId: "t-2" });
  });

  it("không nhớ gì về một phiên SSH gõ tay", () => {
    // There is no id to point at, and the password must not be written out — so nothing is
    // written at all.
    expect(
      tabStateFor({ kind: "ssh", config: CONFIG, targetId: null, runOnConnect: null }),
    ).toBeUndefined();
  });
});
