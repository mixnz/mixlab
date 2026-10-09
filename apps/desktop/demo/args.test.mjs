import { describe, expect, it } from "vitest";
import { COLOR_THEMES } from "../src/shell/themeModel";
import { modifierFor, parseArgs, parseClipArgs, storageFor } from "./args.mjs";

const IDS = ["hero", "sites", "database"];

describe("parseArgs", () => {
  it("defaults to every scene, both themes, macOS, a full run that writes images", () => {
    expect(parseArgs([], IDS)).toEqual({
      check: false,
      full: true,
      scenes: IDS,
      themes: ["dark", "light"],
      platform: "mac",
    });
  });

  it("narrows by scene and theme, which makes the run partial", () => {
    const options = parseArgs(["--check", "--scene", "hero,sites", "--theme", "light"], IDS);
    expect(options).toMatchObject({ check: true, full: false, scenes: ["hero", "sites"], themes: ["light"] });
  });

  it("refuses what it does not know, naming it", () => {
    expect(() => parseArgs(["--scene", "nope"], IDS)).toThrow('unknown scene "nope"');
    expect(() => parseArgs(["--platform", "beos"], IDS)).toThrow('unknown platform "beos"');
    expect(() => parseArgs(["--theme"], IDS)).toThrow("--theme needs a value");
    expect(() => parseArgs(["--fast"], IDS)).toThrow("unknown argument --fast");
  });
});

describe("storageFor", () => {
  it("seeds a window that skips first run and restores one active tab", () => {
    const scene = { id: "hero", moduleId: "mixengine", tabTitle: "MixEngine", state: { screen: "dashboard" } };
    const storage = storageFor(scene, "light");
    expect(JSON.parse(storage["mixlab-modules"])).toEqual(["mixengine", "db", "rest", "terminal", "tools", "tunnel"]);
    expect(storage["mixlab-theme"]).toBe("light");
    expect(storage["mixlab-palette"]).toBeUndefined();
    expect(storage["mixlab-lang"]).toBe("en");
    expect(JSON.parse(storage["mixlab-session"])).toEqual({
      tabs: [{ id: "demo-hero", moduleId: "mixengine", title: "MixEngine", state: { screen: "dashboard" } }],
      activeId: "demo-hero",
    });
  });
});

describe("storageFor in dark", () => {
  it("films the first colour theme", () => {
    const storage = storageFor({ id: "hero", moduleId: "mixengine", tabTitle: "MixEngine" }, "dark");
    expect(storage["mixlab-theme"]).toBe("color");
    expect(storage["mixlab-palette"]).toBe(COLOR_THEMES[0]);
  });
});

describe("modifierFor", () => {
  it("is Meta on macOS and Control elsewhere", () => {
    expect(modifierFor("mac")).toBe("Meta");
    expect(modifierFor("windows")).toBe("Control");
    expect(modifierFor("linux")).toBe("Control");
  });
});

describe("parseClipArgs", () => {
  const CLIPS = ["new-site"];

  it("defaults to every clip, both themes, macOS, a full run", () => {
    expect(parseClipArgs([], CLIPS)).toEqual({
      check: false,
      full: true,
      clips: CLIPS,
      themes: ["dark", "light"],
      platform: "mac",
    });
  });

  it("narrows by clip and theme", () => {
    expect(parseClipArgs(["--clip", "new-site", "--theme", "dark"], CLIPS)).toMatchObject({
      full: false,
      clips: ["new-site"],
      themes: ["dark"],
    });
  });

  it("names what it does not know, and the command it belongs to", () => {
    expect(() => parseClipArgs(["--clip", "nope"], CLIPS)).toThrow('unknown clip "nope"');
    expect(() => parseClipArgs(["--scene", "hero"], CLIPS)).toThrow("Usage: npm run clips");
  });
});
