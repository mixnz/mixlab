import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { MetricsFrame, SiteSummary } from "@mixengine/api";

import ActionBar from "../../../components/ActionBar";
import Button from "../../../components/Button";
import ErrorBanner from "../../../components/ErrorBanner";
import MonogramBadge from "../../../components/MonogramBadge";
import StatusPill, { type StatusTone } from "../../../components/StatusPill";
import { errorMessage } from "../../../core/errors";
import { IS_MAC, IS_WINDOWS } from "../../../core/platform";
import { hideTrayPanel, openMainWindow, quitApp } from "../../../core/window";
import { ChevronRightIcon, EngineIcon, GlobeIcon, LockIcon, PlayIcon, PowerIcon, StopIcon } from "../../../icons";
import { useTranslation } from "../../../i18n";
import * as api from "../api";
import DaemonUsage from "../components/DaemonUsage";
import { applyEvent, needsResync, rowsFrom, type ServiceRow } from "../daemonState";
import { ensureDaemonWatch, subscribeDaemonWatch } from "../daemonWatch";
import { serviceStateHint, serviceStateKey, serviceStateTone, toggleMode } from "../serviceStateLabel";
import { DAEMON_SUBJECT, parseMetricsFrame, readingFor, servicesTotal } from "../metricsState";
import { siteVisit } from "../siteState";
import { isFree } from "../storagePicker";
import {
  CONFIRM_TIMEOUT_MS,
  nextConfirm,
  serviceCounts,
  shutdownReport,
  type Confirmable,
  type ConfirmEvent,
  type ShutdownReport,
} from "./trayModel";
import styles from "./TrayPanel.module.css";

/* The popover slides in from the right on the two systems where it is one. On Linux it is an
   ordinary window the window manager placed, and content sliding about inside it would look broken. */
const SLIDES = IS_MAC || IS_WINDOWS;

/** How often an open panel asks whether a daemon has come up elsewhere — MixLab's Start, `mix`. */
const POLL_MS = 2000;

/** The slide out, to the millisecond of `TrayPanel.module.css`'s exit transition. */
const SLIDE_OUT_MS = 420;

/**
 * The tray panel — T168, the spec's D3. What the Dashboard would say about this machine, in 360
 * pixels: the daemon, its services with Start and Stop, *Stop all*, the sites, and the way into
 * MixLab and out of MixEngine.
 *
 * **It keeps nothing the Dashboard does not.** Every show reads `service.list` and `site.list`
 * again, and the event stream moves rows in between — the same `applyEvent` the Dashboard runs.
 */
