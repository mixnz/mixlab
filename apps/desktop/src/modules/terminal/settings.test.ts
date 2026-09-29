import { describe, expect, it } from "vitest";
import { DEFAULT_FONT_SIZE } from "./fontSize";
import {
  DEFAULT_FONT_FAMILY,
  DEFAULT_SETTINGS,
  MAX_SCROLLBACK,
  MIN_SCROLLBACK,
  clampScrollback,
  sanitizeSettings,
  withLegacyFontSize,
} from "./settings";

describe("clampScrollback", () => {
  it("keeps a sensible number as it is", () => {
    expect(clampScrollback(20000)).toBe(20000);
  });

  it("stops at both ends", () => {
    expect(clampScrollback(0)).toBe(MIN_SCROLLBACK);
    expect(clampScrollback(-40)).toBe(MIN_SCROLLBACK);
    expect(clampScrollback(10_000_000)).toBe(MAX_SCROLLBACK);
  });

  /* The input is `type="number"`, but what arrives here is `Number(text)` of a field the user types
     into by hand, and an empty field gives `NaN`. */
  it("falls back to the default for a value that is not a number", () => {
    expect(clampScrollback(Number.NaN)).toBe(DEFAULT_SETTINGS.scrollback);
    expect(clampScrollback(Number.POSITIVE_INFINITY)).toBe(DEFAULT_SETTINGS.scrollback);
  });

  it("keeps the count whole", () => {
    expect(clampScrollback(1200.7)).toBe(1201);
  });
});

describe("sanitizeSettings", () => {
  it("defaults the terminal to Geist Mono", () => {
    expect(DEFAULT_FONT_FAMILY).toBe('"Geist Mono Variable", monospace');
  });

  /* The previous default names a font the app no longer bundles, so a file that still carries it
     is carrying a default rather than a choice. */
  it("moves the retired default font to the current one", () => {
    expect(sanitizeSettings({ fontFamily: '"Fira Code", monospace' }).fontFamily).toBe(DEFAULT_FONT_FAMILY);
  });

  it("gives every default when there is nothing stored", () => {
    expect(sanitizeSettings(undefined)).toEqual(DEFAULT_SETTINGS);
    expect(sanitizeSettings(null)).toEqual(DEFAULT_SETTINGS);
    expect(sanitizeSettings({})).toEqual(DEFAULT_SETTINGS);
  });

  /* The case really worth testing: a file written by the previous version, missing exactly the
     fields this version added. Fields present must be kept, missing ones filled in. */
  it("fills in only what an older file is missing", () => {
    const settings = sanitizeSettings({ fontSize: 18, scrollback: 12000 });
    expect(settings.fontSize).toBe(18);
    expect(settings.scrollback).toBe(12000);
    expect(settings.fontFamily).toBe(DEFAULT_FONT_FAMILY);
    expect(settings.cursorStyle).toBe(DEFAULT_SETTINGS.cursorStyle);
    expect(settings.rightClickPastes).toBe(false);
    expect(settings.titleShowsTargetName).toBe(DEFAULT_SETTINGS.titleShowsTargetName);
  });

  /* With nobody having touched the settings, the tab still carries the saved target's name: that is
     what someone opening five sessions to five servers needs, and `deploy@10.0.0.7` five times is
     not. */
  it("names a tab after its saved target until told otherwise", () => {
    expect(sanitizeSettings({}).titleShowsTargetName).toBe(true);
  });

  /* On by default, so `false` in the file is something the user said — not a missing field. It
     has to survive, just as `cursorBlink: false` survives. */
  it("keeps a setting the user turned off", () => {
    expect(sanitizeSettings({ titleShowsTargetName: false }).titleShowsTargetName).toBe(false);
  });

  it("keeps every field a full file gives it", () => {
    const stored = {
      fontFamily: "Cascadia Mono, monospace",
      fontSize: 16,
      scrollback: 9000,
      cursorStyle: "bar",
      cursorBlink: false,
      defaultShell: "pwsh",
      defaultCwd: "C:\\work",
      rightClickPastes: true,
      titleShowsTargetName: true,
    };
    expect(sanitizeSettings(stored)).toEqual(stored);
  });

  /* The user can edit this file by hand, and an unknown value here is a terminal that cannot draw
     its cursor, not a log line. */
  it("refuses a cursor style xterm does not have", () => {
    expect(sanitizeSettings({ cursorStyle: "spiral" }).cursorStyle).toBe(
      DEFAULT_SETTINGS.cursorStyle,
    );
  });

  it("refuses junk in every other field too", () => {
    const settings = sanitizeSettings({
      fontFamily: "   ",
      fontSize: "abc",
      scrollback: -5,
      cursorBlink: "yes",
      defaultShell: "",
      defaultCwd: 42,
      rightClickPastes: "on",
      titleShowsTargetName: 1,
    });
    expect(settings.fontFamily).toBe(DEFAULT_FONT_FAMILY);
    expect(settings.fontSize).toBe(DEFAULT_FONT_SIZE);
    expect(settings.scrollback).toBe(MIN_SCROLLBACK);
    expect(settings.cursorBlink).toBe(DEFAULT_SETTINGS.cursorBlink);
    expect(settings.defaultShell).toBeNull();
    expect(settings.defaultCwd).toBeNull();
    expect(settings.rightClickPastes).toBe(false);
    expect(settings.titleShowsTargetName).toBe(DEFAULT_SETTINGS.titleShowsTargetName);
  });
});

describe("withLegacyFontSize", () => {
  /* The previous round kept the font size in localStorage. A user who adjusted it must not see the
     screen jump back to the default just because where it is stored changed. */
  it("takes the old localStorage size when the file has none", () => {
    expect(withLegacyFontSize(undefined, "20").fontSize).toBe(20);
    expect(withLegacyFontSize({ scrollback: 8000 }, "20").fontSize).toBe(20);
  });

  it("leaves the file alone once the file has a size of its own", () => {
    expect(withLegacyFontSize({ fontSize: 11 }, "20").fontSize).toBe(11);
  });

  it("clamps the old value like any other", () => {
    expect(withLegacyFontSize({}, "900").fontSize).toBe(32);
  });

  it("ignores an old value that is not a number, or none at all", () => {
    expect(withLegacyFontSize({}, "abc").fontSize).toBe(DEFAULT_FONT_SIZE);
    expect(withLegacyFontSize({}, null).fontSize).toBe(DEFAULT_FONT_SIZE);
  });

  it("carries the rest of the file through untouched", () => {
    expect(withLegacyFontSize({ scrollback: 8000 }, "20").scrollback).toBe(8000);
  });
});
