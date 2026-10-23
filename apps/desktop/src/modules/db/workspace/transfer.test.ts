import { describe, expect, it } from "vitest";
import type { TransferProgress } from "../api";
import { busyChanged, progressHeard } from "./transfer";

const HALF = { id: "c", done: 50, total: 100 } as unknown as TransferProgress;

describe("busyChanged", () => {
  it("puts the overlay up with no reading yet", () => {
    expect(busyChanged(null, "Dumping shop…")).toEqual({ label: "Dumping shop…", progress: null });
  });

  it("keeps the reading when the label changes under a running transfer", () => {
    expect(busyChanged({ label: "a", progress: HALF }, "b")).toEqual({ label: "b", progress: HALF });
  });

  it("takes the overlay and its reading down together", () => {
    expect(busyChanged({ label: "a", progress: HALF }, "")).toBeNull();
  });
});

describe("progressHeard", () => {
  it("lands on the transfer that is up, and on nothing once it is over", () => {
    expect(progressHeard({ label: "a", progress: null }, HALF)).toEqual({ label: "a", progress: HALF });
    expect(progressHeard(null, HALF)).toBeNull();
  });
});
