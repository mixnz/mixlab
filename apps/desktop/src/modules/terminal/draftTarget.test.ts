import { describe, expect, it } from "vitest";
import { savedFromDraft, sameTarget, type DraftTarget } from "./draftTarget";
import type { SavedTarget } from "./types";

const draft: DraftTarget = {
  name: "shop · npm run dev",
  shellName: "",
  cwd: "C:\\Sites\\shop",
  env: { MIXENGINE_HOME: "C:\\h" },
  pathPrepend: ["C:\\h\\bin"],
  runOnConnect: "npm run dev",
  onRestore: "type",
};

describe("savedFromDraft", () => {
  it("is a saved local target on the shell it was given, with everything the draft said", () => {
    expect(savedFromDraft(draft, "pwsh", "t-1")).toEqual({
      id: "t-1",
      name: "shop · npm run dev",
      kind: "local",
      shellName: "pwsh",
      cwd: "C:\\Sites\\shop",
      runOnConnect: "npm run dev",
      onRestore: "type",
      env: { MIXENGINE_HOME: "C:\\h" },
      pathPrepend: ["C:\\h\\bin"],
    });
  });

  it("leaves out what is empty, and a run on restore, which is what absent means", () => {
    const plain = savedFromDraft({ ...draft, env: {}, pathPrepend: [], onRestore: "run" }, "pwsh", "t-2");
    expect(plain).not.toHaveProperty("env");
    expect(plain).not.toHaveProperty("pathPrepend");
    expect(plain).not.toHaveProperty("onRestore");
  });
});

describe("sameTarget", () => {
  const saved = savedFromDraft(draft, "pwsh", "t-1");

  it("finds the entry a second press would duplicate", () => {
    const again = savedFromDraft(draft, "pwsh", "t-9");
    expect(sameTarget([saved], again)).toBe(saved);
  });

  it("does not take a different command or folder for the same one", () => {
    expect(sameTarget([saved], { ...saved, id: "x", runOnConnect: "npm start" })).toBeUndefined();
    expect(sameTarget([saved], { ...saved, id: "x", cwd: "C:\\Sites\\other" })).toBeUndefined();
    const server: SavedTarget = {
      id: "s",
      name: saved.name,
      kind: "ssh",
      config: { host: "h", port: 22, username: "u", auth: { type: "password", password: "" } },
      runOnConnect: "npm run dev",
    };
    expect(sameTarget([server], saved)).toBeUndefined();
  });
});
