import type { CertIssueReport } from "@mixengine/api";
import type { IssueOutcome } from "@mixengine/api";

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
