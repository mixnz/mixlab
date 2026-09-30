import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { PREVIEW_SCHEME } from "./api";

const read = (path: string) => readFileSync(new URL(path, import.meta.url), "utf8");
const PREVIEW_RS = read("../../../src-tauri/src/modules/rest/preview.rs");
const CONFIG = read("../../../src-tauri/tauri.conf.json");

/** The three places the preview's scheme is named agree — the pane is blank on the first that does not. */
describe("the REST preview scheme", () => {
  it("is MixLab's", () => {
    expect(PREVIEW_SCHEME).toBe("mixlab-preview");
  });

  it("is the one the Rust side registers", () => {
    expect(PREVIEW_RS).toContain(`pub const SCHEME: &str = "${PREVIEW_SCHEME}";`);
  });

  it("is framed by both policies, in both of its forms", () => {
    const security = JSON.parse(CONFIG).app.security;
    for (const policy of [security.csp, security.devCsp]) {
      const frameSrc = String(policy)
        .split(";")
        .find((part) => part.trim().startsWith("frame-src"));
      expect(frameSrc).toContain(`${PREVIEW_SCHEME}:`);
      expect(frameSrc).toContain(`http://${PREVIEW_SCHEME}.localhost`);
    }
  });
});
