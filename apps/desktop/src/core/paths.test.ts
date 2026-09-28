import { describe, expect, it } from "vitest";

import { fileName, joinPath, nativePath } from "./paths";

describe("nativePath", () => {
  it("respells forward slashes on Windows", () => {
    expect(nativePath("C:\\Users\\dev\\blog/public/assets", "windows")).toBe(
      "C:\\Users\\dev\\blog\\public\\assets",
    );
  });

  it("leaves a Unix path alone, backslash included — there it is a character of a name", () => {
    expect(nativePath("/srv/blog/a\\b", "posix")).toBe("/srv/blog/a\\b");
  });
});

describe("joinPath", () => {
  it("joins with the system's separator", () => {
    expect(joinPath("C:\\bulk", "runtimes", "windows")).toBe("C:\\bulk\\runtimes");
    expect(joinPath("/Volumes/SSD/bulk", "runtimes", "posix")).toBe("/Volumes/SSD/bulk/runtimes");
  });

  it("respells both halves on Windows", () => {
    expect(joinPath("D:/bulk", "public/assets", "windows")).toBe("D:\\bulk\\public\\assets");
  });

  it("never doubles a separator", () => {
    expect(joinPath("C:\\bulk\\", "\\runtimes", "windows")).toBe("C:\\bulk\\runtimes");
    expect(joinPath("/srv/", "/public", "posix")).toBe("/srv/public");
  });

  it("keeps a filesystem root rooted", () => {
    expect(joinPath("/", "public", "posix")).toBe("/public");
    expect(joinPath("C:\\", "public", "windows")).toBe("C:\\public");
  });
});

describe("fileName", () => {
  it("takes the last segment whichever separator wrote it", () => {
    expect(fileName("C:\\data\\shop.sqlite")).toBe("shop.sqlite");
    expect(fileName("/home/me/shop.sqlite")).toBe("shop.sqlite");
    expect(fileName("C:\\data/mixed\\shop.sqlite")).toBe("shop.sqlite");
  });

  it("falls back to the whole path rather than an empty title", () => {
    expect(fileName("C:\\data\\")).toBe("data");
    expect(fileName("\\")).toBe("\\");
    expect(fileName("  ")).toBe("");
  });
});
