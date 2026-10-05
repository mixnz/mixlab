import type { CertIssueReport } from "@mixengine/api";
import type { CertProblem } from "@mixengine/api";
import type { CertStatusReport } from "@mixengine/api";
import type { IssueOutcome } from "@mixengine/api";
import type { SiteCertStatus } from "@mixengine/api";

import type { StatusTone } from "../../components/StatusPill";

export interface CertRow {
  domain: string;
  outcome: IssueOutcome;
  sans: string[];
  daysLeft: number | null;
}

/**
 * `cert.issue` called without `site` both draws the table and reissues — the way the roadmap chose
 * for T2.7. Only `state.state === "present"` has a `cert` to read `sans`/`days_left` from;
 * `absent`/`unusable` have nothing to read, which is not an error.
 */
export function buildCertRows(report: CertIssueReport): CertRow[] {
  return report.sites.map(({ domain, outcome, state }) => ({
    domain,
    outcome,
    sans: state.state === "present" ? state.cert.sans : [],
    daysLeft: state.state === "present" ? state.cert.days_left : null,
  }));
}

/** `cert.status`'s answer, by the domain each row of the disk table is keyed on. */
export function servedByDomain(report: CertStatusReport): Record<string, SiteCertStatus> {
  return Object.fromEntries(report.sites.map((site) => [site.domain, site]));
}

/** What the Served cell says: not checked, served as it should be, or the one problem to act on. */
export type ServedWord = "unchecked" | "served" | CertProblem;

export interface ServedCell {
  tone: StatusTone;
  word: ServedWord;
  /** The handshake's own words, when it has any — the pill's tooltip. */
  because: string | null;
}

/**
 * The Served cell for one row — roadmap task **T199**, the design's D3.
 *
 * **The problem decides, not the handshake.** `cert.status` names the one condition worth acting
 * on, and a cell that re-derived it from the handshake would be a second opinion that can disagree
 * with the daemon's. The handshake only supplies the reason in words.
 */
export function servedCell(status: SiteCertStatus | undefined): ServedCell {
  if (status === undefined) return { tone: "neutral", word: "unchecked", because: null };

  const { handshake, problem } = status;
  const because =
    handshake.handshake === "not_served" || handshake.handshake === "failed"
      ? handshake.because
      : handshake.handshake === "presented" && handshake.trust.trust === "rejected"
        ? handshake.trust.because
        : null;

  if (problem !== null) {
    return { tone: problem === "expiring" ? "warning" : "danger", word: problem, because };
  }
  if (handshake.handshake === "presented" && handshake.trust.trust === "trusted") {
    return { tone: "success", word: "served", because: null };
  }
  return { tone: "neutral", word: "unchecked", because };
}
