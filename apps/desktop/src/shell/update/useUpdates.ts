import { useCallback, useEffect, useState } from "react";
import { useWindowFocused } from "../../core/windowFocus";
import * as api from "./api";
import { panelView, retryStep, updateView, type Panel, type Step, type View } from "./view";

/** 30 seconds after the window is drawn, then every 24 hours — spec D6. */
const FIRST_CHECK_MS = 30_000;
const EVERY_MS = 24 * 60 * 60 * 1000;
/** How often the installer's result is looked for while it is open — spec D5. */
const DISK_POLL_MS = 3_000;

/**
 * Runs `read` at once, then every `ms`; returns the stop. Polling starts when the window gets its
 * focus back, which is the moment a person returns from the installer: the first read waiting a
 * whole interval left the pane saying the installer was open for seconds after it had finished.
 */
export function pollNowAndEvery(read: () => void, ms: number): () => void {
  read();
  const timer = window.setInterval(read, ms);
  return () => window.clearInterval(timer);
}

/** The code a rejected command carries, when it carries one. */
function codeOf(error: unknown): string | undefined {
  return typeof error === "object" && error !== null ? (error as { code?: unknown }).code?.toString() : undefined;
}

export interface Updates {
  status: api.UpdateStatus | null;
  view: View;
  /** What the corner panel draws (T188 D1). */
  panel: Panel;
  progress: api.Progress | null;
  handedOver: api.HandedOver | null;
  /** How many services an install restarts; null with no MixEngine running, or before it is known. */
  restarts: number | null;
  checking: boolean;
  /** "Later": hides the panel until the next window start. */
  later: boolean;
  /** A rejected command, as the backend sent it; the pane translates it. */
  error: unknown;
  /** A download or an install that failed, for the panel; null when the last one did not. */
  failure: unknown;
  /** Why *Check now* could not read the feed. */
  checkFailure: string | null;
  checkNow: () => Promise<void>;
  setAutomatic: (on: boolean) => Promise<void>;
  /** Download and prove the offered release. Resolves to whether it finished. */
  download: () => Promise<boolean>;
  /** Stop a running download; what arrived is kept for the next one. */
  cancelDownload: () => Promise<void>;
  /** Windows: install and relaunch. macOS and Linux: open the installer. Resolves to whether it did. */
  install: () => Promise<boolean>;
  /** The step that failed, again. */
  retry: () => Promise<void>;
  skip: () => Promise<void>;
  remindLater: () => void;
  /** Close the panel's failure: the offer stays away until the next start, as *Later* does. */
  closeFailure: () => void;
  finish: () => Promise<void>;
  backFromHandover: () => void;
  dismissError: () => void;
}

/**
 * MixLab's updater, for the shell: mounted once in `Workspace`, and handed to the Updates pane, the
 * corner panel and the Settings button, which all read this one state. Checks on its own only while
 * the automatic switch is on; never downloads or installs without a click (T187 D6, T188 D3).
 *
 * `watching` is whether somebody can see the result of an installer: the poll for the version on
 * disk runs only then, so a person who walked away leaves no timer behind.
 */
