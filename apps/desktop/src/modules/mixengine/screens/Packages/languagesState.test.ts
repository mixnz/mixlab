import { describe, expect, it } from "vitest";
import type { RuntimeSummary } from "@mixengine/api";
import { EMPTY_CATALOGUE, EMPTY_LOCAL, forceAsked, readAnswered, type LanguagesDialog } from "./languagesState";

const PHP = { kind: "php", version: "8.4.1" } as RuntimeSummary;

describe("readAnswered", () => {
  it("is the empty answer when a first read fails, so the spinner goes", () => {
    expect(readAnswered(null, EMPTY_LOCAL)).toBe(EMPTY_LOCAL);
    expect(readAnswered(null, EMPTY_CATALOGUE)).toEqual({ available: [], stale: false, unavailable: [], updates: [] });
  });

  it("keeps what an earlier read said when a later one fails", () => {
    const had = { installed: [PHP], onDisk: [] };
    expect(readAnswered(had, EMPTY_LOCAL)).toBe(had);
  });
});

describe("forceAsked", () => {
  it("puts the daemon's sentence in the uninstall question that is up", () => {
    const asked: LanguagesDialog = { kind: "uninstall", target: PHP, forceHint: null };
    expect(forceAsked(asked, "blog pins php 8.4.1")).toEqual({ kind: "uninstall", target: PHP, forceHint: "blog pins php 8.4.1" });
  });

  it("leaves any other dialog, and none, as they are", () => {
    const other: LanguagesDialog = { kind: "removeDevkit", version: "3.3" };
    expect(forceAsked(other, "x")).toBe(other);
    expect(forceAsked(null, "x")).toBeNull();
  });
});
