import { describe, expect, it } from "vitest";

import { offerFrom } from "./previousCopy";

const copy = {
  path: "D:/mixlab_data/data/.mixengine-state.db",
  taken_at: 1790000000000,
  projects: 3,
  sites: 5,
  services: 2,
  runtimes: 4,
  packages: 1,
  newer: false,
};

describe("offerFrom", () => {
  it("offers what the daemon counted", () => {
    expect(offerFrom({ copy })).toEqual({ projects: 3, sites: 5, services: 2, restorable: true });
  });

  it("names a copy a newer build wrote, and does not offer to restore it", () => {
    expect(offerFrom({ copy: { ...copy, newer: true } })).toMatchObject({ restorable: false });
  });

  it("offers nothing when the daemon found no copy", () => {
    expect(offerFrom({})).toBeNull();
  });
});