export function useUpdates(watching: boolean): Updates {
  const [status, setStatus] = useState<api.UpdateStatus | null>(null);
  const [progress, setProgress] = useState<api.Progress | null>(null);
  const [handedOver, setHandedOver] = useState<api.HandedOver | null>(null);
  const [onDisk, setOnDisk] = useState<string | null>(null);
  const [later, setLater] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [failure, setFailure] = useState<unknown>(null);
  const [lastStep, setLastStep] = useState<Step | null>(null);
  const [restarts, setRestarts] = useState<number | null>(null);
  const [checkFailure, setCheckFailure] = useState<string | null>(null);
  const focused = useWindowFocused();

  useEffect(() => {
    const quietly = () => void api.updateCheck(false).then(setStatus).catch(() => undefined);
    void api
      .updateStatus()
      .then(setStatus)
      .catch(() => undefined);
    const first = window.setTimeout(quietly, FIRST_CHECK_MS);
    const every = window.setInterval(quietly, EVERY_MS);
    return () => {
      window.clearTimeout(first);
      window.clearInterval(every);
    };
  }, []);

  const polling = handedOver !== null && watching && focused;
  useEffect(() => {
    if (!polling) return;
    return pollNowAndEvery(
      () =>
        void api
          .updateVersionOnDisk()
          .then(setOnDisk)
          .catch(() => undefined),
      DISK_POLL_MS,
    );
  }, [polling]);

  const run = useCallback(async (work: () => Promise<void>) => {
    setError(null);
    try {
      await work();
    } catch (e) {
      setError(e);
    }
  }, []);

  /** A download or an install: its failure is the panel's as well as the pane's. */
  const step = useCallback(async (name: Step, work: () => Promise<void>): Promise<boolean> => {
    setError(null);
    setFailure(null);
    setLastStep(name);
    try {
      await work();
      return true;
    } catch (e) {
      const code = codeOf(e);
      // A cancel is a return to the offer, not a failure.
      if (code === "error.updateCancelled") return false;
      // The staged files went away: read the status again, so the view drops back to the offer.
      if (code === "error.updateNotDownloaded") {
        void api
          .updateStatus()
          .then(setStatus)
          .catch(() => undefined);
      }
      setError(e);
      setFailure(e);
      return false;
    }
  }, []);

  const view: View =
    status === null
      ? "upToDate"
      : updateView({
          current: status.current,
          placement: status.placement,
          offered: status.feed && { version: status.feed.version, hasBuild: status.feed.hasBuild },
          skipped: status.skipped,
          installing: installing || status.installing,
          handedOver: handedOver !== null,
          onDisk,
          downloading,
          downloaded: status.downloaded,
        });
  const panel = panelView({ view, later, failed: failure !== null });

  // The sentence under *Install and restart* on Windows: asked when the offer becomes ready, since
  // it is a call to a daemon that may not be running (T188 D1).
  const swapReady = view === "ready" && status?.placement.kind === "swap";
  useEffect(() => {
    if (!swapReady) return;
    void api
      .updateRestarts()
      .then(setRestarts)
      .catch(() => setRestarts(null));
  }, [swapReady]);

  const download = () =>
    step("download", async () => {
      setDownloading(true);
      try {
        await api.updateDownload(setProgress);
      } finally {
        setDownloading(false);
        setProgress(null);
      }
      // What the backend proved is the feed it holds; the status says which version that is. A
      // status that cannot be read now does not undo a download that finished: the offered
      // version is the one it proved.
      const next = await api.updateStatus().catch(() => null);
      setStatus((prev) => next ?? (prev?.feed ? { ...prev, downloaded: prev.feed.version } : prev));
    });

  const install = () =>
    step("install", async () => {
      if (status?.placement.kind === "installer") {
        setHandedOver(await api.updateOpenInstaller());
        return;
      }
      setInstalling(true);
      try {
        // Success ends in a relaunch; this only comes back on failure.
        await api.updateInstall();
      } finally {
        setInstalling(false);
      }
    });

  return {
    status,
    view,
    panel,
    progress,
    handedOver,
    restarts,
    checking,
    later,
    error,
    failure,
    checkFailure,
    checkNow: () =>
      run(async () => {
        setChecking(true);
        setCheckFailure(null);
        try {
          const next = await api.updateCheck(true);
          setStatus(next);
          setCheckFailure(next.failure);
        } finally {
          setChecking(false);
        }
      }),
    setAutomatic: (on) =>
      run(async () => {
        await api.updateSetAutomatic(on);
        setStatus(await api.updateStatus());
      }),
    download,
    cancelDownload: () => api.updateCancelDownload().catch(() => undefined),
    install,
    retry: async () => {
      if (lastStep === null) return;
      await (retryStep(lastStep, failure) === "install" ? install() : download());
    },
    skip: () =>
      run(async () => {
        if (!status?.feed) return;
        await api.updateSkip(status.feed.version);
        setStatus(await api.updateStatus());
      }),
    remindLater: () => setLater(true),
    closeFailure: () => {
      setFailure(null);
      setError(null);
      setLater(true);
    },
    finish: () => run(api.updateFinish),
    backFromHandover: () => {
      setHandedOver(null);
      setOnDisk(null);
    },
    dismissError: () => {
      setError(null);
      setFailure(null);
    },
  };
}
