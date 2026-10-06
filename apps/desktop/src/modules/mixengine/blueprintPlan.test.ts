import { describe, expect, it } from "vitest";

import {
  answerSubjectFor,
  blueprintAppliedFrom,
  buildAnswers,
  buildScaffoldConsent,
  canApply,
  describePlanAction,
  failedSteps,
  jobFailureMessage,
  jobWasCancelled,
  scaffoldConsentState,
  scaffoldLeftCommand,
  scaffoldStepIndex,
} from "./blueprintPlan";
import type { BlueprintApplied } from "@mixengine/api";
import type { PlanStep } from "@mixengine/api";
import type { BlueprintPlan } from "@mixengine/api";
import type { JobSummary } from "@mixengine/api";
import type { StepOutcome } from "@mixengine/api";

function step(partial: Partial<PlanStep> & Pick<PlanStep, "action" | "disposition">): PlanStep {
  return { elevates: false, ...partial };
}

describe("answerSubjectFor", () => {
  it("names the runtime for an install_runtime choice", () => {
    const s = step({
      action: { action: "install_runtime", kind: "php", wanted: "8.3" },
      disposition: { disposition: "choice", installed: "8.2.1", wanted: "8.3" },
    });
    expect(answerSubjectFor(s)).toEqual({ subject: "runtime", kind: "php" });
  });

  it("names the service instance for an ensure_service choice", () => {
    const s = step({
      action: {
        action: "ensure_service",
        package: "mariadb",
        instance: "main",
        version: "^11",
        dedicated: false,
      },
      disposition: { disposition: "choice", installed: "10.6.0", wanted: "^11" },
    });
    expect(answerSubjectFor(s)).toEqual({ subject: "service", id: "mariadb@main" });
  });

  it("is null for a step that is not a choice", () => {
    const s = step({
      action: { action: "add_domain", domain: "blog.test", primary: true },
      disposition: { disposition: "satisfied" },
    });
    expect(answerSubjectFor(s)).toBeNull();
  });
});

describe("scaffoldStepIndex", () => {
  it("finds the run_scaffold step's index, or -1 when there is none", () => {
    const steps: PlanStep[] = [
      step({
        action: { action: "add_domain", domain: "blog.test", primary: true },
        disposition: { disposition: "satisfied" },
      }),
      step({
        action: { action: "run_scaffold", command: "composer install" },
        disposition: { disposition: "confirm", what: "composer install" },
      }),
    ];
    expect(scaffoldStepIndex(steps)).toBe(1);
    expect(scaffoldStepIndex(steps.slice(0, 1))).toBe(-1);
  });
});

describe("scaffoldConsentState", () => {
  const scaffold = step({
    action: { action: "run_scaffold", command: "composer create-project laravel/laravel ." },
    disposition: { disposition: "confirm", what: "composer create-project laravel/laravel ." },
  });
  const domain = step({
    action: { action: "add_domain", domain: "blog.test", primary: true },
    disposition: { disposition: "satisfied" },
  });

  it("is none when the plan asks for no command at all", () => {
    expect(scaffoldConsentState([domain], false)).toBe("none");
    expect(scaffoldConsentState([domain], true)).toBe("none");
  });

  it("tells an unticked box apart from a plan with nothing to tick", () => {
    expect(scaffoldConsentState([domain, scaffold], false)).toBe("declined");
    expect(scaffoldConsentState([domain, scaffold], true)).toBe("agreed");
  });
});

describe("scaffoldLeftCommand", () => {
  function applied(steps: StepOutcome[]): BlueprintApplied {
    return { blueprint: "laravel-starter", project: "blog", root: "/srv/blog", steps };
  }

  it("names the command a skipped step was carrying", () => {
    const left = applied([
      {
        action: { action: "run_scaffold", command: "composer create-project laravel/laravel ." },
        result: { result: "not_run", why: "nobody agreed to it" },
      },
    ]);
    expect(scaffoldLeftCommand(left)).toBe("composer create-project laravel/laravel .");
  });

  it("is null when the command ran, and when there was none", () => {
    expect(
      scaffoldLeftCommand(
        applied([
          { action: { action: "run_scaffold", command: "composer install" }, result: { result: "done" } },
        ]),
      ),
    ).toBeNull();
    expect(scaffoldLeftCommand(applied([]))).toBeNull();
  });
});

describe("failedSteps", () => {
  function applied(steps: StepOutcome[]): BlueprintApplied {
    return { blueprint: "laravel-starter", project: "blog", root: "/srv/blog", steps };
  }

  it("finds a step that ran and failed inside an otherwise successful apply", () => {
    const outcomes: StepOutcome[] = [
      { action: { action: "add_domain", domain: "blog.test", primary: true }, result: { result: "done" } },
      {
        action: { action: "run_scaffold", command: "composer create-project laravel/laravel ." },
        result: { result: "failed", why: "exit code 1: Could not find package" },
      },
    ];
    expect(failedSteps(applied(outcomes))).toEqual([outcomes[1]]);
  });

  // A declined consent is an answer, not a failure — `scaffoldLeftCommand` is what says that one.
  it("is empty for done, already_true and not_run", () => {
    expect(
      failedSteps(
        applied([
          { action: { action: "run_scaffold", command: "composer install" }, result: { result: "done" } },
          {
            action: { action: "add_domain", domain: "blog.test", primary: true },
            result: { result: "already_true" },
          },
          {
            action: { action: "run_scaffold", command: "npm install" },
            result: { result: "not_run", why: "nobody agreed to it" },
          },
        ]),
      ),
    ).toEqual([]);
    expect(failedSteps(applied([]))).toEqual([]);
  });
});

