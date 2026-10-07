import { useCallback, useEffect, useRef, useState } from "react";

import ActionBar from "../../../../components/ActionBar";
import Button from "../../../../components/Button";
import ConfirmDialog from "../../../../components/ConfirmDialog";
import ContextMenu from "../../../../components/ContextMenu";
import Card from "../../../../components/Card";
import EmptyState from "../../../../components/EmptyState";
import ErrorBanner from "../../../../components/ErrorBanner";
import LoadingState from "../../../../components/LoadingState";
import MonogramBadge from "../../../../components/MonogramBadge";
import PageHeader from "../../../../components/PageHeader";
import SegmentedControl from "../../../../components/SegmentedControl";
import StatusPill, { type StatusTone } from "../../../../components/StatusPill";
import Switch from "../../../../components/Switch";
import Table from "../../../../components/Table";
import {
  CopyIcon,
  DatabaseGenericIcon,
  FolderIcon,
  LockIcon,
  LogIcon,
  MoreIcon,
  PlayIcon,
  PlusIcon,
  PowerIcon,
  ReloadIcon,
  StopIcon,
} from "../../../../icons";
import { copyText } from "../../../../core/clipboard";
import { serialQueue } from "../../../../core/serialQueue";
import { useWindowFocused } from "../../../../core/windowFocus";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { DaemonStatus } from "@mixengine/api";
import type { DatabaseClientReport } from "@mixengine/api";
import type { DatabaseCredentials } from "@mixengine/api";
import type { StoppedBy } from "@mixengine/api";
import type { DiskUsage } from "@mixengine/api";
import CredentialDialog from "../../components/CredentialDialog";
import ElevationDialog from "../../components/ElevationDialog";
import ServiceForm from "../../components/ServiceForm";
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
import CpuRing from "../../components/CpuRing";
import DaemonUsage from "../../components/DaemonUsage";
import { serviceBadge } from "../../serviceBadge";
import { useCpuScale } from "../../cpuScale";
import type { MetricsFrame } from "@mixengine/api";
import {
  DAEMON_SUBJECT,
  formatBytes,
  cpuShare,
  formatCpu,
  metricsSubjectFor,
  parseMetricsFrame,
  readingFor,
} from "../../metricsState";
import { pendingFrom } from "../../pendingOps";
import {
  DATABASE_MODULE_ID,
  openChoices,
  opensADatabase,
} from "../ServicesDetail/openChoices";
import { eventArrived, noReadsYet, readBegan, readLanded } from "../../readOrder";
import { serviceStateHint, serviceStateKey, serviceStateTone, toggleMode } from "../../serviceStateLabel";
import CleanupDialog from "./CleanupDialog";
import DiskUsagePanel from "./DiskUsagePanel";
import FoundServices from "./FoundServices";
import RestorePrevious from "./RestorePrevious";
import PathNudge from "./PathNudge";
import QuickStart from "./QuickStart";
import { shouldOfferQuickStart } from "../../quickStart";
import type { SiteSummary } from "@mixengine/api";
import styles from "./Dashboard.module.css";

/* An explicit lookup table rather than building `${action}ing`: "stop" + "ing" gives "stoping", and
   a translation key built by string concatenation is a key nobody can grep for. */
const PENDING_LABEL = {
  start: "mixengine.dashboard.starting",
  stop: "mixengine.dashboard.stopping",
  restart: "mixengine.dashboard.restarting",
} as const;

type ServiceFilter = "all" | "running" | "stopped";

/**
 * The daemon, and everything it supervises.
 *
 * **State comes from the stream, not from guessing.** Pressing Start moves the row to `starting`
 * when `service_state_changed` says so, not at the moment of the click — a switch that lies about
 * whether MariaDB is running is worse than a slow switch.
 */
