import { describe, expect, it } from "vitest";

import type { ServiceSummary, SiteSummary } from "@mixengine/api";

import { failureMayHaveChanged, serviceFailure, siteFailure } from "./failureState";

const failed = {
  id: "php-fpm@phpmyadmin",
  state: "failed",
  last_failure: {
    at: 1_760_000_000_000,
    reason: { kind: "spawn_failed" },
    detail: "no credential is stored at x",
  },
} as unknown as ServiceSummary;

function site(pool: string | null): SiteSummary {
  return {
    domain: "phpmyadmin.mixengine.test",
    kind: { kind: "php-fpm", pool },
  } as unknown as SiteSummary;
}

describe("siteFailure", () => {
  it("names the pool and its sentence", () => {
    expect(siteFailure(site("php-fpm@phpmyadmin"), [failed])).toEqual({
      service: "php-fpm@phpmyadmin",
      detail: "no credential is stored at x",
    });
  });

  it("is nothing for a pool that runs, a site with no pool, or a pool whose row is gone", () => {
    const running = { ...failed, state: "running", last_failure: null } as unknown as ServiceSummary;
    expect(siteFailure(site("php-fpm@phpmyadmin"), [running])).toBeNull();
    expect(siteFailure(site(null), [failed])).toBeNull();
    expect(
      siteFailure({ domain: "a.test", kind: { kind: "static" } } as unknown as SiteSummary, [failed]),
    ).toBeNull();
    expect(siteFailure(site("php-fpm@gone"), [failed])).toBeNull();
  });
});

describe("serviceFailure", () => {
  it("is the note of a failed service and nothing else", () => {
    expect(serviceFailure("php-fpm@phpmyadmin", [failed])?.detail).toBe("no credential is stored at x");
    expect(serviceFailure("mailpit", [failed])).toBeNull();
    expect(serviceFailure(null, [failed])).toBeNull();
  });

  it("shows no failure the state no longer claims", () => {
    const stale = { ...failed, state: "stopped" } as unknown as ServiceSummary;
    expect(serviceFailure("php-fpm@phpmyadmin", [stale])).toBeNull();
  });
});

describe("failureMayHaveChanged", () => {
  const moved = (to: string) => JSON.stringify({ type: "service_state_changed", service: "x", to });

  /* T200b, D6: a screen showing failures reads the listing again when one may have started or
     ended — a move into `failed`, or out of it into `running`. */
  it("is a move into failed or into running", () => {
    expect(failureMayHaveChanged(moved("failed"))).toBe(true);
    expect(failureMayHaveChanged(moved("running"))).toBe(true);
    expect(failureMayHaveChanged(moved("stopping"))).toBe(false);
    expect(failureMayHaveChanged(JSON.stringify({ type: "job_finished", job: 1 }))).toBe(false);
    expect(failureMayHaveChanged("not json")).toBe(false);
  });
});
