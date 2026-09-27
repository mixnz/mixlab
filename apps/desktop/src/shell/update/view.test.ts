import { describe, expect, it } from "vitest";
import { isNewer, panelView, updateView, type View, type ViewInput } from "./view";

const base: ViewInput = {
  current: "0.0.9",
  placement: { kind: "swap" },
  offered: { version: "0.0.10", hasBuild: true },
  skipped: null,
  installing: false,
  handedOver: false,
  onDisk: null,
  downloading: false,
  downloaded: null,
};

describe("updateView", () => {
  it("offers a newer version", () => expect(updateView(base)).toBe("offer"));

  it("says up to date when the feed is not newer", () =>
    expect(updateView({ ...base, offered: { version: "0.0.9", hasBuild: true } })).toBe("upToDate"));

  it("never offers an older version, whatever the feed says", () =>
    expect(updateView({ ...base, offered: { version: "0.0.8", hasBuild: true } })).toBe("upToDate"));

  it("says so when there is no build for this machine", () =>
    expect(updateView({ ...base, offered: { version: "0.0.10", hasBuild: false } })).toBe("noBuild"));

  it("keeps a skipped version quiet", () => expect(updateView({ ...base, skipped: "0.0.10" })).toBe("skipped"));

  it("puts development and elsewhere before any offer", () => {
    expect(updateView({ ...base, placement: { kind: "development" } })).toBe("development");
    expect(updateView({ ...base, placement: { kind: "elsewhere" } })).toBe("elsewhere");
  });

  it("shows the work in progress over the offer", () =>
    expect(updateView({ ...base, installing: true })).toBe("installing"));

  it("offers Finish once the installer has put the new version on disk", () => {
    const handed: ViewInput = { ...base, placement: { kind: "installer" }, handedOver: true };
    expect(updateView(handed)).toBe("handedOver");
    expect(updateView({ ...handed, onDisk: "0.0.9" })).toBe("handedOver");
    expect(updateView({ ...handed, onDisk: "0.0.10" })).toBe("finish");
  });

  it("is up to date when nothing has been read yet", () =>
    expect(updateView({ ...base, offered: null })).toBe("upToDate"));

  it("is downloading while a download runs, over the offer", () =>
    expect(updateView({ ...base, downloading: true })).toBe("downloading"));

  it("is ready when the offered version is downloaded, without downloading again", () =>
    expect(updateView({ ...base, downloaded: "0.0.10" })).toBe("ready"));

  it("does not call an older download ready for a newer offer", () =>
    expect(updateView({ ...base, downloaded: "0.0.9" })).toBe("offer"));

  it("keeps a skipped version quiet even when it is downloaded", () =>
    expect(updateView({ ...base, skipped: "0.0.10", downloaded: "0.0.10" })).toBe("skipped"));

  it("shows an install in progress over a finished download", () =>
    expect(updateView({ ...base, downloaded: "0.0.10", installing: true })).toBe("installing"));
});

describe("panelView", () => {
  const panel = (view: View, later = false, failed = false) => panelView({ view, later, failed });

  it("offers, downloads and asks to install", () => {
    expect(panel("offer")).toBe("offer");
    expect(panel("downloading")).toBe("downloading");
    expect(panel("ready")).toBe("ready");
  });

  it("hides after Later until the next start, but not a download in progress", () => {
    expect(panel("offer", true)).toBe("hidden");
    expect(panel("ready", true)).toBe("hidden");
    expect(panel("downloading", true)).toBe("downloading");
  });

  it("says a failure over the offer or the install it came from", () => {
    expect(panel("offer", false, true)).toBe("failed");
    expect(panel("ready", false, true)).toBe("failed");
  });

  it("draws nothing for any other state, failed or not", () => {
    const quiet: View[] = ["upToDate", "noBuild", "skipped", "development", "elsewhere", "installing", "handedOver", "finish"];
    for (const view of quiet) {
      expect(panel(view)).toBe("hidden");
      expect(panel(view, false, true)).toBe("hidden");
    }
  });
});

describe("isNewer", () => {
  it("compares numerically, part by part", () => {
    expect(isNewer("0.0.10", "0.0.9")).toBe(true);
    expect(isNewer("0.0.9", "0.0.10")).toBe(false);
    expect(isNewer("0.1.0", "0.0.99")).toBe(true);
    expect(isNewer("0.0.9", "0.0.9")).toBe(false);
  });
});
