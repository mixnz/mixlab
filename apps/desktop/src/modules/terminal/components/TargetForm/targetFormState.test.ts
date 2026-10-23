import { describe, expect, it } from "vitest";
import { DEFAULT_SSH_PORT } from "../../../../core/ssh";
import type { LocalShell, SavedTarget } from "../../types";
import {
  buildChoice,
  buildTarget,
  initialForm,
  isSavable,
  snapshotOf,
  targetForm,
  type FormState,
} from "./targetFormState";

const BASH: LocalShell = { name: "bash", path: "/bin/bash", args: ["-l"] };

const LOCAL: SavedTarget = {
  id: "l-1",
  name: "shop",
  kind: "local",
  shellName: "bash",
  cwd: "/srv/shop",
  runOnConnect: "npm run dev",
  onRestore: "type",
  env: { A: "1" },
  pathPrepend: ["/x"],
};

const SSH: SavedTarget = {
  id: "s-1",
  name: "box",
  kind: "ssh",
  config: { host: "box.test", port: 2222, username: "ada", auth: { type: "password", password: "pw" } },
  runOnConnect: "htop",
};

const fresh = (): FormState => initialForm(null);

describe("initialForm", () => {
  it("holds nothing and opens on this machine when no tab came back", () => {
    const state = fresh();
    expect(state.held).toEqual({ kind: "none" });
    expect(state.draft.kind).toBe("local");
    expect(state.draft.ssh.port).toBe(DEFAULT_SSH_PORT);
    expect(state.draft.local.advanced).toBe(false);
  });

  it("rebuilds what the failed tab typed, with the saved entry's id and no snapshot yet", () => {
    const state = initialForm({
      kind: "local",
      shell: BASH,
      cwd: "/tmp",
      targetId: "l-1",
      runOnConnect: "ls",
      press: true,
      env: { A: "1" },
      pathPrepend: null,
    });
    expect(state.held).toEqual({ kind: "held", id: "l-1", snapshot: null, changedElsewhere: false });
    expect(state.draft.local).toEqual({ shellName: "bash", cwd: "/tmp", envText: "A=1", pathText: "", advanced: true });
    expect(state.draft.runOnConnect).toBe("ls");
  });
});

describe("apply", () => {
  it("loads a local entry and clears the server half", () => {
    const typed = targetForm(fresh(), { type: "editSsh", patch: { host: "other.test", password: "secret" } });
    const state = targetForm(typed, { type: "apply", entry: LOCAL });
    expect(state.draft.kind).toBe("local");
    expect(state.draft.local).toEqual({ shellName: "bash", cwd: "/srv/shop", envText: "A=1", pathText: "/x", advanced: true });
    expect(state.draft.ssh).toEqual(fresh().draft.ssh);
    expect(state.held).toEqual({ kind: "held", id: "l-1", snapshot: snapshotOf(LOCAL), changedElsewhere: false });
  });

  it("loads a server entry, clears the local fields and keeps the shell", () => {
    const local = targetForm(fresh(), { type: "apply", entry: LOCAL });
    const state = targetForm(local, { type: "apply", entry: SSH });
    expect(state.draft.kind).toBe("ssh");
    expect(state.draft.local).toEqual({ shellName: "bash", cwd: "", envText: "", pathText: "", advanced: true });
    expect(state.draft.ssh).toEqual({
      host: "box.test",
      port: 2222,
      username: "ada",
      authType: "password",
      password: "pw",
      keyPath: "",
      passphrase: "",
    });
    expect(state.draft.onRestore).toBe("run");
  });

  it("is what the form then describes, so Update reads unchanged", () => {
    const state = targetForm(fresh(), { type: "apply", entry: LOCAL });
    expect(snapshotOf(buildTarget(state.draft, ""))).toBe(snapshotOf(LOCAL));
  });
});

describe("clear", () => {
  it("empties the form and lets go of the entry, keeping the kind and the shell", () => {
    const held = targetForm(fresh(), { type: "apply", entry: LOCAL });
    const state = targetForm(held, { type: "clear" });
    expect(state.held).toEqual({ kind: "none" });
    expect(state.draft.kind).toBe("local");
    expect(state.draft.name).toBe("");
    expect(state.draft.local.shellName).toBe("bash");
    expect(state.draft.local.cwd).toBe("");
  });
});

