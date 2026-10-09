/**
 * The capture script's command line, and what each scene puts in `localStorage` before the app
 * loads. Pure, so both are unit-tested without a browser.
 */

/** The registry's module ids — `scenes.test.mjs` asserts this list still matches `MODULES`. */
export const MODULE_IDS = ["mixengine", "db", "rest", "terminal", "tools", "tunnel"];

export const THEMES = ["dark", "light"];

export const PLATFORMS = ["mac", "windows", "linux"];

/** `core/platform.ts` reads only "Mac OS X" and "Windows" out of these. */
export const USER_AGENTS = {
  mac: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
  windows:
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
  linux: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
};

function assertKnown(values, known, what) {
  for (const value of values) {
    if (!known.includes(value)) {
      throw new Error(`unknown ${what} "${value}"; expected one of ${known.join(", ")}`);
    }
  }
}

function parseRun(argv, ids, { flag, key, noun, command }) {
  const options = {
    check: false,
    full: true,
    [key]: [...ids],
    themes: [...THEMES],
    platform: "mac",
  };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--check") {
      options.check = true;
      continue;
    }
    if (arg === flag || arg === "--theme" || arg === "--platform") {
      const value = argv[i + 1];
      if (value === undefined || value.startsWith("--")) throw new Error(`${arg} needs a value`);
      i++;
      const values = value.split(",");
      if (arg === flag) {
        assertKnown(values, ids, noun);
        options[key] = values;
        options.full = false;
      } else if (arg === "--theme") {
        assertKnown(values, THEMES, "theme");
        options.themes = values;
        options.full = false;
      } else {
        assertKnown([value], PLATFORMS, "platform");
        options.platform = value;
      }
      continue;
    }
    throw new Error(
      `unknown argument ${arg}. Usage: npm run ${command} -- [--check] ` +
        `[${flag} ${ids.join(",")}] [--theme dark,light] [--platform mac|windows|linux]`,
    );
  }
  return options;
}

export function parseArgs(argv, sceneIds) {
  return parseRun(argv, sceneIds, { flag: "--scene", key: "scenes", noun: "scene", command: "screenshots" });
}

export function parseClipArgs(argv, clipIds) {
  return parseRun(argv, clipIds, { flag: "--clip", key: "clips", noun: "clip", command: "clips" });
}

/** What dark is filmed in: the first colour theme, navy, the dark MixLab had before the redesign
 *  (`COLOR_THEMES` in `shell/themeModel.ts`). */
export const DARK_PALETTE = "navy";

/** Everything the window reads from `localStorage` on the way up, for one scene. */
export function storageFor(scene, theme) {
  const tabId = `demo-${scene.id}`;
  const look =
    theme === "dark" ? { "mixlab-theme": "color", "mixlab-palette": DARK_PALETTE } : { "mixlab-theme": theme };
  return {
    "mixlab-modules": JSON.stringify(MODULE_IDS),
    ...look,
    "mixlab-lang": "en",
    "mixlab-session": JSON.stringify({
      tabs: [{ id: tabId, moduleId: scene.moduleId, title: scene.tabTitle, state: scene.state }],
      activeId: tabId,
    }),
  };
}

/** The key `hasPrimaryModifier` in `core/platform.ts` listens for under each user agent. */
export function modifierFor(platform) {
  return platform === "mac" ? "Meta" : "Control";
}
