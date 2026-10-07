import { describe, expect, it } from "vitest";

import { formatSize, rowTone, type TunnelInfo } from "./tunnelState";

const row = (over: Partial<TunnelInfo>): TunnelInfo => ({
  id: 1,
  target: "http://localhost:5173",
  url: null,
  state: "connecting",
  detail: null,
  hint: null,
  ...over,
});

describe("rowTone", () => {
  it("is in transition while connecting, good once open, bad once failed", () => {
    expect(rowTone(row({}))).toBe("warning");
    expect(rowTone(row({ state: "open", url: "https://a.trycloudflare.com" }))).toBe("success");
    expect(rowTone(row({ state: "failed" }))).toBe("danger");
  });
});

describe("formatSize", () => {
  it("says a download's size in megabytes, to one place", () => {
    expect(formatSize(55_365_048)).toBe("52.8 MB");
  });
});
