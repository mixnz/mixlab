import { describe, expect, it } from "vitest";
import { resolve, translator } from "./index";
import { EN, VI } from "./dicts";

describe("resolve", () => {
  it("reads a nested key", () => {
    expect(resolve(EN, "common.save")).toBe(EN.common.save);
  });

  it("returns the key itself when the dictionary doesn't have it", () => {
    // Documented behaviour, per docs/standards/desktop/i18n.md: "An unknown key resolves to the key
    // string itself rather than throwing."
    expect(resolve(EN, "no.such.key" as never)).toBe("no.such.key");
  });

  it("does not throw when key is not a string", () => {
    // The real failure this guards: a Record<SomeUnion, TranslationKey> indexed by a value read
    // from disk (e.g. KIND_LABEL[someUnknownKind], before kindLabel() existed) can return
    // undefined, and undefined ends up here.
    expect(() => resolve(EN, undefined as never)).not.toThrow();
  });
});

/* What `t` and `useLanguageT` both say: one lookup, bound to one dictionary. */
describe("translator", () => {
  it("reads the dictionary it was built from", () => {
    expect(translator(EN)("common.save")).toBe(EN.common.save);
    expect(translator(VI)("common.save")).toBe(VI.common.save);
  });

  it("fills a template's variables", () => {
    expect(translator(EN)("update.available", { version: "1.2.3" })).toBe("MixLab 1.2.3 is available.");
  });

  it("leaves a variable nobody gave in its braces", () => {
    expect(translator(EN)("update.available")).toBe("MixLab {{version}} is available.");
    expect(translator(EN)("update.available", { other: 1 })).toBe("MixLab {{version}} is available.");
  });
});
