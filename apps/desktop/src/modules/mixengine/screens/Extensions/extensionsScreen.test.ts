import { describe, expect, it } from "vitest";
import type { ExtensionSummary } from "@mixengine/api";
import { deleteDataTicked, listsAnswered, EMPTY_LISTS, type ExtensionsDialog } from "./extensionsScreen";

const MAILPIT = { id: "mailpit", name: "Mailpit" } as ExtensionSummary;

describe("listsAnswered", () => {
  it("is the empty lists when the first read fails, and what was read before after that", () => {
    expect(listsAnswered(null)).toBe(EMPTY_LISTS);
    const had = { ...EMPTY_LISTS, installed: [MAILPIT] };
    expect(listsAnswered(had)).toBe(had);
  });
});

describe("deleteDataTicked", () => {
  it("is the uninstall question's own box", () => {
    const asked: ExtensionsDialog = { kind: "uninstall", row: MAILPIT, deleteData: false };
    expect(deleteDataTicked(asked, true)).toEqual({ kind: "uninstall", row: MAILPIT, deleteData: true });
  });

  it("leaves the install dialog, and none, alone", () => {
    const install: ExtensionsDialog = { kind: "install", source: { type: "registry", id: "minio" } };
    expect(deleteDataTicked(install, true)).toBe(install);
    expect(deleteDataTicked(null, true)).toBeNull();
  });
});
