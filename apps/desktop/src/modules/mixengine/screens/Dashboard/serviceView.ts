import type { StoppedBy } from "@mixengine/api";
import type { StatusTone } from "../../../../components/StatusPill";
import type { useTranslation } from "../../../../i18n";
import { serviceStateHint, serviceStateKey, serviceStateTone } from "../../serviceStateLabel";

type Translate = ReturnType<typeof useTranslation>["t"];

/* An explicit lookup table rather than building `${action}ing`: "stop" + "ing" gives "stoping", and
   a translation key built by string concatenation is a key nobody can grep for. */
export const PENDING_LABEL = {
  start: "mixengine.dashboard.starting",
  stop: "mixengine.dashboard.stopping",
  restart: "mixengine.dashboard.restarting",
} as const;

export type ServiceFilter = "all" | "running" | "stopped";

/** The state as a pill tone: whether it is serving, not which of the seven states it is in. */
export function pillTone(state: string | null | undefined, stoppedBy?: StoppedBy | null): StatusTone {
  const tone = serviceStateTone(state, stoppedBy);
  if (tone === "ok") return "success";
  if (tone === "bad") return "danger";
  if (tone === "busy") return "warning";
  return "neutral";
}

/** The translated state; a state from a daemon newer than this build is shown verbatim. */
export function stateLabel(t: Translate, state: string | null | undefined, stoppedBy?: StoppedBy | null): string {
  const key = serviceStateKey(state, stoppedBy);
  return key === null ? (state ?? "—") : t(key);
}

/** The sentence the short label leaves out, for the pill's tooltip — and for a failed service,
 *  why it failed (T200b, D6). */
export function stateHint(
  t: Translate,
  state: string | null | undefined,
  stoppedBy?: StoppedBy | null,
  lastFailure?: string | null,
): string | undefined {
  if (state === "failed" && lastFailure) return lastFailure;
  const key = serviceStateHint(state, stoppedBy);
  return key === null ? undefined : t(key);
}

/** Serving, for the filter and the summary: `degraded` still answers. */
export function isServing(state: string | null | undefined): boolean {
  return state === "running" || state === "degraded";
}
