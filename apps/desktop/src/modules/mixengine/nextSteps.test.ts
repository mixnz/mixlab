import { describe, expect, it } from "vitest";
import type { BlueprintApplied, NextStep } from "@mixengine/api";
import { oneShotState, targetToSave, openAddress, opensByItself, requiredRuns } from "./nextSteps";

const once = (run: string, optional = false): NextStep => ({ kind: "once", run, optional });
const serve = (run: string, optional = false): NextStep => ({ kind: "serve", run, optional });
const open = (path?: string): NextStep => ({ kind: "open", path });

function applied(steps: NextStep[] | null, failed = false): BlueprintApplied {
  return {
    blueprint: "x", project: "shop", root: "/p",
    steps: failed ? [{ action: { action: "run_scaffold", command: "c" }, result: { result: "failed", why: "w" } }] : [],
    next_steps: steps === null ? undefined : { trusted: true, steps },
  } as BlueprintApplied;
}

describe("opensByItself", () => {
  it("opens with no steps, only optional ones, or only open steps", () => {
    expect(opensByItself(applied(null))).toBe(true);
    expect(opensByItself(applied([once("php artisan migrate", true), serve("npm run dev", true)]))).toBe(true);
    expect(opensByItself(applied([open("/wp-admin/install.php")]))).toBe(true);
  });
  it("waits when a step is needed or a step failed", () => {
    expect(opensByItself(applied([serve("npm run dev")]))).toBe(false);
    expect(opensByItself(applied([once("npm run build")]))).toBe(false);
    expect(opensByItself(applied(null, true))).toBe(false);
  });
});

describe("openAddress", () => {
  it("lands on the first open step's path", () => {
    expect(openAddress("https://wp.test", [once("x", true), open("/wp-admin/install.php")])).toBe(
      "https://wp.test/wp-admin/install.php",
    );
    expect(openAddress("https://a.test/", [])).toBe("https://a.test/");
  });
});

describe("requiredRuns", () => {
  it("puts required once steps before the first required serve, and nothing optional", () => {
    expect(
      requiredRuns([once("npm init -y"), once("x", true), serve("node index.js"), serve("worker"), serve("y", true)]),
    ).toEqual({ first: ["npm init -y", "node index.js"], furtherServes: ["worker"] });
  });
});

describe("terminal states", () => {
  const toolchain = { bin: "C:\\Users\\Nguyễn Văn\\.mixengine\\bin", home: "C:\\Users\\Nguyễn Văn\\.mixengine" };

  it("a one-shot run carries the toolchain and the lines", () => {
    expect(oneShotState(["npm run dev"], "/p", toolchain, true)).toEqual({
      kind: "local", shellName: "", cwd: "/p",
      env: { MIXENGINE_HOME: toolchain.home }, pathPrepend: [toolchain.bin],
      run: ["npm run dev"], press: true,
    });
  });

  /* What *Save as Terminal target* asks the Terminal to save: a dev server's line, typed and left
     for Enter when the tab comes back, so reopening MixLab never starts a server by itself. */
  it("a target to save is the serve line, typed-not-run on restore", () => {
    expect(targetToSave("shop", "/p", "npm run dev", toolchain)).toEqual({
      name: "shop · npm run dev", shellName: "", cwd: "/p",
      env: { MIXENGINE_HOME: toolchain.home }, pathPrepend: [toolchain.bin],
      runOnConnect: "npm run dev", onRestore: "type",
    });
  });
});
