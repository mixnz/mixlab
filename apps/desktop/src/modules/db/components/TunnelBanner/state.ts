import type { AppError } from "../../../../core/errors";
import type { TunnelState } from "../../tunnel";

/**
 * What the banner is saying, if it is saying anything.
 *
 * Kept apart from the component because this is the only place with anything to get wrong: the
 * order events arrive in, and which one is allowed to replace which.
 */
export type BannerState =
  | { kind: "hidden" }
  | { kind: "reconnecting" }
  | { kind: "reconnected" }
  | { kind: "failed"; error: AppError };

export const HIDDEN: BannerState = { kind: "hidden" };

/**
 * Whether the popup may show for this state.
 *
 * `ripe` means disconnected long enough to be worth blocking the screen; `showing` means the popup
 * has been standing there from before.
 *
 * Two different gates, because two disconnections are not alike. Most last only a few hundred
 * milliseconds, heal themselves, and the statement carries on as if nothing happened: blocking
 * there is just a flash of the screen. And "reconnected" only reassures someone who was just
 * blocked — someone who saw nothing happen does not need to be told it has passed.
 */
export function popupShows(state: BannerState, ripe: boolean, showing: boolean): boolean {
  switch (state.kind) {
    case "hidden":
      return false;
    case "reconnecting":
      return ripe;
    case "reconnected":
      return showing;
    case "failed":
      return true;
  }
}

/** The banner's next state. Returns `current` itself when there is nothing new to say. */
export function nextBannerState(current: BannerState, event: TunnelState): BannerState {
  switch (event.state) {
    case "reconnecting":
      return current.kind === "reconnecting" ? current : { kind: "reconnecting" };
    case "reconnected":
      // Nothing has ever shown, so there is nobody to reassure: a tab opened after the tunnel has
      // healed itself has no reason to report "reconnected".
      return current.kind === "hidden" ? current : { kind: "reconnected" };
    case "failed": {
      const error = event.error ?? { code: "error.sshUnavailable" };
      // The same error repeating is the watcher's backoff beat, not news. Keep the same object so
      // nothing on the React side is rebuilt every minute.
      if (current.kind === "failed" && current.error.code === error.code) return current;
      return { kind: "failed", error };
    }
  }
}
