import { describe, expect, it } from "vitest";
import { allZones, canonicalZone, preferredZone, zoneOffset } from "./zones";

describe("canonicalZone", () => {
  it("đổi tên cũ của IANA sang tên hiện hành", () => {
    expect(canonicalZone("Asia/Saigon")).toBe("Asia/Ho_Chi_Minh");
    expect(canonicalZone("Asia/Calcutta")).toBe("Asia/Kolkata");
    expect(canonicalZone("Europe/Kiev")).toBe("Europe/Kyiv");
    expect(canonicalZone("America/Buenos_Aires")).toBe("America/Argentina/Buenos_Aires");
    expect(canonicalZone("Asia/Rangoon")).toBe("Asia/Yangon");
  });

  it("để nguyên tên đã chuẩn", () => {
    expect(canonicalZone("Asia/Ho_Chi_Minh")).toBe("Asia/Ho_Chi_Minh");
    expect(canonicalZone("Europe/London")).toBe("Europe/London");
    expect(canonicalZone("UTC")).toBe("UTC");
  });
});

describe("allZones", () => {
  const zones = allZones();

  it("dùng tên chuẩn, không còn tên cũ", () => {
    expect(zones).toContain("Asia/Ho_Chi_Minh");
    expect(zones).not.toContain("Asia/Saigon");
    expect(zones).not.toContain("Asia/Calcutta");
    expect(zones).not.toContain("Europe/Kiev");
  });

  it("không có mục trùng, và đã sắp xếp", () => {
    expect(new Set(zones).size).toBe(zones.length);
    expect([...zones].sort()).toEqual(zones);
  });

  it("mọi tên trong danh sách đều dùng được với Intl", () => {
    for (const zone of zones) {
      expect(() => new Intl.DateTimeFormat("en", { timeZone: zone }).format(0)).not.toThrow();
    }
  });
});

describe("zoneOffset", () => {
  const at = Date.parse("2026-01-15T00:00:00Z");

  it("in ra chênh lệch so với UTC", () => {
    expect(zoneOffset("Asia/Ho_Chi_Minh", at)).toBe("+07:00");
    expect(zoneOffset("UTC", at)).toBe("+00:00");
  });

  it("giữ được phần lẻ nửa tiếng và 45 phút", () => {
    expect(zoneOffset("Asia/Kolkata", at)).toBe("+05:30");
    expect(zoneOffset("Asia/Kathmandu", at)).toBe("+05:45");
  });

  it("in dấu âm cho phía tây", () => {
    expect(zoneOffset("America/New_York", at)).toBe("-05:00");
  });

  it("theo mùa chứ không cố định — cùng một vùng, hai thời điểm, hai chênh lệch", () => {
    const summer = Date.parse("2026-07-15T00:00:00Z");
    expect(zoneOffset("America/New_York", summer)).toBe("-04:00");
  });

  it("không ngã với một vùng không có thật", () => {
    expect(zoneOffset("Khong/Co_That", at)).toBe("");
  });
});

describe("preferredZone", () => {
  const at = Date.parse("2026-01-15T00:00:00Z");

  /* Windows only has `SE Asia Standard Time` for Bangkok, Hanoi and Jakarta alike, and ICU maps
     that raw ID to `Asia/Bangkok`. A machine set to Vietnamese therefore defaults to Bangkok — the
     same +07:00 so it does not show, yet still the wrong country. */
  it("đổi sang vùng của nước trong locale khi vùng của máy không thuộc nước đó", () => {
    expect(preferredZone("Asia/Bangkok", ["vi-VN"], at)).toBe("Asia/Ho_Chi_Minh");
  });

  it("để nguyên khi vùng của máy vốn đã thuộc nước đó", () => {
    expect(preferredZone("Asia/Bangkok", ["th-TH"], at)).toBe("Asia/Bangkok");
    expect(preferredZone("America/New_York", ["en-US"], at)).toBe("America/New_York");
    expect(preferredZone("Europe/Berlin", ["de-DE"], at)).toBe("Europe/Berlin");
  });

  /* The condition is that the offsets must match: a machine set to London with a Vietnamese locale
     is someone who really is in London, not a machine Windows mapped to the wrong zone. */
  it("không đổi khi chênh lệch không trùng", () => {
    expect(preferredZone("Europe/London", ["vi-VN"], at)).toBe("Europe/London");
  });

  it("để nguyên khi locale không nói nước nào", () => {
    expect(preferredZone("Asia/Bangkok", ["vi"], at)).toBe("Asia/Bangkok");
    expect(preferredZone("Asia/Bangkok", [""], at)).toBe("Asia/Bangkok");
  });

  it("không ngã với locale hỏng", () => {
    expect(preferredZone("Asia/Bangkok", ["khong-phai-locale!!"], at)).toBe("Asia/Bangkok");
  });

  /* This is the real situation on WebView2: `resolvedOptions().locale` follows the webview's
     display language and gives `en-US`, while the user's real region only shows in
     `navigator.languages`. */
  it("đi tiếp xuống nguồn sau khi nguồn đầu không cứu được", () => {
    expect(preferredZone("Asia/Bangkok", ["en-US", "vi-VN"], at)).toBe("Asia/Ho_Chi_Minh");
  });

  it("bỏ qua thẻ rỗng và thẻ hỏng giữa danh sách", () => {
    expect(preferredZone("Asia/Bangkok", ["", "hong!!", "vi", "vi-VN"], at)).toBe(
      "Asia/Ho_Chi_Minh",
    );
  });

  /* The first source has already confirmed the machine's zone is in the right country, so a later
     source must not overturn it: a Thai machine whose `navigator.languages` includes Vietnamese
     still has to stay in Bangkok. */
  it("dừng ngay khi một nguồn xác nhận vùng của máy là đúng nước", () => {
    expect(preferredZone("Asia/Bangkok", ["th-TH", "vi-VN"], at)).toBe("Asia/Bangkok");
  });

  it("giữ nguyên khi không nguồn nào nói được nước", () => {
    expect(preferredZone("Asia/Bangkok", [], at)).toBe("Asia/Bangkok");
  });

  it("luôn trả về một vùng có trong danh sách", () => {
    const zones = allZones();
    for (const locale of ["vi-VN", "th-TH", "en-US", "de-DE", "ja-JP"]) {
      expect(zones).toContain(preferredZone("Asia/Bangkok", [locale], at));
    }
  });
});
