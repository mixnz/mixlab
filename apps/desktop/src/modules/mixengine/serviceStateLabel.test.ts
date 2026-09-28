import { describe, expect, it } from "vitest";

import { serviceStateHint, serviceStateKey, serviceStateTone, toggleMode } from "./serviceStateLabel";

describe("serviceStateKey", () => {
  it("names a key for every state the closed enum has", () => {
    for (const state of [
      "stopped",
      "starting",
      "running",
      "degraded",
      "stopping",
      "restarting",
      "failed",
    ]) {
      expect(serviceStateKey(state)).toBe(`mixengine.serviceState.${state}`);
    }
  });

  /* `ServiceState` is closed today, but this reads whatever a daemon sent — including one newer
     than this build. An unknown state is shown as the daemon spelled it, never swallowed. */
  it("returns null for a state it does not know, so the raw word can be shown", () => {
    expect(serviceStateKey("hibernating")).toBeNull();
  });

  it("treats absent as absent rather than as a state", () => {
    expect(serviceStateKey(null)).toBeNull();
    expect(serviceStateKey(undefined)).toBeNull();
  });
});

describe("serviceStateTone", () => {
  it("calls a running service good", () => {
    expect(serviceStateTone("running")).toBe("ok");
  });

  it("calls a service that is not running bad — stopped is a state, failed is a verdict", () => {
    expect(serviceStateTone("stopped")).toBe("bad");
    expect(serviceStateTone("failed")).toBe("bad");
  });

  /* Everything mid-move, plus `degraded`: it is up but not well, which is a warning about a
     service that still answers — not the red of one that does not. */
  it("calls every transition, and a degraded service, busy", () => {
    expect(serviceStateTone("starting")).toBe("busy");
    expect(serviceStateTone("stopping")).toBe("busy");
    expect(serviceStateTone("restarting")).toBe("busy");
    expect(serviceStateTone("degraded")).toBe("busy");
  });

  it("gives no tone to a state it does not know, or to none at all", () => {
    expect(serviceStateTone("hibernating")).toBeNull();
    expect(serviceStateTone(null)).toBeNull();
    expect(serviceStateTone(undefined)).toBeNull();
  });

  it("has a tone for every state that has a label, so no state is drawn colourless by accident", () => {
    for (const state of ["stopped", "starting", "running", "degraded", "stopping", "restarting", "failed"]) {
      expect(serviceStateTone(state)).not.toBeNull();
    }
  });
});

describe("toggleMode", () => {
  it("is up while the service answers — degraded still answers, so the action is still stop", () => {
    expect(toggleMode("running", false)).toBe("up");
    expect(toggleMode("degraded", false)).toBe("up");
  });

  it("is down for anything that is not answering", () => {
    expect(toggleMode("stopped", false)).toBe("down");
    expect(toggleMode("failed", false)).toBe("down");
  });

  it("is moving for every state that is mid-move", () => {
    expect(toggleMode("starting", false)).toBe("moving");
    expect(toggleMode("stopping", false)).toBe("moving");
    expect(toggleMode("restarting", false)).toBe("moving");
  });

  /* An action sent and not yet confirmed by an event is the same thing to a reader: something is
     happening and there is nothing to press. The state still says the old value at that point. */
  it("is moving while an action is in flight, whatever the state still says", () => {
    expect(toggleMode("running", true)).toBe("moving");
    expect(toggleMode("stopped", true)).toBe("moving");
  });

  it("treats an unknown or absent state as down, which offers start rather than nothing", () => {
    expect(toggleMode("hibernating", false)).toBe("down");
    expect(toggleMode(null, false)).toBe("down");
    expect(toggleMode(undefined, false)).toBe("down");
  });
});

/* T167g, ADR 0041: a service MixEngine put to rest is neither broken nor something a person turned
   off, so it is drawn as resting — and only that combination changes anything. */
describe("a service MixEngine stopped", () => {
  it("is resting, in a tone of its own", () => {
    expect(serviceStateKey("stopped", "daemon")).toBe("mixengine.serviceState.resting");
    expect(serviceStateTone("stopped", "daemon")).toBe("resting");
    expect(serviceStateHint("stopped", "daemon")).toBe("mixengine.serviceState.restingHint");
  });

  it("changes nothing when a person stopped it, nobody did, or the daemon did not say", () => {
    for (const stoppedBy of ["person", "never", null, undefined] as const) {
      expect(serviceStateKey("stopped", stoppedBy)).toBe("mixengine.serviceState.stopped");
      expect(serviceStateTone("stopped", stoppedBy)).toBe("bad");
      expect(serviceStateHint("stopped", stoppedBy)).toBeNull();
    }
  });

  it("changes nothing for a service that is not stopped", () => {
    expect(serviceStateKey("running", "daemon")).toBe("mixengine.serviceState.running");
    expect(serviceStateTone("failed", "daemon")).toBe("bad");
    expect(serviceStateHint("running", "daemon")).toBeNull();
  });
});
