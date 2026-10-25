import { describe, expect, it } from "vitest";

import { canChangePort, confirmLabel, parsePort, restartsOnMove } from "./portDialogState";

describe("parsePort", () => {
  it("reads a port a client can dial", () => {
    expect(parsePort("3306")).toBe(3306);
    expect(parsePort(" 6379 ")).toBe(6379);
    expect(parsePort("65535")).toBe(65535);
  });

  it("refuses what is not one, before the daemon is asked", () => {
    for (const text of ["", "0", "65536", "33.6", "abc", "-1", "3306x"]) {
      expect(parsePort(text)).toBeNull();
    }
  });
});

describe("confirmLabel", () => {
  it("says the restart when the service is running", () => {
    expect(confirmLabel(true)).toBe("mixengine.port.changeAndRestart");
    expect(confirmLabel(false)).toBe("mixengine.port.change");
  });
});

describe("canChangePort", () => {
  it("offers a move only where the daemon said yes", () => {
    expect(canChangePort({ port: 3306, portMovable: true })).toBe(true);
    expect(canChangePort({ port: 3306, portMovable: false })).toBe(false);
  });

  it("offers nothing to a daemon too old to say", () => {
    expect(canChangePort({ port: 3306, portMovable: null })).toBe(false);
  });

  it("offers nothing for a service with no port", () => {
    expect(canChangePort({ port: null, portMovable: true })).toBe(false);
  });
});

describe("restartsOnMove", () => {
  it("is true for anything up or on its way up, as the daemon reads it", () => {
    for (const state of ["running", "starting", "restarting", "degraded"] as const) {
      expect(restartsOnMove(state)).toBe(true);
    }
  });

  it("is false for a service that is down", () => {
    for (const state of ["stopped", "failed", null] as const) {
      expect(restartsOnMove(state)).toBe(false);
    }
  });
});
