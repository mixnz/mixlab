import { describe, expect, it } from "vitest";
import type { ProjectDetail } from "@mixengine/api";
import { detailRead, NO_ADDRESSES, opened, stillListed } from "./projectsScreen";

const detailOf = (name: string) => ({ project: { name }, pins: [] }) as unknown as ProjectDetail;
const ADDRESSES = { first: "https://blog.test", byDomain: {} };

describe("an open project row", () => {
  it("opens with nothing read yet", () => {
    expect(opened("blog")).toEqual({ name: "blog", detail: null, addresses: NO_ADDRESSES });
  });

  it("takes the detail read for it", () => {
    expect(detailRead(opened("blog"), "blog", detailOf("blog"), ADDRESSES)).toEqual({
      name: "blog",
      detail: detailOf("blog"),
      addresses: ADDRESSES,
    });
  });

  it("ignores a detail that lands after another row was opened", () => {
    const blog = opened("blog");
    expect(detailRead(blog, "acme-shop", detailOf("acme-shop"), ADDRESSES)).toBe(blog);
    expect(detailRead(null, "acme-shop", detailOf("acme-shop"), ADDRESSES)).toBeNull();
  });

  it("closes when its project is no longer listed", () => {
    const blog = opened("blog");
    expect(stillListed(blog, ["blog", "acme-shop"])).toBe(blog);
    expect(stillListed(blog, ["acme-shop"])).toBeNull();
    expect(stillListed(null, ["blog"])).toBeNull();
  });
});
