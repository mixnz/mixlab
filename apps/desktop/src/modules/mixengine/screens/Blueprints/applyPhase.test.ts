import { describe, expect, it } from "vitest";
import type { BlueprintApplied, BlueprintPlan, PackageRelease, PlanStep } from "@mixengine/api";
import { answered, cancelAsked, devkitFound, ended, planned, runningJob, started, type Phase } from "./applyPhase";

const DOTENV = {
  action: { action: "write_dotenv", key: "DATABASE_URL", path: ".env" },
  disposition: { disposition: "confirm" },
} as unknown as PlanStep;

function plan(trusted: boolean): BlueprintPlan {
  return { blueprint: "laravel", project: "blog", root: "/r", steps: [DOTENV], source: { source: "gallery" }, trusted } as unknown as BlueprintPlan;
}

const OFFER = { version: "3.3" } as unknown as PackageRelease;
const APPLIED = { root: "/r", steps: [] } as unknown as BlueprintApplied;

describe("planned", () => {
  it("ticks the command and the .env keys for a blueprint someone vouches for", () => {
    const phase = planned(plan(true), []);
    expect(phase.kind === "plan" && phase.answers).toEqual({
      choices: {},
      scaffold: true,
      dotenv: { DATABASE_URL: true },
      prerequisites: false,
      devkit: null,
      devkitAgreed: true,
    });
  });

  it("leaves them for the person to tick on one nobody vouches for", () => {
    const phase = planned(plan(false), []);
    expect(phase.kind === "plan" && [phase.answers.scaffold, phase.answers.dotenv]).toEqual([false, { DATABASE_URL: false }]);
  });
});

describe("answered", () => {
  it("changes only the answers it is given", () => {
    const phase = answered(planned(plan(true), []), { scaffold: false });
    expect(phase.kind === "plan" && [phase.answers.scaffold, phase.answers.devkitAgreed]).toEqual([false, true]);
  });

  it("does nothing outside a plan", () => {
    const running = started(1);
    expect(answered(running, { scaffold: false })).toBe(running);
  });
});

describe("devkitFound", () => {
  it("lands on the plan it was asked for", () => {
    const shown = plan(true);
    const phase = devkitFound(planned(shown, []), shown, { offer: OFFER });
    expect(phase.kind === "plan" && phase.answers.devkit).toEqual({ offer: OFFER });
  });

  it("is dropped once the apply has started, or on another plan", () => {
    const shown = plan(true);
    const running = started(7);
    expect(devkitFound(running, shown, { offer: OFFER })).toBe(running);
    const other = planned(plan(true), []);
    expect(devkitFound(other, shown, { offer: OFFER })).toBe(other);
  });
});

describe("cancel", () => {
  it("is asked of a running apply, and taken back when the request fails", () => {
    const asked = cancelAsked(started(7), true);
    expect(asked).toEqual({ kind: "running", jobId: 7, cancelAsked: true });
    expect(cancelAsked(asked, false)).toEqual({ kind: "running", jobId: 7, cancelAsked: false });
  });

  it("is carried into the done view, where the short list is explained", () => {
    expect(ended(cancelAsked(started(7), true), { kind: "done", applied: APPLIED })).toEqual({
      kind: "done",
      applied: APPLIED,
      cancelled: true,
    });
    expect(ended(started(7), { kind: "done", applied: APPLIED })).toEqual({ kind: "done", applied: APPLIED, cancelled: false });
  });

  it("ends a failed apply without it", () => {
    expect(ended(cancelAsked(started(7), true), { kind: "failed", message: "no" })).toEqual({ kind: "failed", message: "no" });
  });
});

describe("runningJob", () => {
  it("is the job of a running apply, the same whatever was asked of it", () => {
    const phases: Phase[] = [started(7), cancelAsked(started(7), true)];
    expect(phases.map(runningJob)).toEqual([7, 7]);
    expect(runningJob({ kind: "form" })).toBeNull();
  });
});