export default function Dashboard({
  active,
  isModuleVisible,
  onViewLogs,
}: {
  active: boolean;
  /** Opens the Logs screen on one service — the row menu's *View logs*. */
  onViewLogs: (serviceId: string) => void;
  /** Whether this window draws the built-in database client — T110. Straight through to the row
   *  menu, which is where *open* lives now. */
  isModuleVisible: (moduleId: string) => boolean;
}) {
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
  const [jobs, setJobs] = useState<JobRow[]>([]);
  /** The jobs Cancel was pressed for, until their `job_finished` — see `forgetFinished`. */
  const [cancelling, setCancelling] = useState<ReadonlySet<number>>(() => new Set());
  /** The latest `/metrics` frame, or `null` when there is none yet (the stream is not open, or no
   *  frame has arrived). */
  const [frame, setFrame] = useState<MetricsFrame | null>(null);
  const focused = useWindowFocused();
  const [disk, setDisk] = useState<DiskUsage | null>(null);
  /** `site.list`, or `null` when not finished reading — the condition for drawing the Quick Start
   *  card (T117). */
  const [sites, setSites] = useState<SiteSummary[] | null>(null);
  const [refreshingDisk, setRefreshingDisk] = useState(false);
  const [cleaning, setCleaning] = useState(false);
  const [error, setError] = useState("");
  const [creating, setCreating] = useState(false);
  /** Which services have an action in flight, and which action. Keyed by id. */
  const [busy, setBusy] = useState<Record<string, api.ServiceAction>>({});
  /** A row's menu, and where it was opened. `null` means no menu is open. */
  const [menu, setMenu] = useState<{ id: string; x: number; y: number } | null>(null);
  /**
   * `database.client` for each service, looked up **once per id** and then remembered.
   *
   * **It has to be known before the click, so it cannot be looked up when the menu opens.** The ⋮
   * button of a service that is not a database has to be greyed out from the moment it is drawn —
   * opening an empty menu and only then finding out is worse than not inviting the click.
   *
   * One lookup per id is acceptable because **the answer never changes**: it is the recipe's
   * protocol, i.e. a property of the package this service runs. So the first `service.list` read
   * pays N lookups, and every `reload()` after that — every return to the tab, every `resync` —
   * pays 0.
   *
   * An id being absent means not finished asking *or* asked and failed, and both draw a greyed-out
   * button.
   */
  const [databases, setDatabases] = useState<Record<string, DatabaseClientReport>>({});
  /** The ids whose question has been sent, so `rows` changing with the stream does not turn into a
   *  barrage of RPCs. */
  const asked = useRef(new Set<string>());
  /** The password shown in the dialog, or `null`. Never kept in `rows`. */
  const [credentials, setCredentials] = useState<DatabaseCredentials | null>(null);
  /** The service being asked "reset the password?", or `null`. */
  const [resetTarget, setResetTarget] = useState<string | null>(null);
  /** Which rows the Services card shows. */
  const [filter, setFilter] = useState<ServiceFilter>("all");
  /** The home path was just copied, for the button to say so for a moment. */
  const [homeCopied, setHomeCopied] = useState(false);
  const { t } = useTranslation();
  const cpuScale = useCpuScale();

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
    /* A loop rather than recursion: a `useCallback` cannot call itself. It goes round once more
       exactly when an event slipped in during the read just done, and stops at the first read that
       was not interrupted — each round is a real round trip, so it limits its own pace. */
    for (;;) {
      const began = readBegan(order.current);
      order.current = began.order;
      let landed;
      try {
        const [next, list, usage] = await Promise.all([
          api.status(),
          api.services(),
          api.diskUsage(false),
        ]);
        landed = readLanded(order.current, began.seq);
        order.current = landed.order;
        // A snapshot that set off before one already drawn must not be drawn: it carries older news
        // than what is on screen, even though it arrived later.
        if (landed.apply) {
          setStatus(next);
          setRows(rowsFrom(list.services));
          setWaiting(next.elevation?.pending ?? 0);
          setDisk(usage);
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
  }, [t]);

  /**
   * Whether this home has any site yet — the condition for drawing the Quick Start card (T117).
   *
   * Read separately from `reload()` rather than folded into its `Promise.all`: `reload()` runs
   * again on every return to the tab and every time the stream says something changed, while this
   * question only changes when a site is created or deleted. On failure the old value stays and no
   * error banner is raised: a Dashboard turned red because it could not ask "is there a site yet"
   * is a Dashboard turned red over a decorative question.
   */
  const readSites = useCallback(async () => {
    try {
      const listed = await api.sites();
      setSites(listed.sites);
    } catch {
      // Leave it: `null` is still "unknown", and `shouldOfferQuickStart` does not invite on `null`.
    }
  }, []);

  /** The disk usage table's "Refresh" button — `refresh: true` walks the disk again, unlike
   *  `reload()` above, which reads the copy the daemon keeps (up to a minute old) so that every
   *  return to the tab does not become a disk walk. */
  const refreshDisk = useCallback(async () => {
    setRefreshingDisk(true);
    try {
      setDisk(await api.diskUsage(true));
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setRefreshingDisk(false);
    }
  }, [t]);

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
  }, [t]);

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
    [t],
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

  // The same beat, a separate call: see `readSites`.
  useEffect(() => {
    if (active) void readSites();
  }, [active, readSites]);

  /**
   * `/metrics` ties its lifetime to `active`, not to mount/unmount like `/events`.
   *
   * **Opening this connection is the subscription** — MixEngine samples at 1 Hz while anyone holds
   * the stream, and once a minute when nobody does. `MixEngineTab.tsx` keeps every visited screen
   * in the DOM instead of unmounting on a tab switch, so if this were tied to unmount, leaving the
   * Dashboard for another screen would close nothing — the daemon would be stuck sampling fast
   * forever with nobody looking. The effect cleanup runs in both cases (`active` turning `false`,
   * and a real unmount), so tying it to `active` covers both.
   *
   * **And only while the window has focus**, the tray panel's rule. `active` says Dashboard is the
   * tab in front, not that anybody is looking: a window left on Dashboard behind another
   * application held the stream open for hours and kept the daemon sampling every second.
   *
   * **Opening and closing go through one queue.** Focus can leave and come back while a stream is
   * still connecting, and the window keeps one `/metrics` slot: a close sent beside the next
   * opening could land after it and end the stream the Dashboard now wants. Queued, each close
   * follows the opening it belongs to, and the next opening follows that close.
   */
  const measuring = active && focused;
  const [metricsQueue] = useState(serialQueue);
  useEffect(() => {
    if (!measuring) return;
    let live = true;
    metricsQueue(async () => {
      // Put away before its turn came: nothing to open.
      if (!live) return;
      await api.metricsWatch((raw) => {
        if (!live) return;
        const next = parseMetricsFrame(raw);
        if (next !== null) setFrame(next);
      });
    });
    return () => {
      live = false;
      setFrame(null);
      metricsQueue(() => api.metricsUnwatch());
    };
  }, [measuring, metricsQueue]);

  useEffect(() => {
    return subscribeDaemonWatch((raw) => {
      // An empty batch means nothing is waiting any more — close the dialog rather than leave it
      // standing there empty. `elevation_required` carries the latest count too: update `waiting`
      // straight from here without waiting for another `reload()` — otherwise the "N operations
      // waiting" button sits still with the old count after Allow, since this event itself was
      // never considered a reason to resync.
      const ops = pendingFrom(raw);
      if (ops !== null) {
        setWaiting(ops.length);
        if (ops.length === 0) {
          setPending(null);
        } else if (active) {
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
      if (needsResync(raw) || isJobFinished(raw)) void reload();
      // An event that changes a row, arriving while a `service.list` is on its way back, means that
      // snapshot may have been read *before* this event — the client cannot tell. Note it here so
      // that read requests one more when it lands. Outside the updater, for the same reason as the
      // two lines above.
      if (movesARow(raw)) order.current = eventArrived(order.current);
      setRows((current) => applyEvent(current, raw).rows);
    });
  }, [active, reload, showWaiting]);



  /**
   * Asks `database.client` for the ids never asked about.
   *
   * **`ServiceSummary` cannot answer this.** `ServiceRole` only tells the front end from everything
   * else, and [ADR 0026] forbids the client from inferring a role from the package name — so
   * `database.client` is the only way. Driven by `rows` because that is where a new service shows
   * up.
   *
   * A failed question **removes the id from `asked`**: the next `reload()` asks again. Keeping it
   * would let a passing glitch lock that row's ⋮ button until the window is closed.
   *
   * [ADR 0026]: https://github.com/mixnz/mixlab/blob/master/docs/decisions/0026-the-active-front-end-is-a-row-and-switching-it-is-a-job.md
   */
  useEffect(() => {
    const missing = rows.map((row) => row.id).filter((id) => !asked.current.has(id));
    if (missing.length === 0) return;
    for (const id of missing) asked.current.add(id);

    void (async () => {
      const answers = await Promise.all(
        missing.map(async (id) => {
          try {
            return [id, await api.databaseClient(id)] as const;
          } catch {
            asked.current.delete(id);
            return [id, null] as const;
          }
        }),
      );
      setDatabases((current) => {
        const next = { ...current };
        for (const [id, report] of answers) if (report !== null) next[id] = report;
        return next;
      });
    })();
  }, [rows]);

  /** Fetches a service's administrator password and opens the dialog. */
  const showCredentials = useCallback(
    async (id: string) => {
      try {
        setCredentials(await api.databaseCredentials(id));
      } catch (e) {
        setError(errorMessage(t, e));
      }
    },
    [t],
  );

  /** Runs `service.reset_credential`, then rereads — it stops and restarts several services. */
  const resetCredential = useCallback(
    async (id: string) => {
      try {
        await api.serviceResetCredential(id);
      } catch (e) {
        setError(errorMessage(t, e));
      } finally {
        await reload();
      }
    },
    [reload, t],
  );


  /**
   * Turns autostart on or off for a service — the same column `AutostartPanel` on the Services
   * screen changes.
   *
   * **Records what the daemon returns, not what was just clicked.** A row saying "yes" while the
   * column in the database still says "no" is worse than a row that changes slowly — the same rule
   * this whole table follows.
   *
   * Starts nothing and stops nothing, so it does not go through `busy`: what it changes is the walk
   * at the next daemon start (T112/T113).
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
    [t],
  );

  /** The row whose menu is open. The menu only lives with one `menu.id`, but the table updates from
   *  the stream, so read it again from `rows` instead of snapshotting the row when it opened — the
   *  autostart label has to follow the column next to it. */
  const menuRow = menu === null ? undefined : rows.find((row) => row.id === menu.id);

  /** The answer for the row whose menu is open, bound to a name: reading `databases[menu.id]` twice
   *  makes TypeScript lose the type narrowing on `client`.
   *
   *  `opensADatabase` filters right here rather than at the ⋮ button: `database.client` also
   *  answers for nginx and for a php-fpm pool, just with `protocol: null`, so being present in
   *  `databases` does not mean there is anything to open. */
  const menuReport =
    menuRow === undefined ? undefined : databases[menuRow.id];
  const menuDatabase =
    menuReport !== undefined && opensADatabase(menuReport) ? menuReport : undefined;

  /** The state as a pill tone: whether it is serving, not which of the seven states it is in. */
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

  function pillTone(state: string | null | undefined, stoppedBy?: StoppedBy | null): StatusTone {
    const tone = serviceStateTone(state, stoppedBy);
    if (tone === "ok") return "success";
    if (tone === "bad") return "danger";
    if (tone === "busy") return "warning";
    return "neutral";
  }

  /** The translated state; a state from a daemon newer than this build is shown verbatim. */
  function stateLabel(state: string | null | undefined, stoppedBy?: StoppedBy | null): string {
    const key = serviceStateKey(state, stoppedBy);
    return key === null ? (state ?? "—") : t(key);
  }

  /** The sentence the short label leaves out, for the pill's tooltip. */
  function stateHint(state: string | null | undefined, stoppedBy?: StoppedBy | null): string | undefined {
    const key = serviceStateHint(state, stoppedBy);
    return key === null ? undefined : t(key);
  }

  const runningCount = rows.filter((row) => isServing(row.state)).length;
  const movingCount = rows.filter(
    (row) => busy[row.id] !== undefined || toggleMode(row.state, false) === "moving",
  ).length;
  const shown = rows.filter((row) =>
    filter === "all" ? true : filter === "running" ? isServing(row.state) : !isServing(row.state),
  );
  const daemon = readingFor(frame, DAEMON_SUBJECT);
  const rowIds = rows.map((row) => row.id);

  function copyHome(home: string) {
    void copyText(home).then(() => {
      setHomeCopied(true);
      window.setTimeout(() => setHomeCopied(false), 1600);
    });
  }

  return (
    <div className={`mixengine-page ${styles.dashboard}`}>
      {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}

      <PageHeader
        title={t("mixengine.sidebar.dashboard")}
        badges={
          status && (
            <>
              {/* Green, like the tray's "Running" pill: this page exists only while the daemon
                  answers, so the engine it names is up. A neutral dot here read as "stopped"
                  beside the services pill, about the one thing on the page that cannot be. */}
              <StatusPill tone="success" className={styles.version}>
                {t("mixengine.dashboard.versionBadge", { version: status.version })}
              </StatusPill>
              {movingCount > 0 ? (
                <StatusPill tone="warning" pulse>
                  {t("mixengine.dashboard.summaryChanging", { count: movingCount })}
                </StatusPill>
              ) : runningCount > 0 ? (
                <StatusPill tone="success">
                  {t("mixengine.dashboard.summaryRunning", { running: runningCount, total: rows.length })}
                </StatusPill>
              ) : (
                rows.length > 0 && (
                  <StatusPill tone="neutral">{t("mixengine.dashboard.summaryStopped")}</StatusPill>
                )
              )}
            </>
          )
        }
        meta={
          status && (
            <div className={styles.metaRow}>
              <div className={styles.home}>
                <FolderIcon size={14} className={styles.homeIcon} />
                <span className={styles.homePath} title={status.home}>
                  {status.home}
                </span>
                <Button
                  size="small"
                  variant="ghost"
                  className={styles.copy}
                  onClick={() => copyHome(status.home)}
                  aria-label={t("mixengine.dashboard.copyHome")}
                >
                  <CopyIcon size={13} />
                  {homeCopied ? t("mixengine.dashboard.copied") : t("mixengine.dashboard.copy")}
                </Button>
              </div>
              {/* The daemon has no `ServiceRow` — drawn apart from the service table, not inserted
                  into `rows`. Always drawn, even before the first frame: the frame stands ready
                  with "—" instead of appearing later and pushing the whole screen down — `frame`
                  goes back to `null` every time the tab is left, so that jump would repeat on every
                  return. */}
              <DaemonUsage inline reading={daemon} cores={frame?.cores ?? 1} />
            </div>
          )
        }
        actions={
          <div className={styles.headerButtons}>
            {/* The dialog does not open by itself when the tab opens: a batch may wait for days,
                and a modal popping up every time the tab opens is something people learn to
                dismiss without reading. */}
            {waiting > 0 && pending === null && (
              <Button size="large" className={styles.waiting} onClick={() => void showWaiting()}>
                {t("mixengine.dashboard.elevationWaiting", { count: waiting })}
              </Button>
            )}
            {/* The manual fallback: a service created/deleted elsewhere produces no event for this
                table to hear about. */}
            <Button size="large" onClick={() => void reload()}>
              <ReloadIcon size={15} />
              {t("mixengine.dashboard.reload")}
            </Button>
            {/* No row changes here: the table changes when `service_state_changed` arrives, not on
                the click. */}
            <Button
              size="large"
              variant="danger"
              onClick={() => void stopAll()}
              disabled={rows.every((row) => row.state !== "running") || Object.keys(busy).length > 0}
            >
              <StopIcon size={15} />
              {t("mixengine.dashboard.stopAll")}
            </Button>
            <Button size="large" variant="primary" onClick={() => setCreating(true)}>
              <PlusIcon size={15} />
              {t("mixengine.dashboard.newService")}
            </Button>
          </div>
        }
      />

      {/* Above the service table, and only when this home has no site yet — T117. */}
      {shouldOfferQuickStart(sites) && <QuickStart onCreated={() => void readSites()} />}
      <PathNudge active={active} />
      {/* T182h: a copy of an earlier install's state, while this home has nothing of its own. */}
      <RestorePrevious active={active} onRestored={() => void reload()} />
      {/* T182g: service data an earlier install left, until nothing is left to adopt. */}
      <FoundServices active={active} onAdopted={() => void reload()} />

      <Card
        flush
        title={t("mixengine.dashboard.servicesTitle")}
        description={t("mixengine.dashboard.servicesAbout")}
        actions={
          <SegmentedControl<ServiceFilter>
            aria-label={t("mixengine.dashboard.servicesTitle")}
            value={filter}
            onChange={setFilter}
            segments={[
              { value: "all", label: t("mixengine.dashboard.filterAll"), count: loaded ? rows.length : undefined },
              {
                value: "running",
                label: t("mixengine.dashboard.filterRunning"),
                count: loaded ? runningCount : undefined,
              },
              {
                value: "stopped",
                label: t("mixengine.dashboard.filterStopped"),
                count: loaded ? rows.length - runningCount : undefined,
              },
            ]}
          />
        }
      >
        {/* Progress is drawn in place, not as a spinner over the whole screen. */}
        {jobs.length > 0 && (
          <ul className={styles.jobs}>
            {jobs.map((job) => (
              <li key={job.id}>
                <span>{job.kind || t("mixengine.dashboard.job")}</span>
                <progress value={job.percent} max={100} />
                <span className={styles.jobMessage}>{job.message}</span>
                <Button
                  size="small"
                  className={styles.jobCancel}
                  onClick={() => void cancelJob(job.id)}
                  busy={cancelling.has(job.id) ? t("mixengine.dashboard.cancelling") : undefined}
                >
                  {t("mixengine.dashboard.cancelJob")}
                </Button>
              </li>
            ))}
          </ul>
        )}

        {!loaded ? (
          <LoadingState />
        ) : rows.length === 0 ? (
          <EmptyState title={t("mixengine.dashboard.noServices")} />
        ) : shown.length === 0 ? (
          <EmptyState
            title={
              filter === "running"
                ? t("mixengine.dashboard.filterEmptyRunning")
                : t("mixengine.dashboard.filterEmptyStopped")
            }
            action={
              <Button size="small" onClick={() => setFilter("all")}>
                {t("mixengine.dashboard.showAll")}
              </Button>
            }
          />
        ) : (
          <Table aria-label={t("mixengine.dashboard.servicesTitle")}>
            <thead>
              <tr>
                <th>{t("mixengine.dashboard.service")}</th>
                <th>{t("mixengine.dashboard.state")}</th>
                {/* Next to State — T114: what is running, and what will run after the next
                    login. */}
                <th>{t("mixengine.dashboard.autostart")}</th>
                <th>{t("mixengine.dashboard.port")}</th>
                <th>{t("mixengine.dashboard.cpu")}</th>
                <th>{t("mixengine.dashboard.memory")}</th>
                <th data-align="end">{t("mixengine.dashboard.actions")}</th>
              </tr>
            </thead>
            <tbody>
              {shown.map((row) => {
                const reading = readingFor(frame, metricsSubjectFor(row.id));
                const mode = toggleMode(row.state, busy[row.id] !== undefined);
                const badge = serviceBadge(row.id, rowIds);
                return (
                  <tr key={row.id}>
                    <td>
                      <span className={styles.service}>
                        <MonogramBadge name={badge.name} tag={badge.tag} size={34} />
                        <span className={styles.serviceName} title={row.id}>
                          {row.id}
                        </span>
                        {row.version !== null && <span className={styles.serviceVersion}>{row.version}</span>}
                      </span>
                    </td>
                    <td data-nowrap>
                      {busy[row.id] ? (
                        /* An action just sent with no event confirming it yet: also "in
                           transition", so the same tone as `starting`/`stopping`. */
                        <StatusPill tone="warning" pulse>
                          {t(PENDING_LABEL[busy[row.id]])}
                        </StatusPill>
                      ) : (
                        <StatusPill
                          tone={pillTone(row.state, row.stoppedBy)}
                          pulse={mode === "moving"}
                          title={stateHint(row.state, row.stoppedBy)}
                        >
                          {stateLabel(row.state, row.stoppedBy)}
                        </StatusPill>
                      )}
                    </td>
                    <td>
                      <Switch
                        small
                        checked={row.autostart}
                        onChange={(next) => void setAutostart(row.id, next)}
                        aria-label={t(
                          row.autostart
                            ? "mixengine.dashboard.autostartOff"
                            : "mixengine.dashboard.autostartOn",
                        )}
                      />
                    </td>
                    <td className={row.port === null ? styles.none : styles.mono} data-nowrap>{row.port ?? "—"}</td>
                    {/* Absent from the frame is "—", not 0%: an idle service and a service that
                        could not be measured are two different statements. */}
                    <td className={styles.mono} data-nowrap>
                      <span className={styles.cpu}>
                        {reading !== null && (
                          <CpuRing size={18} share={cpuShare(reading.cpu_percent ?? 0, frame?.cores ?? 1, cpuScale)} />
                        )}
                        {formatCpu(reading?.cpu_percent ?? null, frame?.cores ?? 1, cpuScale)}
                      </span>
                    </td>
                    <td className={styles.mono} data-nowrap>
                      {reading === null ? "—" : formatBytes(reading.rss_bytes)}
                    </td>
                    <td data-align="end" data-nowrap>
                      <span className={styles.actions}>
                        {mode === "moving" ? (
                          <Button
                            size="small"
                            className={styles.toggle}
                            busy={busy[row.id] ? t(PENDING_LABEL[busy[row.id]]) : stateLabel(row.state, row.stoppedBy)}
                            aria-label={t("mixengine.dashboard.moving", { service: row.id })}
                          />
                        ) : mode === "up" ? (
                          <Button
                            size="small"
                            className={`${styles.toggle} ${styles.stop}`}
                            aria-label={t("mixengine.dashboard.stopService", { service: row.id })}
                            onClick={() => void act(row.id, "stop")}
                          >
                            <StopIcon size={13} className={styles.stopMark} />
                            {t("mixengine.dashboard.stop")}
                          </Button>
                        ) : (
                          <Button
                            size="small"
                            variant="positive"
                            className={styles.toggle}
                            aria-label={t("mixengine.dashboard.startService", { service: row.id })}
                            onClick={() => void act(row.id, "start")}
                          >
                            <PlayIcon size={13} />
                            {t("mixengine.dashboard.start")}
                          </Button>
                        )}
                        {/* A stopped service has nothing to restart: Start alone, not a greyed
                            Restart beside it. */}
                        {mode !== "down" && (
                          <Button
                            size="small"
                            onClick={() => void act(row.id, "restart")}
                            disabled={busy[row.id] !== undefined || mode !== "up"}
                          >
                            <ReloadIcon size={13} />
                            {t("mixengine.dashboard.restart")}
                          </Button>
                        )}
                        {/* Never greyed: autostart and the logs are there for *every* service. */}
                        <ActionBar
                          actions={[
                            {
                              key: "menu",
                              icon: MoreIcon,
                              label: t("mixengine.dashboard.rowMenu"),
                              onClick: (event) => {
                                const at = event.currentTarget.getBoundingClientRect();
                                setMenu({ id: row.id, x: at.left, y: at.bottom });
                              },
                            },
                          ]}
                        />
                      </span>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </Table>
        )}
        {loaded && rows.length > 0 && (
          <div className={styles.footer}>
            {t("mixengine.dashboard.footerSummary", {
              total: rows.length,
              running: runningCount,
              stopped: rows.length - runningCount,
            })}
          </div>
        )}
      </Card>

      <DiskUsagePanel
        disk={disk}
        refreshing={refreshingDisk}
        onRefresh={() => void refreshDisk()}
        onCleanup={() => setCleaning(true)}
      />

      {cleaning && disk && (
        <CleanupDialog
          disk={disk}
          onCancel={() => setCleaning(false)}
          onStarted={() => setCleaning(false)}
        />
      )}

      {/* This menu is never empty: autostart and logs are there for *every* service. */}
      {menu !== null && (
        <ContextMenu x={menu.x} y={menu.y} onClose={() => setMenu(null)}>
          <button
            type="button"
            onClick={() => {
              const id = menu.id;
              setMenu(null);
              onViewLogs(id);
            }}
          >
            <LogIcon size={14} />
            {t("mixengine.dashboard.viewLogs")}
          </button>

          {menuRow !== undefined && (
            <button
              type="button"
              disabled={menuRow.port === null}
              onClick={() => {
                const port = menuRow.port;
                setMenu(null);
                if (port !== null) void copyText(String(port));
              }}
            >
              <CopyIcon size={14} />
              {t("mixengine.dashboard.copyPort")}
            </button>
          )}

          {/* The label flips with that row's own Autostart column, rather than being a tick. */}
          {menuRow !== undefined && (
            <button
              type="button"
              onClick={() => {
                const id = menuRow.id;
                const wanted = !menuRow.autostart;
                setMenu(null);
                void setAutostart(id, wanted);
              }}
            >
              <PowerIcon size={14} />
              {t(
                menuRow.autostart
                  ? "mixengine.dashboard.autostartOff"
                  : "mixengine.dashboard.autostartOn",
              )}
            </button>
          )}

          {menuDatabase !== undefined && (
            <>
              <div className="context-menu-separator" />
              {openChoices(menuDatabase.client, isModuleVisible(DATABASE_MODULE_ID)).map((choice) => (
                <button
                  key={choice}
                  type="button"
                  onClick={() => {
                    const id = menu.id;
                    setMenu(null);
                    // A `db` tab in this window; the password travels neither way through here (T83).
                    void api.databaseExploreData(id).catch((e: unknown) => setError(errorMessage(t, e)));
                  }}
                >
                  <DatabaseGenericIcon size={14} />
                  {t(
                    choice === "builtIn"
                      ? "mixengine.dashboard.exploreData"
                      : "mixengine.dashboard.exploreDataEnabling",
                  )}
                </button>
              ))}

              <button
                type="button"
                onClick={() => {
                  const id = menu.id;
                  setMenu(null);
                  void showCredentials(id);
                }}
              >
                <LockIcon size={14} />
                {t("mixengine.dashboard.credentials")}
              </button>

              {/* The ellipsis is a promise: clicking opens a question, it does not run right
                  away. */}
              <button
                type="button"
                onClick={() => {
                  const id = menu.id;
                  setMenu(null);
                  setResetTarget(id);
                }}
              >
                <ReloadIcon size={14} />
                {t("mixengine.dashboard.resetCredential")}
              </button>
            </>
          )}
        </ContextMenu>
      )}

      {credentials !== null && (
        <CredentialDialog credentials={credentials} onClose={() => setCredentials(null)} />
      )}

      {/* Not `danger`: `ConfirmDialog` keeps that colour for operations that **lose data**, and
          this one keeps every database intact. */}
      {resetTarget !== null && (
        <ConfirmDialog
          title={t("mixengine.dashboard.resetTitle")}
          message={t("mixengine.dashboard.resetMessage", { service: resetTarget })}
          confirmLabel={t("mixengine.dashboard.resetConfirm")}
          onConfirm={() => {
            const id = resetTarget;
            setResetTarget(null);
            void resetCredential(id);
          }}
          onCancel={() => setResetTarget(null)}
        />
      )}

      {creating && (
        <ServiceForm
          onCancel={() => setCreating(false)}
          onCreated={() => {
            setCreating(false);
            void reload();
          }}
        />
      )}

      {pending && (
        <ElevationDialog
          pending={pending}
          canPrompt={canPrompt}
          reason={reason}
          onClose={() => {
            setPending(null);
            // After a grant or a drop the queue is different: reread the count instead of keeping
            // the old one.
            void reload();
          }}
        />
      )}
    </div>
  );
}

/** Serving, for the filter and the summary: `degraded` still answers. */
function isServing(state: string | null | undefined): boolean {
  return state === "running" || state === "degraded";
}
