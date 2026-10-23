import type { BlueprintApplied, BlueprintPlan, MismatchAnswer, PackageRelease, Requirement } from "@mixengine/api";
import { dotenvKeys } from "../../blueprintPlan";

/** What the person has said about a plan. Only meaningful while that plan is on screen, so it
 *  lives in the `plan` phase and goes with it. */
export interface PlanAnswers {
  choices: Record<number, MismatchAnswer>;
  /** The scaffold command, or the archive, may run. */
  scaffold: boolean;
  /** One box per `.env` key the plan offers, keyed by the key. */
  dotenv: Record<string, boolean>;
  /** Install what the plan's releases lack on this machine. */
  prerequisites: boolean;
  /* A Ruby this blueprint pins that cannot build gems with C extensions, and the devkit to install
     with the apply, asked where the decision is made. `null` until the listing answers, and when
     nothing is offered. */
  devkit: { offer: PackageRelease | null } | null;
  /** Ticked by default: the steps after the apply need it, and a project made without it is half
   *  built. */
  devkitAgreed: boolean;
}

export type Phase =
  | { kind: "form" }
  | { kind: "plan"; plan: BlueprintPlan; needs: Requirement[]; answers: PlanAnswers }
  /** `cancelAsked`: Cancel was pressed on the running apply. */
  | { kind: "running"; jobId: number; cancelAsked: boolean }
  /** `cancelled`: carried from `cancelAsked`. An apply cancelled between two steps stops there and
   *  still **succeeds** with the steps it ran (the daemon's apply loop), so the done view is the only
   *  place left to say the list is short because somebody asked. */
  | { kind: "done"; applied: BlueprintApplied; cancelled: boolean }
  | { kind: "failed"; message: string };

/** A dry run's plan, with its answers as they start. */
export function planned(plan: BlueprintPlan, needs: Requirement[]): Phase {
  return {
    kind: "plan",
    plan,
    needs,
    answers: {
      choices: {},
      // Ticked for a blueprint someone vouches for, which is what the person picked it for; left
      // for them to tick on one nobody vouches for.
      scaffold: plan.trusted,
      // Ticked for a signed blueprint, as the scaffold's box is; empty for one nobody vouches for,
      // which could name a key the project reads for something else.
      dotenv: Object.fromEntries(dotenvKeys(plan.steps).map((key) => [key, plan.trusted])),
      prerequisites: false,
      devkit: null,
      devkitAgreed: true,
    },
  };
}

/** The plan's answers with `patch` applied. Outside a plan there is nothing to answer. */
export function answered(phase: Phase, patch: Partial<PlanAnswers>): Phase {
  return phase.kind === "plan" ? { ...phase, answers: { ...phase.answers, ...patch } } : phase;
}

/** The devkit listing's answer, which arrives after the plan is drawn. It belongs to the plan it was
 *  asked for, and to nothing once the apply has started: `apply` reads the offer when pressed. */
export function devkitFound(phase: Phase, plan: BlueprintPlan, devkit: PlanAnswers["devkit"]): Phase {
  return phase.kind === "plan" && phase.plan === plan ? answered(phase, { devkit }) : phase;
}

export function started(jobId: number): Phase {
  return { kind: "running", jobId, cancelAsked: false };
}

/** Cancel pressed, or taken back when the request to cancel failed. */
export function cancelAsked(phase: Phase, asked: boolean): Phase {
  return phase.kind === "running" ? { ...phase, cancelAsked: asked } : phase;
}

/** How the running apply ended. */
export function ended(
  phase: Phase,
  end: { kind: "done"; applied: BlueprintApplied } | { kind: "failed"; message: string },
): Phase {
  if (end.kind === "failed") return end;
  return { ...end, cancelled: phase.kind === "running" && phase.cancelAsked };
}

/** The job a running apply follows, or `null`. What the effects that follow it depend on, so a new
 *  `phase` object for the same job (Cancel pressed) does not start them over. */
export function runningJob(phase: Phase): number | null {
  return phase.kind === "running" ? phase.jobId : null;
}
