import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { pollNowAndEvery } from "./useUpdates";

describe("pollNowAndEvery", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.stubGlobal("window", globalThis);
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("reads at once, so coming back from the installer shows Finish without a wait", () => {
    const read = vi.fn();
    const stop = pollNowAndEvery(read, 3_000);
    expect(read).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(3_000);
    expect(read).toHaveBeenCalledTimes(2);
    stop();
    vi.advanceTimersByTime(9_000);
    expect(read).toHaveBeenCalledTimes(2);
  });
});
