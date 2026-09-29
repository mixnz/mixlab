import { describe, expect, it } from "vitest";
import { HIDDEN, nextBannerState, popupShows, type BannerState } from "./state";

const reconnecting = { id: "a", state: "reconnecting" } as const;
const reconnected = { id: "a", state: "reconnected" } as const;
const failed = { id: "a", state: "failed", error: { code: "error.sshAuthFailed" } } as const;

describe("nextBannerState", () => {
  it("shows the loss, then the recovery, and the recovery is what the component hides again", () => {
    const losing = nextBannerState(HIDDEN, reconnecting);
    expect(losing).toEqual({ kind: "reconnecting" });
    expect(nextBannerState(losing, reconnected)).toEqual({ kind: "reconnected" });
  });

  it("says nothing about a recovery nobody saw the loss of", () => {
    // A tab opened after the tunnel has healed itself has nobody to reassure.
    expect(nextBannerState(HIDDEN, reconnected)).toBe(HIDDEN);
  });

  it("keeps the same failure rather than replacing it", () => {
    // The watcher widens its interval and reports the same error again; if each time were a new
    // object, everything React attaches to that object would be rebuilt on the backoff beat.
    const first = nextBannerState(HIDDEN, failed);
    expect(first).toEqual({ kind: "failed", error: { code: "error.sshAuthFailed" } });
    expect(nextBannerState(first, failed)).toBe(first);
  });

  it("replaces a failure when the reason changed", () => {
    const first = nextBannerState(HIDDEN, failed);
    const other = { id: "a", state: "failed", error: { code: "error.sshTimeout" } } as const;
    expect(nextBannerState(first, other)).toEqual({
      kind: "failed",
      error: { code: "error.sshTimeout" },
    });
  });

  it("clears a failure when the tunnel comes back, and when it is being tried again", () => {
    const first: BannerState = nextBannerState(HIDDEN, failed);
    expect(nextBannerState(first, reconnected)).toEqual({ kind: "reconnected" });
    expect(nextBannerState(first, reconnecting)).toEqual({ kind: "reconnecting" });
  });

  it("does not restart a reconnection that is already on screen", () => {
    const busy = nextBannerState(HIDDEN, reconnecting);
    expect(nextBannerState(busy, reconnecting)).toBe(busy);
  });
});

describe("popupShows", () => {
  it("says nothing at all until there is something to say", () => {
    expect(popupShows(HIDDEN, true, true)).toBe(false);
  });

  it("waits out a drop too short to be worth blocking the screen for", () => {
    const losing: BannerState = { kind: "reconnecting" };
    expect(popupShows(losing, false, false)).toBe(false);
    expect(popupShows(losing, true, false)).toBe(true);
  });

  it("blocks on a failure however long it took to get there", () => {
    const dead: BannerState = { kind: "failed", error: { code: "error.sshAuthFailed" } };
    expect(popupShows(dead, false, false)).toBe(true);
  });

  it("only reassures whoever was being blocked", () => {
    const back: BannerState = { kind: "reconnected" };
    expect(popupShows(back, true, true)).toBe(true);
    expect(popupShows(back, true, false)).toBe(false);
  });
});
