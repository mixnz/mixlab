import { describe, expect, it } from "vitest";
import { isServing, pillTone, stateHint, stateLabel } from "./serviceView";

const t = ((key: string) => `t:${key}`) as Parameters<typeof stateLabel>[0];

describe("isServing", () => {
  it("counts degraded as serving, since it still answers", () => {
    expect(isServing("running")).toBe(true);
    expect(isServing("degraded")).toBe(true);
  });
  it("does not count a stopped service", () => {
    expect(isServing("stopped")).toBe(false);
  });
});

describe("pillTone", () => {
  it("maps the state's tone onto a pill's", () => {
    expect(pillTone("running")).toBe("success");
    expect(pillTone("failed")).toBe("danger");
    expect(pillTone("starting")).toBe("warning");
  });
  it("keeps a service the daemon put to rest neutral", () => {
    expect(pillTone("stopped", "daemon")).toBe("neutral");
  });
});

describe("stateLabel", () => {
  it("shows a state from a newer daemon as it came", () => {
    expect(stateLabel(t, "hibernating")).toBe("hibernating");
  });
});

describe("stateHint", () => {
  it("gives a failed service's own reason", () => {
    expect(stateHint(t, "failed", null, "port 3306 is taken")).toBe("port 3306 is taken");
  });
});
