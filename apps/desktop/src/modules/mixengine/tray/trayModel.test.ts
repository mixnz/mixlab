import { describe, expect, it } from "vitest";

import type { ServiceRow } from "../daemonState";
import { nextConfirm, serviceCounts, shutdownReport } from "./trayModel";

function row(id: string, state: ServiceRow["state"]): ServiceRow {
  return { id, state, port: null, autostart: false, stoppedBy: null, version: null, lastFailure: null };
}

describe("the inline confirmation", () => {
  it("asks, and a confirm or a cancel both put the button back", () => {
    const armed = nextConfirm(null, { type: "arm", key: "shutdown" });
    expect(armed).toBe("shutdown");
    expect(nextConfirm(armed, { type: "cancel" })).toBeNull();
  });

  it("goes away when its own time runs out", () => {
    expect(nextConfirm("stopAll", { type: "timeout", key: "stopAll" })).toBeNull();
  });

  it("is not taken away by the timer of an earlier question", () => {
    expect(nextConfirm("shutdown", { type: "timeout", key: "stopAll" })).toBe("shutdown");
  });

  it("goes away when the panel hides", () => {
    expect(nextConfirm("stopAll", { type: "hide" })).toBeNull();
  });
});

describe("the service counts", () => {
  it("counts a degraded service as up and a starting one as not", () => {
    const rows = [row("a", "running"), row("b", "degraded"), row("c", "starting"), row("d", "stopped")];
    expect(serviceCounts(rows)).toEqual({ up: 2, total: 4 });
  });
});

describe("the shutdown report", () => {
  it("says how many stopped and names what would not", () => {
    expect(
      shutdownReport({
        services: {
          planned: ["caddy", "mariadb@main"],
          complete: false,
          reached: ["caddy"],
          failed: { service: "mariadb@main" },
          blocked: [],
        },
        unordered: null,
      }),
    ).toEqual({ stopped: 1, failed: "mariadb@main", unordered: null });
  });

  it("carries the daemon's sentence when there was no order", () => {
    const report = shutdownReport({
      services: { planned: [], complete: true, reached: [], blocked: [] },
      unordered: { code: "invalid_argument", message: "extension.toml does not parse" },
    });
    expect(report.unordered).toBe("extension.toml does not parse");
  });
});
