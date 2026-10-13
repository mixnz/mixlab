import { afterEach, describe, expect, it, vi } from "vitest";
import { IS_MAC } from "./platform";
import { isBlockedReload } from "./reload";

function press(key: string, { mod = true, shift = false } = {}): KeyboardEvent {
  return {
    key,
    ctrlKey: mod && !IS_MAC,
    metaKey: mod && IS_MAC,
    shiftKey: shift,
    altKey: false,
  } as KeyboardEvent;
}

/* The plain `Ctrl+R` belongs to the shortcut dispatcher. A refusal made here runs before the
   dispatcher whenever its listener has been re-added, and the dispatcher ignores a prevented
   event — which is how the panes once stopped answering the key at all. */
describe("isBlockedReload", () => {
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it("leaves the plain chord to the dispatcher in a packaged build", () => {
    vi.stubEnv("PROD", true);
    expect(isBlockedReload(press("r"))).toBe(false);
    expect(isBlockedReload(press("R"))).toBe(false);
  });

  it("refuses the rest of the webview's reloads in a packaged build", () => {
    vi.stubEnv("PROD", true);
    expect(isBlockedReload(press("F5", { mod: false }))).toBe(true);
    expect(isBlockedReload(press("R", { shift: true }))).toBe(true);
  });

  it("leaves every reload live under the dev build", () => {
    vi.stubEnv("PROD", false);
    expect(isBlockedReload(press("r"))).toBe(false);
    expect(isBlockedReload(press("F5", { mod: false }))).toBe(false);
  });
});
