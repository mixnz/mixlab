import { describe, expect, it } from "vitest";

import { resolve } from "../../../i18n";
import { EN, VI } from "../../../i18n/dicts";

/**
 * The tab's gate, when the daemon is not found, says where it looked — T111.
 *
 * `VI` is not typed against `EN` in `i18n/dicts.ts`, so a key present on one side and missing on
 * the other is something `tsc` says nothing about: `resolve` returns the key string itself, and a
 * Vietnamese user sees `mixengine.gate.lookedIn` in the middle of the screen. This is the only
 * place that says no.
 */
describe("the not-installed gate", () => {
  it("labels the list of directories in every language", () => {
    for (const dict of [EN, VI]) {
      const label = resolve(dict, "mixengine.gate.lookedIn");

      expect(label).not.toBe("mixengine.gate.lookedIn");
      expect(label.trim()).not.toBe("");
    }
  });

  /* The list is a `<ul>` and is no longer interpolated into the sentence, so the sentence must
     carry no placeholder: a leftover `{{searched}}` would be printed just like that, since nobody
     passes it a variable any more. */
  it("states the fault in a sentence with nothing left to interpolate", () => {
    for (const dict of [EN, VI]) {
      expect(resolve(dict, "mixengine.gate.notInstalled")).not.toContain("{{");
    }
  });
});
