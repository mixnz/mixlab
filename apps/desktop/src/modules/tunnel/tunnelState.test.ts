import { describe, expect, it } from "vitest";

import {
  formatSize,
  hintKey,
  needsTokenCard,
  parseProvider,
  programPathKey,
  rowTone,
  tokenStateKey,
  type TunnelInfo,
} from "./tunnelState";

const row = (over: Partial<TunnelInfo>): TunnelInfo => ({
  id: 1,
  provider: "cloudflare",
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

describe("parseProvider", () => {
  // Somebody who used the tab before ngrok joined it has nothing stored, and gets the tab they had.
  it("reads cloudflare when nothing is stored", () => {
    expect(parseProvider(null)).toBe("cloudflare");
  });

  it("ignores a stored value it does not know", () => {
    expect(parseProvider("frp")).toBe("cloudflare");
  });

  it("keeps a provider it knows", () => {
    expect(parseProvider("ngrok")).toBe("ngrok");
  });
});

describe("programPathKey", () => {
  // The cloudflared key predates ngrok: renaming it would lose the path a person set.
  it("keeps cloudflared's key and gives ngrok its own", () => {
    expect(programPathKey("cloudflare")).toBe("mixlab-tunnel-cloudflared");
    expect(programPathKey("ngrok")).toBe("mixlab-tunnel-ngrok");
  });
});

describe("hintKey", () => {
  it("names each hint by its own string", () => {
    expect(hintKey(row({ hint: "allowedHosts" }))).toEqual({ key: "tunnelTab.hintAllowedHosts" });
    expect(hintKey(row({ hint: "nothingListening" }))).toEqual({
      key: "tunnelTab.hintNothingListening",
      params: { target: "http://localhost:5173" },
    });
    expect(hintKey(row({ provider: "ngrok", hint: "needsAuthtoken" }))).toEqual({
      key: "tunnelTab.hintNeedsAuthtoken",
    });
  });

  it("says nothing without a hint", () => {
    expect(hintKey(row({}))).toBeNull();
  });
});

describe("tokenStateKey", () => {
  it("says where a token comes from, and never what it is", () => {
    expect(tokenStateKey({ set: true, source: "config", path: "/c/ngrok.yml" })).toBe("tunnelTab.tokenInConfig");
    expect(tokenStateKey({ set: true, source: "environment", path: null })).toBe("tunnelTab.tokenInEnvironment");
    expect(tokenStateKey({ set: false, source: null, path: "/c/ngrok.yml" })).toBe("tunnelTab.tokenNone");
  });
});

describe("needsTokenCard", () => {
  const none = { set: false, source: null, path: null } as const;
  const set = { set: true, source: "config", path: "/c/ngrok.yml" } as const;

  it("asks for a token only with ngrok chosen, found, and no token set", () => {
    expect(needsTokenCard("ngrok", true, none)).toBe(true);
    expect(needsTokenCard("ngrok", true, set)).toBe(false);
    expect(needsTokenCard("ngrok", false, none)).toBe(false);
    expect(needsTokenCard("cloudflare", true, none)).toBe(false);
  });

  // Before the status has been read, nothing is claimed.
  it("asks nothing while the status is unknown", () => {
    expect(needsTokenCard("ngrok", true, null)).toBe(false);
  });
});
