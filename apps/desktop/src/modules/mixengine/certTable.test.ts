import { describe, expect, it } from "vitest";

import { buildCertRows, servedByDomain, servedCell } from "./certTable";
import type { CertIssueReport } from "@mixengine/api";
import type { SiteCert } from "@mixengine/api";
import type { SiteCertStatus } from "@mixengine/api";

describe("buildCertRows", () => {
  it("reads sans and days_left from a present certificate", () => {
    const report: CertIssueReport = {
      sites: [
        {
          domain: "blog.test",
          outcome: { outcome: "reused" },
          state: {
            state: "present",
            cert: {
              subject: "blog.test",
              sans: ["blog.test", "www.blog.test"],
              issuer: "MixEngine Local CA",
              fingerprint: "abc",
              not_before: 0,
              not_after: 1,
              days_left: 42,
            },
          },
        },
      ],
    };
    expect(buildCertRows(report)).toEqual([
      {
        domain: "blog.test",
        outcome: { outcome: "reused" },
        sans: ["blog.test", "www.blog.test"],
        daysLeft: 42,
      },
    ]);
  });

  it("a site with no certificate on disk has no sans and no days left", () => {
    const report: CertIssueReport = {
      sites: [
        {
          domain: "shop.test",
          outcome: { outcome: "not_wanted", because: "no https" },
          state: { state: "absent" },
        },
      ],
    };
    expect(buildCertRows(report)).toEqual([
      {
        domain: "shop.test",
        outcome: { outcome: "not_wanted", because: "no https" },
        sans: [],
        daysLeft: null,
      },
    ]);
  });

  it("an unusable certificate also has no sans and no days left", () => {
    const report: CertIssueReport = {
      sites: [
        {
          domain: "api.test",
          outcome: { outcome: "refused", because: "key mismatch" },
          state: { state: "unusable", because: "key_and_certificate_disagree" },
        },
      ],
    };
    expect(buildCertRows(report)[0].daysLeft).toBeNull();
    expect(buildCertRows(report)[0].sans).toEqual([]);
  });
});

describe("servedCell", () => {
  const cert: SiteCert = {
    subject: "blog.test",
    sans: ["blog.test"],
    issuer: "MixEngine Local CA",
    fingerprint: "abc",
    not_before: 0,
    not_after: 1,
    days_left: 42,
  };
  const status = (partial: Partial<SiteCertStatus>): SiteCertStatus => ({
    domain: "blog.test",
    domains: ["blog.test"],
    disk: { state: "present", cert },
    handshake: { handshake: "presented", cert, trust: { trust: "trusted" } },
    problem: null,
    ...partial,
  });

  it("says nothing for a row that was not checked", () => {
    expect(servedCell(undefined)).toEqual({ tone: "neutral", word: "unchecked", because: null });
  });

  it("a trusted certificate with no problem is served", () => {
    expect(servedCell(status({}))).toEqual({ tone: "success", word: "served", because: null });
  });

  it("names the problem the daemon found, with the front end's reason", () => {
    const cell = servedCell(
      status({ handshake: { handshake: "not_served", because: "connection refused" }, problem: "not_served" }),
    );
    expect(cell).toEqual({ tone: "danger", word: "not_served", because: "connection refused" });
  });

  it("carries a rejected chain's reason", () => {
    const cell = servedCell(
      status({
        handshake: { handshake: "presented", cert, trust: { trust: "rejected", because: "unknown issuer" } },
        problem: "not_trusted",
      }),
    );
    expect(cell).toEqual({ tone: "danger", word: "not_trusted", because: "unknown issuer" });
  });

  it("a different certificate served is a problem even when the handshake is trusted", () => {
    const cell = servedCell(status({ problem: "served_certificate_differs" }));
    expect(cell).toEqual({ tone: "danger", word: "served_certificate_differs", because: null });
  });

  it("an expiring certificate is a warning, not a failure", () => {
    expect(servedCell(status({ problem: "expiring" })).tone).toBe("warning");
  });

  it("a handshake that was not asked is unchecked", () => {
    expect(servedCell(status({ handshake: { handshake: "not_asked" } })).word).toBe("unchecked");
  });
});

describe("servedByDomain", () => {
  it("keys each site by its primary domain", () => {
    const site = {
      domain: "blog.test",
      domains: ["blog.test"],
      disk: { state: "absent" },
      handshake: { handshake: "not_asked" },
      problem: "no_certificate",
    } as const satisfies SiteCertStatus;
    expect(servedByDomain({ sites: [site] })).toEqual({ "blog.test": site });
  });
});