describe("canApply", () => {
  const choiceStep = step({
    action: { action: "install_runtime", kind: "php", wanted: "8.3" },
    disposition: { disposition: "choice", installed: "8.2.1", wanted: "8.3" },
  });

  it("is false while a choice step has no answer", () => {
    expect(canApply([choiceStep], {})).toBe(false);
  });

  it("is true once every choice step is answered", () => {
    expect(canApply([choiceStep], { 0: "install" })).toBe(true);
  });

  it("is false when any step is blocked or unsupported, answered or not", () => {
    const blocked = step({
      action: { action: "create_site", kind: { kind: "static" }, doc_root: "", https: true },
      disposition: { disposition: "blocked", reason: "no web server" },
    });
    expect(canApply([blocked], {})).toBe(false);
  });

  it("is true when every step is satisfied/create/confirm and nothing needs an answer", () => {
    const scaffold = step({
      action: { action: "run_scaffold", command: "composer install" },
      disposition: { disposition: "confirm", what: "composer install" },
    });
    expect(canApply([scaffold], {})).toBe(true);
  });
});

describe("buildAnswers", () => {
  it("pairs each answered choice step with its subject, in step order", () => {
    const steps: PlanStep[] = [
      step({
        action: { action: "install_runtime", kind: "php", wanted: "8.3" },
        disposition: { disposition: "choice", installed: "8.2.1", wanted: "8.3" },
      }),
      step({
        action: { action: "add_domain", domain: "blog.test", primary: true },
        disposition: { disposition: "satisfied" },
      }),
    ];
    expect(buildAnswers(steps, { 0: "use_installed" })).toEqual([
      { subject: { subject: "runtime", kind: "php" }, answer: "use_installed" },
    ]);
  });

  it("omits a choice step with no recorded answer", () => {
    const steps: PlanStep[] = [
      step({
        action: { action: "install_runtime", kind: "php", wanted: "8.3" },
        disposition: { disposition: "choice", installed: "8.2.1", wanted: "8.3" },
      }),
    ];
    expect(buildAnswers(steps, {})).toEqual([]);
  });
});

describe("buildScaffoldConsent", () => {
  const plan: BlueprintPlan = {
    blueprint: "laravel-starter",
    project: "blog",
    root: "/srv/blog",
    steps: [],
    source: "imported",
    trusted: false,
    signature: "missing",
  };

  it("names the exact command shown, and marks it untrusted when the plan is", () => {
    const s = step({
      action: { action: "run_scaffold", command: "composer install" },
      disposition: { disposition: "confirm", what: "composer install" },
    });
    expect(buildScaffoldConsent(plan, s)).toEqual({ command: "composer install", untrusted: true });
  });

  it("is null for a step that is not run_scaffold", () => {
    const s = step({
      action: { action: "add_domain", domain: "blog.test", primary: true },
      disposition: { disposition: "satisfied" },
    });
    expect(buildScaffoldConsent(plan, s)).toBeNull();
  });
});

function fakeT(key: string, vars?: Record<string, string | number>): string {
  return vars ? `${key}:${JSON.stringify(vars)}` : key;
}

describe("describePlanAction", () => {
  it("interpolates the action's own fields into its key", () => {
    const text = describePlanAction(fakeT, { action: "add_domain", domain: "blog.test", primary: true });
    expect(text).toBe('mixengine.blueprints.apply.action.add_domain:{"domain":"blog.test"}');
  });

  it("falls back to a literal 'latest' when install_package names no version", () => {
    const text = describePlanAction(fakeT, { action: "install_package", package: "redis" });
    expect(text).toContain('"wanted":"latest"');
  });
});

describe("blueprintAppliedFrom / jobFailureMessage", () => {
  function job(outcome: JobSummary["outcome"]): JobSummary {
    return {
      id: 1,
      kind: "blueprint.apply",
      state: "succeeded",
      percent: 100,
      message: "",
      started_at: 0,
      finished_at: 1,
      outcome,
    };
  }

  it("reads the applied result out of a succeeded job", () => {
    const applied = { blueprint: "b", project: "blog", root: "/srv/blog", steps: [] };
    expect(blueprintAppliedFrom(job({ ending: "succeeded", result: applied }))).toEqual(applied);
    expect(jobFailureMessage(job({ ending: "succeeded", result: applied }))).toBeNull();
  });

  it("returns null for a failed or cancelled job, and a message for the failure", () => {
    const failed = job({ ending: "failed", error: { code: "internal", message: "disk full" } });
    expect(blueprintAppliedFrom(failed)).toBeNull();
    expect(jobFailureMessage(failed)).toBe("disk full");

    const cancelled = job({ ending: "cancelled" });
    expect(blueprintAppliedFrom(cancelled)).toBeNull();
    expect(jobFailureMessage(cancelled)).toBeNull();
  });

  it("says a cancelled job was cancelled, and no other ending", () => {
    expect(jobWasCancelled(job({ ending: "cancelled" }))).toBe(true);
    expect(jobWasCancelled(job({ ending: "failed", error: { code: "internal", message: "x" } }))).toBe(false);
    expect(jobWasCancelled(job({ ending: "succeeded", result: null }))).toBe(false);
    expect(jobWasCancelled(job(null))).toBe(false);
  });

  it("returns null for a job with no outcome yet", () => {
    expect(blueprintAppliedFrom(job(null))).toBeNull();
  });
});
