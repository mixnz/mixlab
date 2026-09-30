import type { AutostartReport } from "@mixengine/api";
import type { CredentialStore, CredentialsStatus, DoctorReport } from "@mixengine/api";

/**
 * Four states an autostart switch can be in, not two — T85b.
 *
 * **`enabledOtherHome` is the state both the roadmap and `client-surface.md` name explicitly**: a
 * registered entry belonging to another home still makes `enabled: true`, and drawing it as
 * `enabledThisHome` says "enabled" for a switch that has never touched the home that is open.
 */
export type AutostartPresentation = "unsupported" | "enabledOtherHome" | "enabledThisHome" | "disabled";

export function autostartPresentation(
  report: Pick<AutostartReport, "mechanism" | "enabled" | "for_this_home">,
): AutostartPresentation {
  if (report.mechanism === "none") return "unsupported";
  if (!report.enabled) return "disabled";
  return report.for_this_home ? "enabledThisHome" : "enabledOtherHome";
}

/**
 * Keeps the order and length of `DoctorReport.checks` — no check is filtered out, including every
 * `outcome: "ok"`. This function exists only so there is one place for a test to assert that,
 * because filtering is the easiest mistake to make when someone "tidies" the list before drawing
 * it (per the `DoctorReport` doc comment: a shorter list reads as a clean answer rather than a
 * question that was never asked).
 */
export function doctorChecksInOrder(report: DoctorReport): DoctorReport["checks"] {
  return report.checks;
}

/**
 * What the Credentials section draws (T194): the store, and the one it may switch to — `null` when
 * the daemon would refuse, or nothing at all from a daemon that predates the member.
 */
export function credentialsPresentation(
  credentials: CredentialsStatus | null | undefined,
): { store: CredentialStore; switchTo: CredentialStore | null } | null {
  if (!credentials) return null;
  const other: CredentialStore = credentials.store === "os" ? "home" : "os";
  return { store: credentials.store, switchTo: credentials.choosable ? other : null };
}