function TrayPanel() {
  const { t } = useTranslation();
  const [presence, setPresence] = useState<api.Presence | null>(null);
  const [setupFree, setSetupFree] = useState(false);
  const [rows, setRows] = useState<ServiceRow[]>([]);
  const [sites, setSites] = useState<SiteSummary[]>([]);
  const [busy, setBusy] = useState<Record<string, api.ServiceAction>>({});
  const [working, setWorking] = useState<Confirmable | "start" | null>(null);
  const [confirm, setConfirm] = useState<Confirmable | null>(null);
  const [report, setReport] = useState<ShutdownReport | null>(null);
  const [error, setError] = useState("");
  /* Off to the right until the window is focused, and back there the moment it is not — so that
     the next show starts from the edge instead of flashing where it was. */
  const [shown, setShown] = useState(!SLIDES);
  /* On Linux the window is never slid away, so "shown" is always true there; what matters for the
     metrics stream is whether the window has the person's attention, which is focus. */
  const [focused, setFocused] = useState(false);
  const [frame, setFrame] = useState<MetricsFrame | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  /* The hide waiting for the slide out to finish — cancelled when the panel is shown again before
     it has, so a quick re-open is not hidden from under the person. */
  const leaving = useRef<ReturnType<typeof setTimeout> | null>(null);

  /**
   * Put the panel away: slide the card out, and only then hide the window. Hiding first would
   * leave the card where it was in the window's last frame, and the next show would flash it in
   * place before sliding it in. On Linux there is no slide, so the window goes at once.
   */
  const dismiss = useCallback(() => {
    if (!SLIDES) {
      void hideTrayPanel();
      return;
    }
    if (leaving.current !== null) return;
    setShown(false);
    leaving.current = setTimeout(() => {
      leaving.current = null;
      void hideTrayPanel();
    }, SLIDE_OUT_MS);
  }, []);

  const refresh = useCallback(async () => {
    try {
      const answer = await api.presence();
      setPresence(answer.presence);
      if (answer.presence === "running") {
        ensureDaemonWatch();
        const [list, siteList] = await Promise.all([api.services(), api.sites()]);
        setRows(rowsFrom(list.services));
        setSites(siteList.sites);
        setSetupFree(false);
      } else {
        setRows([]);
        setSites([]);
        // A home that never started still has its storage choice to make, and that is a screen of
        // MixLab's, not a question for a panel this size.
        setSetupFree(
          answer.presence === "notRunning" &&
            (await api.storage().then(isFree).catch(() => false)),
        );
      }
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }, [t]);

  const dispatchConfirm = useCallback((event: ConfirmEvent) => {
    setConfirm((current) => nextConfirm(current, event));
    if (event.type === "arm") {
      if (timer.current !== null) clearTimeout(timer.current);
      timer.current = setTimeout(
        () => setConfirm((current) => nextConfirm(current, { type: "timeout", key: event.key })),
        CONFIRM_TIMEOUT_MS,
      );
    }
  }, []);

  /* Read again every time the panel comes up, and put any open question away when it goes: a
     question left armed behind a hidden window would be answered by the next click on it. */
  useEffect(() => {
    void refresh();
    let unlisten: (() => void) | undefined;
    let live = true;
    void getCurrentWindow()
      .onFocusChanged(({ payload: focused }) => {
        setFocused(focused);
        if (focused) {
          if (leaving.current !== null) {
            clearTimeout(leaving.current);
            leaving.current = null;
          }
          setReport(null);
          void refresh();
          // One frame at the edge first, or the browser skips straight to the end of the slide.
          if (SLIDES) requestAnimationFrame(() => setShown(true));
        } else {
          dispatchConfirm({ type: "hide" });
        }
      })
      .then((stop) => {
        if (live) unlisten = stop;
        else stop();
      });
    return () => {
      live = false;
      unlisten?.();
    };
  }, [refresh, dispatchConfirm]);

  /* The daemon's CPU and memory, only while the panel is in front of somebody and the daemon is up.
     Opening `/metrics` is what makes the daemon sample every second, so a hidden panel holding it
     open would keep it doing so for nobody (`MetricsState`, and the Dashboard's same rule). */
  const measuring = shown && focused && presence === "running";
  useEffect(() => {
    if (!measuring) return;
    let live = true;
    void api
      .metricsWatch((raw) => {
        if (!live) return;
        const next = parseMetricsFrame(raw);
        if (next !== null) setFrame(next);
      })
      // A panel put away while the stream was still opening: the unwatch below ran first and
      // found nothing to close, so close what just opened.
      .then(() => {
        if (!live) void api.metricsUnwatch();
      })
      .catch(() => undefined);
    return () => {
      live = false;
      setFrame(null);
      void api.metricsUnwatch();
    };
  }, [measuring]);

  /* Stopped here and started from MixLab or `mix` while the panel is open: nothing else would tell
     it, because a stopped daemon has no event stream to send the news on. */
  useEffect(() => {
    if (!shown || presence === null || presence === "running") return;
    const poll = window.setInterval(() => void refresh(), POLL_MS);
    return () => window.clearInterval(poll);
  }, [shown, presence, refresh]);

  useEffect(
    () =>
      subscribeDaemonWatch((raw) => {
        if (needsResync(raw)) void refresh();
        setRows((current) => applyEvent(current, raw).rows);
      }),
    [refresh],
  );

  /* Every way the panel goes — a click elsewhere, a click on the icon, Open MixLab — arrives from
     `src-tauri/src/tray.rs` as this one event, so all of them slide out the same way. */
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let live = true;
    void getCurrentWindow()
      .listen("tray://dismiss", dismiss)
      .then((stop) => {
        if (live) unlisten = stop;
        else stop();
      });
    return () => {
      live = false;
      unlisten?.();
    };
  }, [dismiss]);

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") dismiss();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [dismiss]);

  useEffect(
    () => () => {
      if (timer.current !== null) clearTimeout(timer.current);
      if (leaving.current !== null) clearTimeout(leaving.current);
    },
    [],
  );

  async function act(id: string, action: api.ServiceAction) {
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
    await refresh();
  }

  async function perform(what: Confirmable | "start", run: () => Promise<void>) {
    dispatchConfirm({ type: "cancel" });
    setWorking(what);
    setError("");
    try {
      await run();
    } catch (e) {
      setError(errorMessage(t, e));
    }
    /* Read the new state before letting the button go: released first, it would say *Start
       MixEngine* again for the moment between the start returning and the panel learning the
       daemon is up. */
    try {
      await refresh();
    } finally {
      setWorking(null);
    }
  }

  async function visit(site: SiteSummary) {
    const { startProject, url } = siteVisit(site);
    try {
      if (startProject !== null) await api.serviceStartProject(startProject);
      await openUrl(url);
      dismiss();
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }

  function pillTone(row: ServiceRow): StatusTone {
    const tone = serviceStateTone(row.state, row.stoppedBy);
    if (tone === "ok") return "success";
    if (tone === "bad") return "danger";
    if (tone === "busy") return "warning";
    return "neutral";
  }

  function stateLabel(row: ServiceRow): string {
    const key = serviceStateKey(row.state, row.stoppedBy);
    return key === null ? (row.state ?? "—") : t(key);
  }

  function stateHint(row: ServiceRow): string | undefined {
    const key = serviceStateHint(row.state, row.stoppedBy);
    return key === null ? undefined : t(key);
  }

  const running = presence === "running";
  const counts = serviceCounts(rows);

  function armButton(key: Confirmable, label: string, variant: "default" | "danger", size: "small" | "normal") {
    return (
      <Button
        size={size}
        variant={variant}
        busy={working === key ? t("mixengine.dashboard.stopping") : undefined}
        disabled={working !== null && working !== key}
        onClick={() => dispatchConfirm({ type: "arm", key })}
      >
        {label}
      </Button>
    );
  }

  function confirmNow() {
    if (confirm === "shutdown") {
      void perform("shutdown", async () => {
        setReport(shutdownReport(await api.shutdown()));
      });
    }
  }

  const question = confirm === "shutdown" ? t("mixengine.tray.confirmShutdown") : null;

  return (
    <div className={styles.stage} data-slides={SLIDES || undefined}>
      <div className={`${styles.panel} ${shown ? styles.shown : ""}`}>
        <header className={styles.header}>
          <img className={styles.logo} src="/logo.svg" alt="" width={38} height={38} />
          <div className={styles.brand}>
            <span className={styles.name}>MixLab</span>
            <span className={styles.slogan}>{t("mixengine.tray.slogan")}</span>
          </div>
        </header>

        <div className={styles.body}>
          {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}

          {/* Who is talking, without a sentence saying so: the header is the application, MixLab;
              this card, with the engine's own mark, is the thing it drives. */}
          {running && (
            <section className={styles.engine}>
              <span className={styles.engineMark}>
                <EngineIcon size={18} />
              </span>
              <span className={styles.engineName}>MixEngine</span>
              <StatusPill tone="success">{t("mixengine.serviceState.running")}</StatusPill>
              <span className={styles.counts}>
                {t("mixengine.tray.counts", { up: counts.up, total: counts.total })}
              </span>
              <div className={styles.usages}>
                <DaemonUsage
                  className={styles.usage}
                  reading={readingFor(frame, DAEMON_SUBJECT)}
                  cores={frame?.cores ?? 1}
                />
                <DaemonUsage
                  className={styles.usage}
                  label={t("mixengine.tray.services")}
                  reading={servicesTotal(frame)}
                  cores={frame?.cores ?? 1}
                />
              </div>
            </section>
          )}

          {/* The daemon is not up: the same gate MixLab's tab draws, in the middle of the panel —
              without the four folders, which are a choice made in MixLab and not here. */}
          {presence !== null && !running && (
            <section className={styles.gate}>
              <span className={styles.gateMark}>
                <EngineIcon size={28} />
              </span>
              <p className={styles.gateTitle}>{t(`mixengine.gate.${presence}`)}</p>
              {report !== null && <ShutdownLines report={report} />}
              {presence === "notRunning" ? (
                setupFree ? (
                  <Button onClick={() => void openMainWindow()}>{t("mixengine.tray.setUp")}</Button>
                ) : (
                  <Button
                    busy={working === "start" ? t("mixengine.gate.starting") : undefined}
                    onClick={() =>
                      void perform("start", async () => {
                        await api.startDaemon();
                      })
                    }
                  >
                    {t("mixengine.gate.start")}
                  </Button>
                )
              ) : (
                <Button onClick={() => void openMainWindow()}>{t("mixengine.tray.openMain")}</Button>
              )}
            </section>
          )}

          {running && (
            <section className={styles.section} data-density="compact">
              <div className={styles.sectionHead}>
                <h2 className={styles.title}>{t("mixengine.tray.services")}</h2>
                {/* No question first, as on the Dashboard: the services start again with a click. */}
                {counts.up > 0 && (
                  <Button
                    size="small"
                    busy={working === "stopAll" ? t("mixengine.dashboard.stopping") : undefined}
                    disabled={working !== null && working !== "stopAll"}
                    onClick={() =>
                      void perform("stopAll", async () => {
                        await api.serviceStopAll();
                      })
                    }
                  >
                    {t("mixengine.tray.stopAll")}
                  </Button>
                )}
              </div>
              {rows.length === 0 ? (
                <p className={styles.empty}>{t("mixengine.tray.noServices")}</p>
              ) : (
                <ul className={styles.list}>
                  {rows.map((row) => {
                    const pending = busy[row.id];
                    const mode = toggleMode(row.state, pending !== undefined);
                    return (
                      <li key={row.id} className={styles.row}>
                        <MonogramBadge name={row.id} size={30} />
                        <div className={styles.rowText}>
                          <span className={styles.rowName} title={row.id}>
                            {row.id}
                          </span>
                          <StatusPill
                            className={styles.rowState}
                            tone={pending ? "warning" : pillTone(row)}
                            pulse={mode === "moving"}
                            title={pending ? undefined : stateHint(row)}
                          >
                            {pending
                              ? t(pending === "stop" ? "mixengine.dashboard.stopping" : "mixengine.dashboard.starting")
                              : stateLabel(row)}
                          </StatusPill>
                        </div>
                        {mode === "up" ? (
                          <Button
                            size="small"
                            className={styles.toggle}
                            aria-label={t("mixengine.dashboard.stopService", { service: row.id })}
                            onClick={() => void act(row.id, "stop")}
                          >
                            <StopIcon size={11} />
                            {t("mixengine.dashboard.stop")}
                          </Button>
                        ) : (
                          <Button
                            size="small"
                            variant="positive"
                            className={styles.toggle}
                            disabled={mode === "moving"}
                            aria-label={t("mixengine.dashboard.startService", { service: row.id })}
                            onClick={() => void act(row.id, "start")}
                          >
                            <PlayIcon size={11} />
                            {t("mixengine.dashboard.start")}
                          </Button>
                        )}
                      </li>
                    );
                  })}
                </ul>
              )}
            </section>
          )}

          {running && (
            <section className={styles.section} data-density="compact">
              <div className={styles.sectionHead}>
                <h2 className={styles.title}>{t("mixengine.tray.sites")}</h2>
              </div>
              {sites.length === 0 ? (
                <p className={styles.empty}>{t("mixengine.tray.noSites")}</p>
              ) : (
                <ul className={styles.list}>
                  {sites.map((site) => {
                    const SchemeIcon = site.https ? LockIcon : GlobeIcon;
                    return (
                      <li key={site.domain}>
                        <Button
                          variant="ghost"
                          className={styles.site}
                          title={t("mixengine.tray.openSite", { domain: site.domain })}
                          onClick={() => void visit(site)}
                        >
                          <SchemeIcon size={14} className={styles.siteIcon} />
                          <span className={styles.siteName}>{site.domain}</span>
                          <ChevronRightIcon size={14} className={styles.siteGo} />
                        </Button>
                      </li>
                    );
                  })}
                </ul>
              )}
            </section>
          )}
        </div>

        {/* One strip at the bottom either way: the three ways out, or the question the last
            click asked — never both squeezed onto one line. */}
        {question !== null ? (
          <footer
            className={`${styles.footer} ${styles.asking}`}
            data-density="compact"
            role="group"
            aria-label={question}
          >
            <span className={styles.question}>{question}</span>
            <Button onClick={() => dispatchConfirm({ type: "cancel" })}>{t("mixengine.tray.cancel")}</Button>
            <Button variant="danger" onClick={confirmNow}>
              {t("mixengine.tray.confirm")}
            </Button>
          </footer>
        ) : (
          <footer className={styles.footer} data-density="compact">
            <Button variant="primary" className={styles.footerAction} onClick={() => void openMainWindow()}>
              {t("mixengine.tray.openMain")}
            </Button>
            {running ? (
              <span className={styles.footerAction}>
                {armButton("shutdown", t("mixengine.tray.shutdown"), "danger", "normal")}
              </span>
            ) : (
              <span className={styles.footerAction} />
            )}
            <ActionBar
              actions={[
                {
                  key: "quit",
                  icon: PowerIcon,
                  // Whether MixEngine goes on without MixLab is worth saying only while it is running.
                  label: t(running ? "mixengine.tray.quit" : "mixengine.tray.quitAlone"),
                  onClick: () => void quitApp(),
                },
              ]}
            />
          </footer>
        )}
      </div>
    </div>
  );
}

/** What `daemon.shutdown` answered, under the gate it left behind. */
function ShutdownLines({ report }: { report: ShutdownReport }) {
  const { t } = useTranslation();
  return (
    <div className={styles.report} role="status">
      <p>{t("mixengine.tray.shutdownDone", { count: report.stopped })}</p>
      {report.failed !== null && <p>{t("mixengine.tray.shutdownFailed", { service: report.failed })}</p>}
      {report.unordered !== null && <p>{t("mixengine.tray.unordered", { message: report.unordered })}</p>}
    </div>
  );
}

export default TrayPanel;
