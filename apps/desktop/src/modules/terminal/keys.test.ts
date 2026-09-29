import { describe, expect, it } from "vitest";
import type { Press } from "../../core/shortcuts";
import { shellKeeps } from "./keys";

function press(over: Partial<Press>): Press {
  return { key: "a", shift: false, alt: false, mod: false, ctrlOnly: false, typing: true, ...over };
}

describe("shellKeeps", () => {
  it("hands over a chord the app is listening for", () => {
    expect(shellKeeps(press({ key: "w", mod: true }), true)).toBe(false);
  });

  /* `Ctrl+R` is reverse history search and `Ctrl+A` is go to the start of the line. They are in the
     app's catalogue, but in a terminal tab nobody listens to them — and what nobody listens to
     belongs to the shell. */
  it("keeps a chord no handler answers", () => {
    expect(shellKeeps(press({ key: "r", mod: true }), false)).toBe(true);
    expect(shellKeeps(press({ key: "a", mod: true }), false)).toBe(true);
  });

  it("keeps everything typed without the shortcut modifier", () => {
    expect(shellKeeps(press({ key: "c" }), true)).toBe(true);
    expect(shellKeeps(press({ key: "v" }), true)).toBe(true);
  });

  /* `Ctrl+C` is two different commands depending on the moment, and what decides is whether there
     is a selection — which `terminal.copy` expresses by only registering when there is one. Here it
     arrives as `claimed`. */
  it("copies when something is selected and cancels when nothing is", () => {
    expect(shellKeeps(press({ key: "c", mod: true }), true)).toBe(false);
    expect(shellKeeps(press({ key: "c", mod: true }), false)).toBe(true);
  });

  /* Paste does not go through the catalogue: let go so the webview pastes into xterm's textarea. */
  it("always steps aside for paste", () => {
    expect(shellKeeps(press({ key: "v", mod: true }), false)).toBe(false);
    expect(shellKeeps(press({ key: "v", mod: true, shift: true }), false)).toBe(false);
  });

  /* On a Mac, `Ctrl+Tab` arrives with `mod` off because `mod` there is `⌘`. Without asking
     `ctrlOnly` the terminal would keep the key and the tab would never change. */
  it("hands over a Ctrl chord the app claims, even with the primary modifier up", () => {
    expect(shellKeeps(press({ key: "tab", ctrlOnly: true }), true)).toBe(false);
  });

  it("keeps a Ctrl chord nobody claims — Ctrl+V on a Mac is the shell's, not a paste", () => {
    expect(shellKeeps(press({ key: "v", ctrlOnly: true }), false)).toBe(true);
    expect(shellKeeps(press({ key: "r", ctrlOnly: true }), false)).toBe(true);
  });
});
