import { describe, expect, it } from "vitest";

import { cleanupFlagFor, isCleanupReclaimable } from "./diskUsageState";

describe("cleanupFlagFor", () => {
  it("maps logs and cache to their CleanupQuery flag", () => {
    expect(cleanupFlagFor("logs")).toBe("keep_logs");
    expect(cleanupFlagFor("cache")).toBe("keep_cache");
  });

  /* runtimes/data/certs never have reclaim: "by_cleanup" — there is no flag to send. */
  it("has no flag for the three categories daemon.cleanup never touches", () => {
    expect(cleanupFlagFor("runtimes")).toBeNull();
    expect(cleanupFlagFor("data")).toBeNull();
    expect(cleanupFlagFor("certs")).toBeNull();
  });
});

describe("isCleanupReclaimable", () => {
  const category = (reclaim: unknown) =>
    ({ id: "logs", location: "/logs", bytes: 0, files: 0, reclaim }) as never;

  it("is true only for by_cleanup, read from the daemon and not guessed from the category id", () => {
    expect(isCleanupReclaimable(category({ reclaim: "by_cleanup", bytes: 0, files: 0 }))).toBe(true);
    expect(isCleanupReclaimable(category({ reclaim: "never", because: "x" }))).toBe(false);
    expect(isCleanupReclaimable(category({ reclaim: "by_method", method: "x", because: "x" }))).toBe(
      false,
    );
    expect(isCleanupReclaimable(category({ reclaim: "at_a_cost", because: "x" }))).toBe(false);
  });
});
