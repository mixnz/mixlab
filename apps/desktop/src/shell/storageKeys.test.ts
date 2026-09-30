import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { describe, expect, it } from "vitest";
import { ACCENT_KEY, LANGUAGE_KEY, MODULES_KEY, SESSION_KEY, THEME_KEY } from "./storageKeys";

const read = (path: string) => readFileSync(new URL(path, import.meta.url), "utf8");
const SCRIPT = read("../../public/storage-keys.js");
const INDEX_HTML = read("../../index.html");
const TRAY_HTML = read("../../tray.html");

/** Runs public/storage-keys.js, as a page would, against `seed`; answers what storage holds after. */
function run(seed: Record<string, string>): Map<string, string> {
  const data = new Map(Object.entries(seed));
  const localStorage = {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => void data.set(key, value),
    removeItem: (key: string) => void data.delete(key),
  };
  runInNewContext(SCRIPT, { localStorage });
  return data;
}

const OLD = ["mixdb-theme", "mixdb-accent", "mixdb-lang", "mixdb-modules", "mixdb-session"];

describe("storage-keys.js", () => {
  it("moves an old key to its new name and removes the old one", () => {
    expect([...run({ "mixdb-theme": "dark" })]).toEqual([[THEME_KEY, "dark"]]);
  });

  it("keeps the new value when both are present, and still removes the old", () => {
    const after = run({ "mixdb-lang": "en", [LANGUAGE_KEY]: "vi" });
    expect([...after]).toEqual([[LANGUAGE_KEY, "vi"]]);
  });

  it("writes nothing when there is nothing to move", () => {
    expect([...run({})]).toEqual([]);
  });

  it("changes nothing on a second run", () => {
    const once = run({ "mixdb-session": "{}", "mixdb-accent": "red" });
    expect(run(Object.fromEntries(once))).toEqual(once);
  });

  it("leaves unrelated keys and the two retiring ones alone", () => {
    const seed = { other: "1", "mixdb-glass": "on", "mixdb-terminal-font-size": "15" };
    expect(Object.fromEntries(run(seed))).toEqual(seed);
  });

  it("writes exactly the names the modules read", () => {
    const after = run(Object.fromEntries(OLD.map((key) => [key, key])));
    expect([...after.keys()].sort()).toEqual(
      [THEME_KEY, ACCENT_KEY, LANGUAGE_KEY, MODULES_KEY, SESSION_KEY].sort(),
    );
  });
});

describe("the pages", () => {
  for (const [page, html] of [
    ["index.html", INDEX_HTML],
    ["tray.html", TRAY_HTML],
  ]) {
    it(`${page} loads storage-keys.js before theme-preload.js`, () => {
      const keys = html.indexOf('src="/storage-keys.js"');
      const preload = html.indexOf('src="/theme-preload.js"');
      expect(keys).toBeGreaterThan(-1);
      expect(preload).toBeGreaterThan(keys);
    });
  }
});
