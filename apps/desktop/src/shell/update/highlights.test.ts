import { describe, expect, it } from "vitest";
import { highlights } from "./highlights";

describe("highlights", () => {
  it("takes the bullets and skips the headings", () =>
    expect(
      highlights("### Added\n\n- A panel in the corner\n- `mix` learns **bold**\n\n### Fixed\n* A [link](https://x) fix"),
    ).toEqual(["A panel in the corner", "mix learns bold", "A link fix"]));

  it("joins a bullet that wraps onto the next line", () =>
    expect(highlights("- The first entry\n  continues here\n- The second")).toEqual([
      "The first entry continues here",
      "The second",
    ]));

  it("falls back to the prose when there are no bullets", () =>
    expect(highlights("## 0.0.10\n\nA quiet release.\nOne fix.")).toEqual(["A quiet release. One fix."]));

  it("reads Windows line endings", () =>
    expect(highlights("### Added\r\n- One\r\n- Two\r\n")).toEqual(["One", "Two"]));

  it("cuts a long entry at 120 characters", () => {
    const [entry] = highlights(`- ${"x".repeat(200)}`);
    expect(entry).toHaveLength(120);
    expect(entry.endsWith("…")).toBe(true);
  });

  it("gives nothing for empty notes", () => expect(highlights("")).toEqual([]));
});
