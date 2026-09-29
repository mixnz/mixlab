import { describe, expect, it } from "vitest";
import { allZones, canonicalZone, preferredZone, zoneOffset } from "./zones";

describe("canonicalZone", () => {
  it("turns old IANA names into current names", () => {
    expect(canonicalZone("Asia/Saigon")).toBe("Asia/Ho_Chi_Minh");
    expect(canonicalZone("Asia/Calcutta")).toBe("Asia/Kolkata");
    expect(canonicalZone("Europe/Kiev")).toBe("Europe/Kyiv");
    expect(canonicalZone("America/Buenos_Aires")).toBe("America/Argentina/Buenos_Aires");
    expect(canonicalZone("Asia/Rangoon")).toBe("Asia/Yangon");
  });

  it("leaves canonical names alone", () => {
    expect(canonicalZone("Asia/Ho_Chi_Minh")).toBe("Asia/Ho_Chi_Minh");
    expect(canonicalZone("Europe/London")).toBe("Europe/London");
    expect(canonicalZone("UTC")).toBe("UTC");
  });
});

describe("allZones", () => {
  const zones = allZones();

  it("uses canonical names, no old ones left", () => {
    expect(zones).toContain("Asia/Ho_Chi_Minh");
    expect(zones).not.toContain("Asia/Saigon");
    expect(zones).not.toContain("Asia/Calcutta");
    expect(zones).not.toContain("Europe/Kiev");
  });

  it("has no duplicates, and is sorted", () => {
    expect(new Set(zones).size).toBe(zones.length);
    expect([...zones].sort()).toEqual(zones);
  });

  it("every name in the list works with Intl", () => {
    for (const zone of zones) {
      expect(() => new Intl.DateTimeFormat("en", { timeZone: zone }).format(0)).not.toThrow();
    }
  });
});

describe("zoneOffset", () => {
  const at = Date.parse("2026-01-15T00:00:00Z");

  it("prints the offset from UTC", () => {
    expect(zoneOffset("Asia/Ho_Chi_Minh", at)).toBe("+07:00");
    expect(zoneOffset("UTC", at)).toBe("+00:00");
  });

  it("keeps half-hour and 45-minute fractions", () => {
    expect(zoneOffset("Asia/Kolkata", at)).toBe("+05:30");
    expect(zoneOffset("Asia/Kathmandu", at)).toBe("+05:45");
  });

  it("prints a minus sign for the west", () => {
    expect(zoneOffset("America/New_York", at)).toBe("-05:00");
  });

  it("follows the season rather than being fixed — one zone, two moments, two offsets", () => {
    const summer = Date.parse("2026-07-15T00:00:00Z");
    expect(zoneOffset("America/New_York", summer)).toBe("-04:00");
  });

  it("does not fall over on a zone that does not exist", () => {
    expect(zoneOffset("Khong/Co_That", at)).toBe("");
  });
});

describe("preferredZone", () => {
  const at = Date.parse("2026-01-15T00:00:00Z");

  /* Windows only has `SE Asia Standard Time` for Bangkok, Hanoi and Jakarta alike, and ICU maps
     that raw ID to `Asia/Bangkok`. A machine set to Vietnamese therefore defaults to Bangkok — the
     same +07:00 so it does not show, yet still the wrong country. */
  it("switches to the locale country's zone when the machine's zone is not in that country", () => {
    expect(preferredZone("Asia/Bangkok", ["vi-VN"], at)).toBe("Asia/Ho_Chi_Minh");
  });

  it("leaves it alone when the machine's zone already belongs to that country", () => {
    expect(preferredZone("Asia/Bangkok", ["th-TH"], at)).toBe("Asia/Bangkok");
    expect(preferredZone("America/New_York", ["en-US"], at)).toBe("America/New_York");
    expect(preferredZone("Europe/Berlin", ["de-DE"], at)).toBe("Europe/Berlin");
  });

  /* The condition is that the offsets must match: a machine set to London with a Vietnamese locale
     is someone who really is in London, not a machine Windows mapped to the wrong zone. */
  it("does not switch when the offsets differ", () => {
    expect(preferredZone("Europe/London", ["vi-VN"], at)).toBe("Europe/London");
  });

  it("leaves it alone when the locale names no country", () => {
    expect(preferredZone("Asia/Bangkok", ["vi"], at)).toBe("Asia/Bangkok");
    expect(preferredZone("Asia/Bangkok", [""], at)).toBe("Asia/Bangkok");
  });

  it("does not fall over on a broken locale", () => {
    expect(preferredZone("Asia/Bangkok", ["khong-phai-locale!!"], at)).toBe("Asia/Bangkok");
  });

  /* This is the real situation on WebView2: `resolvedOptions().locale` follows the webview's
     display language and gives `en-US`, while the user's real region only shows in
     `navigator.languages`. */
  it("moves on to the next source when the first cannot help", () => {
    expect(preferredZone("Asia/Bangkok", ["en-US", "vi-VN"], at)).toBe("Asia/Ho_Chi_Minh");
  });

  it("skips empty and broken tags in the middle of the list", () => {
    expect(preferredZone("Asia/Bangkok", ["", "hong!!", "vi", "vi-VN"], at)).toBe(
      "Asia/Ho_Chi_Minh",
    );
  });

  /* The first source has already confirmed the machine's zone is in the right country, so a later
     source must not overturn it: a Thai machine whose `navigator.languages` includes Vietnamese
     still has to stay in Bangkok. */
  it("stops as soon as a source confirms the machine's zone is in the right country", () => {
    expect(preferredZone("Asia/Bangkok", ["th-TH", "vi-VN"], at)).toBe("Asia/Bangkok");
  });

  it("keeps it as is when no source can name a country", () => {
    expect(preferredZone("Asia/Bangkok", [], at)).toBe("Asia/Bangkok");
  });

  it("always returns a zone that is in the list", () => {
    const zones = allZones();
    for (const locale of ["vi-VN", "th-TH", "en-US", "de-DE", "ja-JP"]) {
      expect(zones).toContain(preferredZone("Asia/Bangkok", [locale], at));
    }
  });
});
