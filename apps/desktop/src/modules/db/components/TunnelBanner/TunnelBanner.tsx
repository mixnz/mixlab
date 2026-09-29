import { useEffect, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import Button from "../../../../components/Button";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import { onTunnelState, tunnelReconnect } from "../../tunnel";
import { HIDDEN, nextBannerState, popupShows, type BannerState } from "./state";
import styles from "./TunnelBanner.module.css";

/** How long "Reconnected" stays before disappearing by itself. */
const REASSURED_MS = 3000;

/**
 * How long a disconnection lasts before it is worth blocking the screen.
 *
 * Below this threshold nobody knows anything happened: the tunnel reopens, the statement is run
 * again, the data arrives — and a popup flashing up and away is just a needless start.
 */
const BLOCK_AFTER_MS = 800;

interface Props {
  connectionId: string;
  /** Disconnects this connection for good — one of the two ways out when the tunnel cannot be
   *  reopened. */
  onDisconnect: () => void;
}

/**
 * Blocks this tab's workspace while its SSH tunnel is broken, until it can be reopened or the user
 * decides to do something else.
 *
 * Blocking rather than notifying, because at that point nothing can really be done: every command
 * returns "connection lost", and that is also the sentence ErrorBanner is swallowing to make room
 * for this — see `useWorkspaceError`.
 *
 * Covers exactly the workspace rather than portalling out to `document.body`: background tabs in
 * MixDB stay mounted and are only `display: none`, so a popup fixed to the viewport would cover the
 * tab being viewed because another tab's tunnel dropped.
 *
 * Returns `null` when there is nothing to say — including for connections not going through a
 * tunnel, since no event ever arrives for them.
 */
function TunnelBanner({ connectionId, onDisconnect }: Props) {
  const { t } = useTranslation();
  const [state, setState] = useState<BannerState>(HIDDEN);
  const [retrying, setRetrying] = useState(false);
  /**
   * The state for which the user pressed "later", compared by identity rather than by value.
   *
   * `nextBannerState` returns the very same object when the new news says nothing different, so the
   * watcher's backoff beat — the same error, once a minute — does not bring back the popup that was
   * just dismissed, while something genuinely different does.
   */
  const [dismissed, setDismissed] = useState<BannerState | null>(null);
  /** Disconnected for longer than {@link BLOCK_AFTER_MS}. */
  const [ripe, setRipe] = useState(false);
  /** The popup has been standing there since the previous render — see {@link popupShows}. */
  const [showing, setShowing] = useState(false);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let stopped = false;
    void onTunnelState(connectionId, (event) =>
      setState((current) => nextBannerState(current, event))
    ).then((fn) => {
      // The tab may have closed before `listen` managed to return: unsubscribe right away rather
      // than leaving a listener nobody removes.
      if (stopped) fn();
      else unlisten = fn;
    });
    return () => {
      stopped = true;
      unlisten?.();
      setState(HIDDEN);
      setDismissed(null);
    };
  }, [connectionId]);

  useEffect(() => {
    if (state.kind !== "reconnecting") {
      setRipe(true);
      return;
    }
    setRipe(false);
    const timer = setTimeout(() => setRipe(true), BLOCK_AFTER_MS);
    return () => clearTimeout(timer);
  }, [state]);

  useEffect(() => {
    if (state.kind !== "reconnected") return;
    const timer = setTimeout(() => setState(HIDDEN), REASSURED_MS);
    return () => clearTimeout(timer);
  }, [state]);

  const visible = state !== dismissed && popupShows(state, ripe, showing);

  useEffect(() => setShowing(visible), [visible]);

  useEffect(() => {
    if (!visible) return;
    function onKeyDown(e: KeyboardEvent) {
      // Esc does exactly what the "later" button does, no more: no disconnecting, no retrying.
      if (e.key === "Escape") setDismissed(state);
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [visible, state]);

  if (!visible || state.kind === "hidden") return null;

  const retry = async () => {
    setRetrying(true);
    try {
      await tunnelReconnect(connectionId);
    } catch {
      // Nothing to catch: whether it reopens or not, the tunnel reports by itself and the popup
      // changes with that news.
    } finally {
      setRetrying(false);
    }
  };

  const message =
    state.kind === "reconnecting"
      ? t("tunnel.reconnecting")
      : state.kind === "reconnected"
        ? t("tunnel.reconnected")
        : t("tunnel.failed", { message: errorMessage(t, state.error) });

  return (
    <div className={styles.overlay}>
      <div
        className={`${styles.dialog} ${styles[state.kind]}`}
        role="alertdialog"
        aria-modal="true"
        aria-label={message}
      >
        <p className={styles.text}>
          {state.kind === "reconnecting" && <span className={styles.spinner} aria-hidden="true" />}
          {message}
        </p>
        {/* "Reconnected" asks nothing: it disappears by itself and everything is usable again. */}
        {state.kind !== "reconnected" && (
          <div className={styles.actions}>
            <Button size="large" onClick={onDisconnect}>
              {t("common.disconnect")}
            </Button>
            <Button size="large" onClick={() => setDismissed(state)}>
              {t("tunnel.later")}
            </Button>
            {state.kind === "failed" && (
              <Button size="large" variant="primary" onClick={retry} disabled={retrying} autoFocus>
                {t("tunnel.retry")}
              </Button>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

export default TunnelBanner;
