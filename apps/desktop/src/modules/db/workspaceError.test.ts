import { describe, expect, it } from "vitest";
import { reachesErrorBanner } from "./workspaceError";

const lost = "Mất kết nối tới máy chủ.";

describe("reachesErrorBanner", () => {
  it("keeps the lost connection off a tunnelled workspace's banner", () => {
    expect(reachesErrorBanner(lost, lost)).toBe(false);
  });

  it("lets it through when nothing else is going to say it", () => {
    // A direct connection: no TunnelBanner shows up in its place.
    expect(reachesErrorBanner(lost, null)).toBe(true);
  });

  it("lets through a failure that has nothing to do with the tunnel", () => {
    expect(reachesErrorBanner("MySQL: You have an error in your SQL syntax", lost)).toBe(true);
  });

  it("lets through the empty string that takes the banner down", () => {
    expect(reachesErrorBanner("", lost)).toBe(true);
  });
});
