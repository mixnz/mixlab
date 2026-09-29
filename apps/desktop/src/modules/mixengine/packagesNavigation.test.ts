import { beforeEach, describe, expect, it } from "vitest";

import {
  peekPendingLanguageFilter,
  requestLanguageFilter,
  takePendingLanguageFilter,
} from "./packagesNavigation";

// Module-level state — reset between tests since nothing else does.
beforeEach(() => {
  takePendingLanguageFilter();
});

describe("packagesNavigation", () => {
  it("is null until a filter is requested", () => {
    expect(peekPendingLanguageFilter()).toBeNull();
    expect(takePendingLanguageFilter()).toBeNull();
  });

  it("hands back a requested filter exactly once", () => {
    requestLanguageFilter("php");
    expect(takePendingLanguageFilter()).toBe("php");
    expect(takePendingLanguageFilter()).toBeNull();
  });

  it("peeking leaves the request for whoever takes it", () => {
    requestLanguageFilter("php");
    expect(peekPendingLanguageFilter()).toBe("php");
    expect(peekPendingLanguageFilter()).toBe("php");
    expect(takePendingLanguageFilter()).toBe("php");
  });

  it("a later request replaces an earlier one nobody took yet", () => {
    requestLanguageFilter("php");
    requestLanguageFilter("node");
    expect(takePendingLanguageFilter()).toBe("node");
  });
});
