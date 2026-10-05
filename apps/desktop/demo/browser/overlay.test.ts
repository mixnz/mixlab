import { readFileSync } from "node:fs";
import { describe, expect, it, vi } from "vitest";
import { installBrowserOverlay } from "./overlay";

const PAGE = readFileSync(new URL("./laravel-welcome.html", import.meta.url), "utf8");

describe("laravel-welcome.html", () => {
  it("is plain HTML: no Blade left, no font or script fetched", () => {
    for (const blade of ["@if", "@else", "@endif", "@vite", "@fonts", "@auth", "{{", "{!!"]) {
      expect(PAGE, blade).not.toContain(blade);
    }
    expect(PAGE).not.toMatch(/<link[^>]+href="https?:/);
    expect(PAGE).not.toMatch(/<script[^>]+src=/);
  });

  it("says where it came from and under which licence", () => {
    expect(PAGE).toContain("laravel/laravel");
    expect(PAGE).toContain("MIT");
    expect(PAGE).toContain("<title>Laravel</title>");
  });
});

describe("installBrowserOverlay", () => {
  it("listens for nothing unless the clip asked for a browser", () => {
    const win = { addEventListener: vi.fn() };
    installBrowserOverlay(win as unknown as Window);
    expect(win.addEventListener).not.toHaveBeenCalled();
  });

  it("waits for MixLab to open a URL when the clip asked for one", () => {
    const win = { __demoFixtures: { browser: true }, addEventListener: vi.fn() };
    installBrowserOverlay(win as unknown as Window);
    expect(win.addEventListener).toHaveBeenCalledWith("demo:open-url", expect.any(Function));
  });
});
