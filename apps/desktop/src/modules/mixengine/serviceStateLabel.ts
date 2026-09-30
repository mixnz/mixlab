import type { ServiceState, StoppedBy } from "@mixengine/api";

/**
 * The translation key for a `ServiceState`, or `null` if there is none.
 *
 * **One table for every screen.** The Dashboard and Extensions both draw service state, and both
 * used to print the wire string directly — so `running` showed up as is, in English, in an app that
 * had translated everything else. Two places translating on their own would become two names for
 * one state.
 *
 * **`null` rather than a guessed key.** `ServiceState` is a closed enum — its doc says plainly "a
 * state machine with room for one more state is one nobody can reason about" — but what is read
 * here is a string a daemon sends, and the daemon may be newer than the running MixLab. An unknown
 * state has to show up exactly as the daemon wrote it, not turn into an empty cell or a
 * translation key that does not exist.
 */
export type ServiceStateKey =
  | `mixengine.serviceState.${ServiceState}`
  | "mixengine.serviceState.resting";

/**
 * `stoppedBy` is what separates **"resting"** from "stopped" — T167g, ADR 0041. A service MixEngine
 * stopped itself for being idle (`stopped` + `daemon`) will be started again on the next request,
 * so it is neither broken nor something the user turned off: drawing it as a red `Stopped` lies
 * about what the user will see. An older daemon does not send `stopped_by`, and then everything
 * shows just as before.
 */
export function serviceStateKey(
  state: string | null | undefined,
  stoppedBy?: StoppedBy | null,
): ServiceStateKey | null {
  if (state === "stopped" && stoppedBy === "daemon") return "mixengine.serviceState.resting";
  return state != null && isServiceState(state) ? `mixengine.serviceState.${state}` : null;
}

/**
 * The explanation that goes with the state label, for the tooltip, or `null` when the label says
 * enough on its own.
 *
 * The label sits in a pill and in narrow cells — the Dashboard table, the tray — so it has to be
 * short; "will start again by itself" is something the user needs to know but not to read at every
 * glance.
 */
export function serviceStateHint(
  state: string | null | undefined,
  stoppedBy?: StoppedBy | null,
): "mixengine.serviceState.restingHint" | null {
  return state === "stopped" && stoppedBy === "daemon" ? "mixengine.serviceState.restingHint" : null;
}

/**
 * The three tones a state is drawn in, or `null` when that state is unknown.
 *
 * Three, not seven: the colour here answers "is it serving", not "which state is it in" — the text
 * already says the state. `degraded` is `busy`, not `bad`: it still answers, just not healthily,
 * and red is for things that do not answer.
 *
 * Kept in one file with `serviceStateKey` because the two functions must cover exactly one set of
 * names; the test checks that.
 */
export type ServiceTone = "ok" | "bad" | "busy" | "resting";

/** `resting` is the fourth tone, grey: not serving right now, but nothing is broken either. */
export function serviceStateTone(
  state: string | null | undefined,
  stoppedBy?: StoppedBy | null,
): ServiceTone | null {
  if (state === "stopped" && stoppedBy === "daemon") return "resting";
  return state != null && isServiceState(state) ? TONE[state] : null;
}

const TONE: Record<ServiceState, ServiceTone> = {
  running: "ok",
  stopped: "bad",
  failed: "bad",
  starting: "busy",
  stopping: "busy",
  restarting: "busy",
  degraded: "busy",
};

/**
 * Which mode a row's toggle button is in.
 *
 * Three modes, and they answer **"what does clicking get you"**, not "which state is the service
 * in":
 *
 * - `up` — it is answering, so what is left is to stop it. `degraded` goes here: it runs weakly,
 *   not not at all, and stopping is still the only meaningful action.
 * - `down` — it is not answering, so the action is to start it. An unknown state goes here too:
 *   inviting a start of something already running gets a readable refusal from the daemon, while
 *   inviting nothing turns that row into a dead end.
 * - `moving` — in transition; there is nothing to click.
 *
 * `inFlight` is "an action was just sent and no event has confirmed it yet". At that point `state`
 * still holds the old value, so looking only at `state` would make the button invite clicking the
 * very thing just clicked.
 */
export type ToggleMode = "up" | "down" | "moving";

export function toggleMode(state: string | null | undefined, inFlight: boolean): ToggleMode {
  if (inFlight) return "moving";
  if (state === "starting" || state === "stopping" || state === "restarting") return "moving";
  return state === "running" || state === "degraded" ? "up" : "down";
}

/** Narrows a wire string to the enum — the only place that knows those seven names, with no
 *  `as`. */
function isServiceState(value: string): value is ServiceState {
  return (KNOWN as ReadonlySet<string>).has(value);
}

const KNOWN: ReadonlySet<ServiceState> = new Set<ServiceState>([
  "stopped",
  "starting",
  "running",
  "degraded",
  "stopping",
  "restarting",
  "failed",
]);
