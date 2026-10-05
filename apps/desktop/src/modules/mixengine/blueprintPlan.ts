import type { AnswerSubject } from "@mixengine/api";
import type { BlueprintApplied } from "@mixengine/api";
import type { BlueprintPlan } from "@mixengine/api";
import type { JobSummary } from "@mixengine/api";
import type { MismatchAnswer } from "@mixengine/api";
import type { PlanAction } from "@mixengine/api";
import type { PlanStep } from "@mixengine/api";
import type { ScaffoldConsent } from "@mixengine/api";
import type { StepOutcome } from "@mixengine/api";
import type { VersionAnswer } from "@mixengine/api";
import type { TranslationKey } from "../../i18n";

/**
 * Only two action kinds ever produce `disposition: "choice"` in the current bindings
 * (`install_runtime`, `ensure_service`) — the subject is derived from the action itself.
 *
 * **The service `id` is built as `${package}@${instance}` — inferred, not yet checked against a
 * real daemon.** See the Task 12 note (Blueprints plan) and the Self-Review Notes.
 */
export function answerSubjectFor(step: PlanStep): AnswerSubject | null {
  if (step.disposition.disposition !== "choice") return null;
  if (step.action.action === "install_runtime") {
    return { subject: "runtime", kind: step.action.kind };
  }
  if (step.action.action === "ensure_service") {
    return { subject: "service", id: `${step.action.package}@${step.action.instance}` };
  }
  return null;
}

/** `-1` when the plan has no `run_scaffold` step — there is always at most one such step in a
 *  plan. */
export function scaffoldStepIndex(steps: PlanStep[]): number {
  return steps.findIndex((step) => step.action.action === "run_scaffold");
}

/**
 * What state the consent box is in — `none` when the plan has no command to ask about at all.
 *
 * **`declined` is an answer, not a box nobody has touched.** In `mix`, this question is a blocking
 * `[y/N]`: without an answer you cannot go on, and `unasked` prints a whole stderr line saying the
 * command was skipped. On the desktop the checkbox is silent, so an apply that skips the init
 * command looks exactly like one that runs it — the user only finds out on the "Done" screen, lost
 * among ten other lines. This state is what lets the interface say it up front, on the button and
 * in the warning block next to the checkbox.
 */
export function scaffoldConsentState(
  steps: PlanStep[],
  agreed: boolean,
): "none" | "agreed" | "declined" {
  if (scaffoldStepIndex(steps) < 0) return "none";
  return agreed ? "agreed" : "declined";
}

/**
 * The init command that was left out, or `null`.
 *
 * Used to build a block of its own at the top of the "Done" screen instead of a `stepNotRun` line
 * lost in the list — and instead of the daemon's `why`, which ends with a hint to run
 * `mix blueprint apply --run-scaffold`: a command-line flag that means nothing to someone clicking
 * a mouse.
 */
export function scaffoldLeftCommand(applied: BlueprintApplied): string | null {
  for (const outcome of applied.steps) {
    if (outcome.action.action !== "run_scaffold") continue;
    if (outcome.result.result !== "not_run") continue;
    return outcome.action.command;
  }
  return null;
}

/**
 * The steps that ran and failed — empty when the apply went through cleanly.
 *
 * **A successful job does not mean a clean apply.** `api/apply.rs` deliberately returns
 * `StepResult::Failed` for a `[scaffold]` with a non-zero exit instead of raising an error: a
 * broken post-install script leaves a project that is still usable — the site still serves, the
 * database is still there — and tearing that down too would be the more expensive wrong direction.
 * The mistake is a client reading "job done" as "done", then inviting people to click into a site
 * whose directory is half-built.
 *
 * `not_run` is not here: it is an answer (the consent box left empty), not a failure, and
 * [`scaffoldLeftCommand`] already says that separately.
 */
export function failedSteps(applied: BlueprintApplied): StepOutcome[] {
  return applied.steps.filter((outcome) => outcome.result.result === "failed");
}

/**
 * The real apply is only enabled when every `choice` step has an answer and no step is `blocked`/
 * `unsupported`. A `confirm` step (scaffold) blocks nothing — declining it only makes that step
 * `not_run`, it does not stop the whole plan from being sent.
 */
export function canApply(steps: PlanStep[], choices: Record<number, MismatchAnswer>): boolean {
  return steps.every((step, i) => {
    const d = step.disposition.disposition;
    if (d === "blocked" || d === "unsupported") return false;
    if (d === "choice") return choices[i] !== undefined;
    return true;
  });
}

export function buildAnswers(
  steps: PlanStep[],
  choices: Record<number, MismatchAnswer>,
): VersionAnswer[] {
  const answers: VersionAnswer[] = [];
  steps.forEach((step, i) => {
    const subject = answerSubjectFor(step);
    const answer = choices[i];
    if (subject && answer) answers.push({ subject, answer });
  });
  return answers;
}

/** `command` is exactly the string the step showed — not anything the user typed again. */
export function buildScaffoldConsent(plan: BlueprintPlan, step: PlanStep): ScaffoldConsent | null {
  if (step.action.action !== "run_scaffold") return null;
  return { command: step.action.command, untrusted: !plan.trusted };
}

/** One human-readable sentence for each `PlanAction` — ten variants, ten i18n keys. */
export function describePlanAction(
  t: (key: TranslationKey, vars?: Record<string, string | number>) => string,
  action: PlanAction,
): string {
  const base = "mixengine.blueprints.apply.action" as const;
  switch (action.action) {
    case "register_project":
      return t(`${base}.register_project`, { name: action.name, root: action.root });
    case "install_runtime":
      return t(`${base}.install_runtime`, { kind: action.kind, wanted: action.wanted });
    case "install_package":
      return t(`${base}.install_package`, {
        package: action.package,
        wanted: action.wanted ?? "latest",
      });
    case "ensure_service":
      return t(`${base}.ensure_service`, { package: action.package, instance: action.instance });
    case "create_database":
      return t(`${base}.create_database`, { database: action.database, user: action.user });
    case "create_site":
      return t(`${base}.create_site`, { kind: action.kind.kind, docRoot: action.doc_root });
    case "add_domain":
      return t(`${base}.add_domain`, { domain: action.domain });
    case "issue_certificate":
      return t(`${base}.issue_certificate`, { domains: action.domains.join(", ") });
    case "set_php_extension":
      return action.runtime
        ? t(`${base}.set_php_extension`, { name: action.name, runtime: action.runtime })
        : t(`${base}.set_php_extension_pending`, { name: action.name });
    case "run_scaffold":
      return t(`${base}.run_scaffold`, { command: action.command });
  }
}

/** `null` for a job that is unfinished, failed or cancelled — not inferred from `state`; only
 *  `outcome` is read. */
export function blueprintAppliedFrom(job: JobSummary): BlueprintApplied | null {
  if (job.outcome?.ending !== "succeeded") return null;
  return job.outcome.result as BlueprintApplied;
}

/** Whether a job ended because somebody cancelled it: neither applied nor failed, and a phase of
 *  its own to the dialog following it. */
export function jobWasCancelled(job: JobSummary): boolean {
  return job.outcome?.ending === "cancelled";
}

/** `null` when the job did not fail (including still running, including cancelled) — a
 *  cancellation is not an error to show. */
export function jobFailureMessage(job: JobSummary): string | null {
  return job.outcome?.ending === "failed" ? job.outcome.error.message : null;
}