describe("defaults", () => {
  it("fills only fields still empty", () => {
    const typed = targetForm(fresh(), { type: "editLocal", patch: { cwd: "/mine" } });
    const state = targetForm(typed, { type: "defaults", shellName: "zsh", cwd: "/home" });
    expect(state.draft.local.shellName).toBe("zsh");
    expect(state.draft.local.cwd).toBe("/mine");
  });
});

describe("a saved entry that changes elsewhere", () => {
  it("is flagged while the form holds edits", () => {
    const held = targetForm(fresh(), { type: "apply", entry: LOCAL });
    const state = targetForm(held, { type: "changedElsewhere" });
    expect(state.held).toEqual({ kind: "held", id: "l-1", snapshot: snapshotOf(LOCAL), changedElsewhere: true });
  });

  it("leaves the form with nothing held once deleted, and Save takes the notice down", () => {
    const held = targetForm(fresh(), { type: "apply", entry: LOCAL });
    const removed = targetForm(held, { type: "removedElsewhere" });
    expect(removed.held).toEqual({ kind: "removed" });
    const copy = { ...LOCAL, id: "l-2" };
    const saved = targetForm(removed, { type: "saved", entry: copy });
    expect(saved.held).toEqual({ kind: "held", id: "l-2", snapshot: snapshotOf(copy), changedElsewhere: false });
  });

  it("takes the notice down when dismissed", () => {
    const removed = targetForm(targetForm(fresh(), { type: "apply", entry: LOCAL }), { type: "removedElsewhere" });
    expect(targetForm(removed, { type: "dismissRemoved" }).held).toEqual({ kind: "none" });
  });
});

describe("named", () => {
  it("takes the name, the restore choice and the snapshot, and leaves the typed fields", () => {
    const start = initialForm({
      kind: "local",
      shell: BASH,
      cwd: "/typed",
      targetId: "l-1",
      runOnConnect: null,
      press: true,
      env: null,
      pathPrepend: null,
    });
    const state = targetForm(start, { type: "named", entry: LOCAL });
    expect(state.draft.name).toBe("shop");
    expect(state.draft.onRestore).toBe("type");
    expect(state.draft.local.cwd).toBe("/typed");
    expect(state.held).toEqual({ kind: "held", id: "l-1", snapshot: snapshotOf(LOCAL), changedElsewhere: false });
  });

  it("lets go of an id whose entry is gone", () => {
    const start = initialForm({ kind: "ssh", config: SSH.kind === "ssh" ? SSH.config : (undefined as never), targetId: "gone", runOnConnect: null, press: true });
    expect(targetForm(start, { type: "named", entry: undefined }).held).toEqual({ kind: "none" });
  });
});

describe("buildChoice and isSavable", () => {
  it("opens nothing on this machine until the shell is known and the variables read", () => {
    let state = targetForm(fresh(), { type: "editLocal", patch: { shellName: "bash", envText: "not a pair" } });
    expect(buildChoice(state.draft, [BASH], null)).toBeNull();
    state = targetForm(state, { type: "editLocal", patch: { envText: "A=1" } });
    expect(buildChoice(state.draft, [BASH], null)).toMatchObject({ kind: "local", shell: BASH, env: { A: "1" } });
  });

  it("asks a server for an address, a user and a password", () => {
    let state = targetForm(fresh(), { type: "edit", patch: { kind: "ssh" } });
    state = targetForm(state, { type: "editSsh", patch: { host: "h", username: "u" } });
    expect(buildChoice(state.draft, [], "s-1")).toBeNull();
    state = targetForm(state, { type: "editSsh", patch: { password: "p" } });
    expect(buildChoice(state.draft, [], "s-1")).toMatchObject({ kind: "ssh", targetId: "s-1" });
  });

  it("saves only with a name, and for this machine a known shell", () => {
    const named = targetForm(fresh(), { type: "edit", patch: { name: "x" } });
    expect(isSavable(named.draft, [BASH])).toBe(false);
    const withShell = targetForm(named, { type: "editLocal", patch: { shellName: "bash" } });
    expect(isSavable(withShell.draft, [BASH])).toBe(true);
  });
});
