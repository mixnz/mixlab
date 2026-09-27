import { describe, expect, it, vi } from "vitest";
import { installThenFollow } from "./UpdatePanel";

describe("installThenFollow", () => {
  it("opens Settings on Updates once the installer is open", async () => {
    const opened = vi.fn();
    await installThenFollow(async () => true, true, opened);
    expect(opened).toHaveBeenCalledOnce();
  });

  it("stays put when opening the installer failed: the panel shows the failure", async () => {
    const opened = vi.fn();
    await installThenFollow(async () => false, true, opened);
    expect(opened).not.toHaveBeenCalled();
  });

  it("never opens Settings for a Windows install, which relaunches", async () => {
    const opened = vi.fn();
    await installThenFollow(async () => true, false, opened);
    expect(opened).not.toHaveBeenCalled();
  });
});
