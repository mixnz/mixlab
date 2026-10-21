import { describe, expect, it } from "vitest";

import { leadKeys } from "./lead";

describe("leadKeys", () => {
  it("promises one prompt only where the window can raise one", () => {
    expect(leadKeys({ canPrompt: true, terminalWaits: false, anyMarked: false })).toEqual([
      "mixengine.elevation.lead",
    ]);
  });

  /* where no prompt can be raised, "changed in one prompt" was not true. */
  it("says the window cannot ask where it cannot", () => {
    expect(leadKeys({ canPrompt: false, terminalWaits: false, anyMarked: false })).toEqual([
      "mixengine.elevation.leadCannotPrompt",
    ]);
  });

  /* "anything marked below" only when something below is marked. */
  it("mentions marked rows only when there are some", () => {
    expect(leadKeys({ canPrompt: true, terminalWaits: true, anyMarked: false })).toEqual([
      "mixengine.elevation.leadWhileTerminalWaits",
    ]);
    expect(leadKeys({ canPrompt: false, terminalWaits: true, anyMarked: true })).toEqual([
      "mixengine.elevation.leadWhileTerminalWaits",
      "mixengine.elevation.leadMarkedWaitForScreen",
    ]);
  });
});
