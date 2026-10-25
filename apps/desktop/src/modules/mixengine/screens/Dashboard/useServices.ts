import { useCallback, useEffect, useRef, useState } from "react";
import type { DaemonStatus } from "@mixengine/api";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import {
  applyEvent,
  applyJob,
  forgetFinished,
  isJobFinished,
  movesARow,
  needsResync,
  rowsFrom,
  type JobRow,
  type ServiceRow,
} from "../../daemonState";
import { subscribeDaemonWatch } from "../../daemonWatch";
import { failureMayHaveChanged } from "../../failureState";
import { hasNewOps, pendingFrom, pendingIds } from "../../pendingOps";
import { eventArrived, noReadsYet, readBegan, readLanded } from "../../readOrder";

interface Options {
  active: boolean;
  /** The disk panel's read, started beside every reread and never awaited — see `useDiskUsage`. */
  readDisk: () => Promise<void>;
  setError: (message: string) => void;
}

/** The services table and the stream that keeps it current: the reads, the actions on a row, the
 *  jobs in progress and the operations waiting for administrator rights. */
export function useServices({ active, readDisk, setError }: Options) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<DaemonStatus | null>(null);
  const [rows, setRows] = useState<ServiceRow[]>([]);
  /** False until the first read has been drawn or has failed — until then an empty `rows` means
   *  "not known yet", not "no services". */
  const [loaded, setLoaded] = useState(false);
  const [pending, setPending] = useState<unknown[] | null>(null);
  /** `ElevationStatus.can_prompt`/`reason` — "is there still a helper to raise the prompt, and why
   *  not when there is not". Defaults to `true` because most machines can raise the prompt; only
   *  changes when `elevation.status` says otherwise. */
  const [canPrompt, setCanPrompt] = useState(true);
  const [reason, setReason] = useState<string | null | undefined>(null);
  /** How many operations are waiting for rights, per `daemon.status`. Just a number; the list is in
   *  `elevation.status`. */
  const [waiting, setWaiting] = useState(0);
  /** The ids the last `elevation_required` carried, so a queue that shrank is told from one that
   *  grew — the event is published both ways. A ledger, not something drawn, hence a ref. */
  const announced = useRef(new Set<number>());
  const [jobs, setJobs] = useState<JobRow[]>([]);
  /** The jobs Cancel was pressed for, until their `job_finished` — see `forgetFinished`. */
  const [cancelling, setCancelling] = useState<ReadonlySet<number>>(() => new Set());
  /** Which services have an action in flight, and which action. Keyed by id. */
  const [busy, setBusy] = useState<Record<string, api.ServiceAction>>({});

  /**
   * The order between reads and events — see `readOrder.ts`.
   *
   * **`rows` has two writers and neither knows about the other.** A `service.list` snapshot and a
   * `service_state_changed` both call `setRows`, and React writes in order of **arrival**, not in
   * the *right* order. Pressing Stop all is when that shows: many operations finish close together,
   * many reads and events jostle on the way back, so an old snapshot overwrites a new one — and
   * since `stopped` is the final transition, no event is left to correct it. The row stays at
   * "Stopping" until someone presses Refresh.
   *
   * `useRef` rather than `useState`: this is a ledger of order, not something drawn, and a
   * `setState` here would re-render every time a message passes through.
   */
  const order = useRef(noReadsYet());

  /* Every error passes through here into a human-readable sentence. `errorMessage` translates the
     `code` and fills in `params`, so MixEngine's `hint` reaches the user intact instead of falling
     into a promise nobody catches — a tab standing still, empty and silent, is a worse outcome than
     any message. */
  const reload = useCallback(async () => {
    // Beside the read below, not inside it: see `readDisk` in `useDiskUsage`.
    void readDisk();
    /* A loop rather than recursion: a `useCallback` cannot call itself. It goes round once more
       exactly when an event slipped in during the read just done, and stops at the first read that
       was not interrupted — each round is a real round trip, so it limits its own pace. */
    for (;;) {
      const began = readBegan(order.current);
      order.current = began.order;
      let landed;
      try {
        const [next, list] = await Promise.all([api.status(), api.services()]);
        landed = readLanded(order.current, began.seq);
        order.current = landed.order;
        // A snapshot that set off before one already drawn must not be drawn: it carries older news
        // than what is on screen, even though it arrived later.
        if (landed.apply) {
          setStatus(next);
          setRows(rowsFrom(list.services));
          setWaiting(next.elevation?.pending ?? 0);
          setLoaded(true);
        }
        setError("");
      } catch (e) {
        // A failed read still has to land, otherwise `inFlight` never returns to 0 and every event
        // after that is treated as racing.
        landed = readLanded(order.current, began.seq);
        order.current = landed.order;
        setError(errorMessage(t, e));
        setLoaded(true);
      }
      if (!landed.readAgain) return;
    }
  }, [readDisk, setError, t]);

  /**
   * Opens the list of operations waiting for administrator rights.
   *
   * A tab opened while the queue already holds something receives **no** `elevation_required` —
   * that event only fires when the queue changes. So the number in `daemon.status` is the only
   * thing saying something is waiting, and `elevation.status` is where the list to show comes from.
   */
  const showWaiting = useCallback(async () => {
    try {
      const answer = await api.elevationStatus();
      setCanPrompt(answer.can_prompt);
      setReason(answer.reason);
      setPending(answer.pending);
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }, [setError, t]);

  /**
   * Sends an action and waits for it to finish. **Does not reread** — the caller decides when to
   * read.
   *
   * The row keeps changing with the stream all the while, per the "state is announced" rule.
   * Rereading is split out of here because one press of Stop all is *one* question, not N: see
   * [`stopAll`].
   */
  const run = useCallback(
    async (id: string, action: api.ServiceAction) => {
      setBusy((current) => ({ ...current, [id]: action }));
      try {
        await api.serviceAction(id, action);
      } catch (e) {
        setError(errorMessage(t, e));
      } finally {
        setBusy((current) => {
          const next = { ...current };
          delete next[id];
          return next;
        });
      }
    },
    [setError, t],
  );

  /**
   * An action on one row, then a reread.
   *
   * **Events are best-effort and never the only way to know the state**, so trusting the stream
   * alone leaves a frozen table when an event is dropped. Rereading is not guessing, it is reading
   * — and `order` above is what keeps that read from being overwritten by an older one.
   */
  const act = useCallback(
    async (id: string, action: api.ServiceAction) => {
      await run(id, action);
      await reload();
    },
    [reload, run],
  );

  /**
   * Stops everything that is running, then rereads **once**.
   *
   * Not a `Promise.all` of `act`: that fires N parallel `reload`s for one click, three RPCs each,
   * and they race each other — exactly what `order` above exists to prevent. Being able to prevent
   * it does not mean we should cause it: one click is one question, so ask once.
   */
  const stopAll = useCallback(async () => {
    await Promise.all(
      rows.filter((row) => row.state === "running").map((row) => run(row.id, "stop")),
    );
    await reload();
  }, [reload, rows, run]);

  // Reread on mount and every time we come back to this screen — the service_state_changed event
  // never reports that another service was created/deleted on the Services screen, and no
  // runtime-style method (installing/removing PHP...) emits anything this table would hear about;
  // coming back to the tab remains the fallback.
  useEffect(() => {
    if (active) void reload();
  }, [active, reload]);

  useEffect(() => {
    return subscribeDaemonWatch((raw) => {
      // An empty batch means nothing is waiting any more — close the dialog rather than leave it
      // standing there empty. `elevation_required` carries the latest count too: update `waiting`
      // straight from here without waiting for another `reload()` — otherwise the "N operations
      // waiting" button sits still with the old count after Allow, since this event itself was
      // never considered a reason to resync.
      const ops = pendingFrom(raw);
      if (ops !== null) {
        const grew = hasNewOps(announced.current, ops);
        announced.current = new Set(pendingIds(ops));
        setWaiting(ops.length);
        if (ops.length === 0) {
          setPending(null);
        } else if (grew && active) {
          // `elevation_required` only carries `pending` (exactly the shape
          // `{"type":"elevation_required","pending":[…]}`), not `can_prompt`/`reason` — read them
          // again through `elevation.status` before opening the dialog ourselves, to know whether
          // this machine can still raise the prompt (e.g. an old `hosts-apply` stuck in the queue
          // from before, while the helper has just been removed by an Uninstall).
          //
          // **Only while this screen is showing.** `MixEngineTab` keeps the Dashboard in the DOM
          // while the user is on another screen, and `Modal` draws through a portal, so a dialog
          // opened from here still floats above that screen — while the screen that started the
          // operation itself (CaBlock "Fix browser trust", Doctor "Repair") has already opened its
          // own dialog for this same queue: two identical modals, one grant job. While hidden, the
          // Dashboard only keeps the count for the "N waiting" button; whoever opens the tab sees
          // that button.
          void showWaiting();
        }
      }
      setJobs((current) => applyJob(current, raw));
      setCancelling((current) => forgetFinished(current, raw));
      // Events are best-effort: when the bus on the other side overflows or the connection drops,
      // reread instead of trusting what is on the screen. Outside the updater, because updaters
      // run twice under StrictMode. `job_finished` is also a reason to reread: a finished
      // `elevation.grant` changes the "N waiting" count with no event of its own saying so (see
      // `isJobFinished`).
      // A service that has just failed carries its sentence in `service.list` only.
      if (needsResync(raw) || isJobFinished(raw) || failureMayHaveChanged(raw)) void reload();
      // An event that changes a row, arriving while a `service.list` is on its way back, means that
      // snapshot may have been read *before* this event — the client cannot tell. Note it here so
      // that read requests one more when it lands. Outside the updater, for the same reason as the
      // two lines above.
      if (movesARow(raw)) order.current = eventArrived(order.current);
      setRows((current) => applyEvent(current, raw).rows);
    });
  }, [active, reload, showWaiting]);

  /**
   * Turns autostart on or off for a service — the same column `AutostartPanel` on the Services
   * screen changes.
   *
   * **Records what the daemon returns, not what was just clicked.** A row saying "yes" while the
   * column in the database still says "no" is worse than a row that changes slowly — the same rule
   * this whole table follows.
   *
   * Starts nothing and stops nothing, so it does not go through `busy`: what it changes is the walk
   * at the next daemon start.
   */
  const setAutostart = useCallback(
    async (id: string, autostart: boolean) => {
      try {
        const summary = await api.serviceSetAutostart({ service: id, autostart });
        setRows((current) =>
          current.map((row) => (row.id === id ? { ...row, autostart: summary.autostart } : row)),
        );
      } catch (e) {
        setError(errorMessage(t, e));
      }
    },
    [setError, t],
  );

  /**
   * Asks a running job to stop. No confirmation: a cancelled install is rerun with one click. The
   * button stays busy until `job_finished`, not until this call answers — cancellation is
   * cooperative, and the work may still be going when it does.
   */
  async function cancelJob(id: number) {
    setCancelling((current) => new Set(current).add(id));
    try {
      await api.jobCancel(id);
    } catch (e) {
      setCancelling((current) => {
        const next = new Set(current);
        next.delete(id);
        return next;
      });
      setError(errorMessage(t, e));
    }
  }

  return {
    status,
    rows,
    loaded,
    busy,
    jobs,
    cancelling,
    waiting,
    pending,
    setPending,
    canPrompt,
    reason,
    reload,
    showWaiting,
    act,
    stopAll,
    setAutostart,
    cancelJob,
  };
}
