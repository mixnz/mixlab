import { describe, expect, it } from "vitest";

import {
  applySharingChange,
  canEditSite,
  formatRemaining,
  joinDocRoot,
  relativeToRoot,
  siteUrl,
  siteVisit,
  type SiteRow,
} from "./siteState";

describe("canEditSite", () => {
  it("a project-owned site can be edited", () => {
    expect(canEditSite({ type: "project", name: "blog" })).toBe(true);
  });

  it("an extension-owned site cannot", () => {
    expect(canEditSite({ type: "extension", id: "ext.mailhog" })).toBe(false);
  });
});

describe("applySharingChange", () => {
  const rows: SiteRow[] = [
    { domain: "blog.test", sharing: { until: null } } as unknown as SiteRow,
    { domain: "shop.test", sharing: null } as unknown as SiteRow,
  ];

  it("ignores events of another type", () => {
    const raw = JSON.stringify({ type: "resync", missed: 1 });
    expect(applySharingChange(rows, raw)).toBe(rows);
  });

  it("clears sharing on the row named by the event, leaves the other alone", () => {
    const raw = JSON.stringify({
      type: "site_sharing_changed",
      domain: "blog.test",
      sharing: null,
      because: { kind: "expired" },
    });
    const next = applySharingChange(rows, raw);
    expect(next.find((r) => r.domain === "blog.test")?.sharing).toBeNull();
    expect(next.find((r) => r.domain === "shop.test")).toBe(rows[1]);
  });

  it("a garbage payload does not throw", () => {
    expect(() => applySharingChange(rows, "not json")).not.toThrow();
    expect(applySharingChange(rows, "not json")).toBe(rows);
  });
});

describe("formatRemaining", () => {
  const now = 1_000_000;

  it("shows minutes and seconds under an hour", () => {
    expect(formatRemaining(now + 65_000, now)).toBe("01:05");
  });

  it("shows hours once there is more than one", () => {
    expect(formatRemaining(now + 3_661_000, now)).toBe("01:01:01");
  });

  it("clamps a past deadline to zero rather than going negative", () => {
    expect(formatRemaining(now - 5_000, now)).toBe("00:00");
  });
});

describe("relativeToRoot", () => {
  const root = "/Volumes/SSD/www/mixengine-test/demo.test";

  it("strips the project root and its separator", () => {
    expect(relativeToRoot(root, `${root}/public`)).toBe("public");
  });

  it("is empty when the picked folder is the root itself", () => {
    expect(relativeToRoot(root, root)).toBe("");
  });

  /* Root chọn kèm dấu / cuối vẫn phải cắt đúng, không để lại một dấu / thừa ở đầu kết quả. */
  it("tolerates a trailing separator on the root", () => {
    expect(relativeToRoot(`${root}/`, `${root}/public`)).toBe("public");
  });

  it("keeps a nested path's own separators", () => {
    expect(relativeToRoot(root, `${root}/public/assets`)).toBe("public/assets");
  });

  /* Không nằm dưới root — SiteCreate.doc_root chấp nhận cả tuyệt đối, giữ nguyên thay vì đoán. */
  it("keeps a path outside the root as-is", () => {
    expect(relativeToRoot(root, "/somewhere/else")).toBe("/somewhere/else");
  });

  it("answers in Windows spelling whichever separator the dialog used", () => {
    const winRoot = "C:\\Users\\dev\\blog";
    expect(relativeToRoot(winRoot, `${winRoot}\\public\\assets`, "windows")).toBe("public\\assets");
    expect(relativeToRoot(winRoot, `${winRoot}/public/assets`, "windows")).toBe("public\\assets");
  });
});

describe("joinDocRoot", () => {
  const root = "/Volumes/SSD/www/mixengine-test/demo.test";

  it("is just the root when the relative part is empty", () => {
    expect(joinDocRoot(root, "")).toBe(root);
  });

  it("joins root and the relative part with exactly one separator", () => {
    expect(joinDocRoot(root, "public")).toBe(`${root}/public`);
  });

  it("tolerates a trailing separator on the root", () => {
    expect(joinDocRoot(`${root}/`, "public")).toBe(`${root}/public`);
  });

  it("joins with a backslash on Windows, the doc root included", () => {
    expect(joinDocRoot("C:\\Users\\dev\\blog", "public\\assets", "windows")).toBe(
      "C:\\Users\\dev\\blog\\public\\assets",
    );
    expect(joinDocRoot("C:\\Users\\dev\\blog", "public/assets", "windows")).toBe(
      "C:\\Users\\dev\\blog\\public\\assets",
    );
  });
});

describe("siteUrl", () => {
  it("uses https when the site declares it", () => {
    expect(siteUrl({ domain: "blog.test", https: true })).toBe("https://blog.test");
  });

  it("and plain http when it does not", () => {
    expect(siteUrl({ domain: "blog.test", https: false })).toBe("http://blog.test");
  });
});

describe("siteVisit", () => {
  it("starts the owning project's services, then opens the site", () => {
    expect(
      siteVisit({
        domain: "blog.test",
        https: true,
        owner: { type: "project", name: "blog" },
      }),
    ).toEqual({ startProject: "blog", url: "https://blog.test" });
  });

  /* An extension's site has no project, and `service.start` is asked for one by name — so the
     start is skipped rather than guessed at. Whatever serves it is already somebody else's. */
  it("opens an extension's site without starting anything", () => {
    expect(
      siteVisit({
        domain: "mail.test",
        https: false,
        owner: { type: "extension", id: "mailpit" },
      }),
    ).toEqual({ startProject: null, url: "http://mail.test" });
  });
});
