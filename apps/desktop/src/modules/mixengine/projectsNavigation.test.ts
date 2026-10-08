import { beforeEach, describe, expect, it } from "vitest";

import { requestProjectDetail, takePendingProjectDetail } from "./projectsNavigation";

// Module-level state — reset between tests since nothing else does.
beforeEach(() => {
  takePendingProjectDetail();
});

describe("projectsNavigation", () => {
  it("is null until a project is asked for", () => {
    expect(takePendingProjectDetail()).toBeNull();
  });

  it("hands back the project to open exactly once", () => {
    requestProjectDetail("shop");
    expect(takePendingProjectDetail()).toBe("shop");
    expect(takePendingProjectDetail()).toBeNull();
  });

  it("a later request replaces an earlier one nobody took yet", () => {
    requestProjectDetail("blog");
    requestProjectDetail("shop");
    expect(takePendingProjectDetail()).toBe("shop");
  });
});
