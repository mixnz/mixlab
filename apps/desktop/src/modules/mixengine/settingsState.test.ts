import { describe, expect, it } from "vitest";

import { autostartPresentation, credentialsPresentation, doctorChecksInOrder } from "./settingsState";

describe("autostartPresentation", () => {
  it("is unsupported when this machine has no mechanism", () => {
    expect(autostartPresentation({ mechanism: "none", enabled: false, for_this_home: false })).toBe(
      "unsupported",
    );
  });

  it("is disabled when nothing is registered", () => {
    expect(
      autostartPresentation({ mechanism: "logon_task", enabled: false, for_this_home: false }),
    ).toBe("disabled");
  });

  /* This is the state T85b exists to name: enabled is true, but not for this home. */
  it("is enabledOtherHome when the registered entry belongs to a different home", () => {
    expect(
      autostartPresentation({ mechanism: "logon_task", enabled: true, for_this_home: false }),
    ).toBe("enabledOtherHome");
  });

  it("is enabledThisHome only when both enabled and for_this_home are true", () => {
    expect(
      autostartPresentation({ mechanism: "launch_agent", enabled: true, for_this_home: true }),
    ).toBe("enabledThisHome");
  });
});

describe("doctorChecksInOrder", () => {
  /* A shorter list reads as a clean answer rather than a question that was never asked — so every
     "ok" has to stay, not be filtered out before drawing. */
  it("keeps every check, including ones that all report ok", () => {
    const checks = [
      { name: "a", outcome: { outcome: "ok" as const } },
      { name: "b", outcome: { outcome: "ok" as const } },
      { name: "c", outcome: { outcome: "ok" as const } },
    ];
    expect(doctorChecksInOrder({ checks })).toEqual(checks);
    expect(doctorChecksInOrder({ checks })).toHaveLength(3);
  });

  it("preserves order for a mix of outcomes", () => {
    const checks = [
      { name: "a", outcome: { outcome: "ok" as const } },
      { name: "b", outcome: { outcome: "problem" as const, id: "hosts_block_differs" as const, because: "x" } },
      { name: "c", outcome: { outcome: "skipped" as const, because: "x" } },
    ];
    expect(doctorChecksInOrder({ checks }).map((c) => c.name)).toEqual(["a", "b", "c"]);
  });
});

describe("credentialsPresentation", () => {
  it("draws nothing for a daemon that predates the member", () => {
    expect(credentialsPresentation(undefined)).toBeNull();
    expect(credentialsPresentation(null)).toBeNull();
  });

  it("offers the other store only when the daemon would accept it", () => {
    expect(credentialsPresentation({ store: "os", choosable: true })).toEqual({ store: "os", switchTo: "home" });
    expect(credentialsPresentation({ store: "home", choosable: true })).toEqual({ store: "home", switchTo: "os" });
    expect(credentialsPresentation({ store: "os", choosable: false })).toEqual({ store: "os", switchTo: null });
  });
});
