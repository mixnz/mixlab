import { Fragment, useCallback, useEffect, useState } from "react";

import Button from "../../../../components/Button";
import ConfirmDialog from "../../../../components/ConfirmDialog";
import EmptyState from "../../../../components/EmptyState";
import ErrorBanner from "../../../../components/ErrorBanner";
import LoadingState from "../../../../components/LoadingState";
import MonogramBadge from "../../../../components/MonogramBadge";
import PageHeader from "../../../../components/PageHeader";
import StatusPill, { type StatusTone } from "../../../../components/StatusPill";
import { PlayIcon, PlusIcon, ReloadIcon, StopIcon, TrashIcon } from "../../../../icons";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { ServiceCreation, ServiceSummary, StoppedBy } from "@mixengine/api";
import { movesARow, needsResync } from "../../daemonState";
import { subscribeDaemonWatch } from "../../daemonWatch";
import { serviceStateHint, serviceStateKey, serviceStateTone, toggleMode } from "../../serviceStateLabel";
import { afterRefusal } from "../../forceStep";
import ServiceForm from "../../components/ServiceForm";
import AutostartPanel from "./AutostartPanel";
import DatabasePanel from "./DatabasePanel";
import IdlePanel from "./IdlePanel";
import LimitsPanel from "./LimitsPanel";
import styles from "./ServicesDetail.module.css";

/* The same explicit table as the Dashboard's: a key built by string concatenation is one nobody can grep. */
const PENDING_LABEL = {
  start: "mixengine.dashboard.starting",
  stop: "mixengine.dashboard.stopping",
  restart: "mixengine.dashboard.restarting",
} as const;

/** `mysql@main` → `mysql`; an id with no instance is its own name. */
function serviceName(id: string): string {
  const at = id.indexOf("@");
  return at < 0 ? id : id.slice(0, at);
}

/** `mysql@main` → `@main`, drawn quieter beside the name. */
function serviceInstance(id: string): string {
  const at = id.indexOf("@");
  return at < 0 ? "" : id.slice(at);
}

/** The state as a pill tone: whether it is serving, not which of the seven states it is in. */
function pillTone(state: string | null | undefined, stoppedBy?: StoppedBy | null): StatusTone {
  const tone = serviceStateTone(state, stoppedBy);
  if (tone === "ok") return "success";
  if (tone === "bad") return "danger";
  if (tone === "busy") return "warning";
  return "neutral";
}

/** The same answer for the small dot in a list row. */
function dotTone(
  state: string | null | undefined,
  stoppedBy?: StoppedBy | null,
): "dotSuccess" | "dotDanger" | "dotWarning" | "dotNeutral" {
  const tone = pillTone(state, stoppedBy);
  return tone === "success" ? "dotSuccess" : tone === "danger" ? "dotDanger" : tone === "warning" ? "dotWarning" : "dotNeutral";
}

export default function ServicesDetail({ active }: { active: boolean }) {
  const [services, setServices] = useState<ServiceSummary[]>([]);
  /** False until the first read has answered — until then an empty `services` means "not known
   *  yet", not "no services". */
  const [loaded, setLoaded] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  /** A service just created did not get the port its recipe wanted. See `PortMoved`: true of this
   *  moment and of nothing else. */
  const [moved, setMoved] = useState<ServiceCreation | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null);
  const [forceHint, setForceHint] = useState<string | null>(null);
  const [error, setError] = useState("");
  /** The action in flight on a service, keyed by id — so a click on one service does not lock another. */
  const [busy, setBusy] = useState<Record<string, api.ServiceAction>>({});
  const { t } = useTranslation();

  const reload = useCallback(async () => {
    try {
      const list = await api.services();
      setServices(list.services);
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setLoaded(true);
    }
  }, [t]);

  // Read again on mount and on every return to this screen — installing or removing a runtime (a
  // PHP build, say) raises no event this screen would hear, and the screen stays mounted across a
  // change of screen, so it no longer gets "remount = a fresh read" for free; coming back to the
  // tab is the fallback, see `Dashboard.tsx`.
  useEffect(() => {
    if (active) void reload();
  }, [active, reload]);

  // The dot beside each row is the service's real state, so a change of state reads the list again.
  useEffect(() => {
    return subscribeDaemonWatch((raw) => {
      if (movesARow(raw) || needsResync(raw)) void reload();
    });
  }, [reload]);

  /** The translated state; a state from a daemon newer than this build is shown verbatim. */
  function stateLabel(state: string | null | undefined, stoppedBy?: StoppedBy | null): string {
    const key = serviceStateKey(state, stoppedBy);
    return key === null ? (state ?? "—") : t(key);
  }

  /** The sentence the short label leaves out, for the tooltip. */
  function stateHint(state: string | null | undefined, stoppedBy?: StoppedBy | null): string | undefined {
    const key = serviceStateHint(state, stoppedBy);
    return key === null ? undefined : t(key);
  }

  /* Select the service just created: whoever stood it up is about to set its limits and idle. */
  function created(creation: ServiceCreation) {
    setCreating(false);
    setSelected(creation.service.id);
    setMoved(creation.moved_from == null ? null : creation);
    void reload();
  }

  /** The "it is not on the port you think" sentence, in the three cases `PortMoved` tells apart. */
  function movedNotice(creation: ServiceCreation): string {
    const from = creation.moved_from;
    if (from == null) return "";
    const holder =
      from.program == null
        ? t("mixengine.servicesDetail.movedByUnknown", { preferred: from.preferred })
        : from.pid == null
          ? t("mixengine.servicesDetail.movedByProgram", {
              preferred: from.preferred,
              program: from.program,
            })
          : t("mixengine.servicesDetail.movedBy", {
              preferred: from.preferred,
              program: from.program,
              pid: from.pid,
            });
    return `${t("mixengine.servicesDetail.movedTo", {
      service: creation.service.id,
      port: creation.service.port ?? "?",
    })} ${holder}`;
  }

  /** Send one action, then read again: the stream is best-effort, never the only way to learn state. */
  async function act(id: string, action: api.ServiceAction) {
    setError("");
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
    await reload();
  }

  async function deleteService(id: string, force: boolean) {
    setError("");
    try {
      await api.serviceDelete({ service: id, force });
      setDeleteTarget(null);
      setForceHint(null);
      if (selected === id) setSelected(null);
      void reload();
    } catch (e) {
      // The same rule `runtime.uninstall` follows in Languages.tsx: the first attempt sends no
      // `force`, and the refusal names the sites declaring this service — so the dialog asks again
      // with the daemon's own sentence rather than one of its own.
      const step = afterRefusal(force, errorMessage(t, e));

      if (step.ask === "force") {
        setForceHint(step.hint);
        return;
      }

      // **Closed, because there is nothing left to ask.** A forced attempt can still be refused
      // for a reason force never crosses — a running service — and a dialog left up on that is a
      // dialog on screen with nothing to say.
      setDeleteTarget(null);
      setForceHint(null);
      setError(step.error);
    }
  }

  const current = services.find((service) => service.id === selected);
  const pending = selected === null ? undefined : busy[selected];
  const mode = toggleMode(current?.state, pending !== undefined);

  return (
    <div className={styles.screen}>
      <div className={styles.list}>
        <div className={styles.listHead}>
          <h2 className={styles.listTitle}>{t("mixengine.sidebar.servicesDetail")}</h2>
          {loaded && <span className={styles.count}>{services.length}</span>}
        </div>
        <Button className={styles.newService} onClick={() => setCreating(true)}>
          <PlusIcon size={14} />
          {t("mixengine.serviceForm.newService")}
        </Button>
        <div className={styles.rows}>
          {services.map((service) => (
            // A plain `<button>` per row, as the module sidebar beside it: `ItemList` draws a
            // single line of text and has no room for the badge and the state under the name.
            <button
              key={service.id}
              type="button"
              className={styles.row}
              aria-current={service.id === selected ? "true" : undefined}
              onClick={() => setSelected(service.id)}
            >
              <MonogramBadge name={service.id} size={30} />
              <span className={styles.rowText}>
                <span className={styles.rowName}>
                  {serviceName(service.id)}
                  <span className={styles.instance}>{serviceInstance(service.id)}</span>
                  {service.version != null && <span className={styles.version}>{service.version}</span>}
                </span>
                <span className={styles.rowState} title={stateHint(service.state, service.stopped_by)}>
                  <span className={`${styles.dot} ${styles[dotTone(service.state, service.stopped_by)]}`} aria-hidden="true" />
                  {stateLabel(service.state, service.stopped_by)}
                </span>
              </span>
            </button>
          ))}
          {!loaded && <LoadingState compact />}
          {loaded && services.length === 0 && <p className={styles.listEmpty}>{t("mixengine.servicesDetail.noServices")}</p>}
        </div>
      </div>

      <div className={styles.detail}>
        {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}
        {selected === null ? (
          <EmptyState title={t("mixengine.servicesDetail.pickService")} />
        ) : (
          <>
            <PageHeader
              leading={<MonogramBadge name={selected} size={50} />}
              title={selected}
              badges={
                // State, role and port on the title's own line: one glance reads the whole service.
                current !== undefined && (
                  <>
                    <StatusPill
                      tone={pillTone(current.state, current.stopped_by)}
                      pulse={mode === "moving"}
                      title={pending !== undefined ? undefined : stateHint(current.state, current.stopped_by)}
                    >
                      {pending !== undefined ? t(PENDING_LABEL[pending]) : stateLabel(current.state, current.stopped_by)}
                    </StatusPill>
                    {current.role?.role === "front_end" && (
                      <span className={styles.tag}>{t("mixengine.servicesDetail.frontEnd")}</span>
                    )}
                    {current.version != null && <span className={styles.port}>{current.version}</span>}
                    {current.port != null && (
                      <span className={styles.port}>{t("mixengine.servicesDetail.port", { port: current.port })}</span>
                    )}
                  </>
                )
              }
              actions={
                <>
                  <Button
                    variant="positive"
                    busy={pending === "start" ? t(PENDING_LABEL.start) : undefined}
                    disabled={current === undefined || mode !== "down"}
                    onClick={() => void act(selected, "start")}
                  >
                    <PlayIcon size={13} />
                    {t("mixengine.dashboard.start")}
                  </Button>
                  <Button
                    className={styles.stop}
                    busy={pending === "stop" ? t(PENDING_LABEL.stop) : undefined}
                    disabled={current === undefined || mode !== "up"}
                    onClick={() => void act(selected, "stop")}
                  >
                    <StopIcon size={13} className={styles.stopMark} />
                    {t("mixengine.dashboard.stop")}
                  </Button>
                  <Button
                    busy={pending === "restart" ? t(PENDING_LABEL.restart) : undefined}
                    disabled={current === undefined || mode !== "up"}
                    onClick={() => void act(selected, "restart")}
                  >
                    <ReloadIcon size={14} />
                    {t("mixengine.dashboard.restart")}
                  </Button>
                  <Button variant="danger" onClick={() => setDeleteTarget(selected)}>
                    <TrashIcon size={14} />
                    {t("mixengine.servicesDetail.delete")}
                  </Button>
                </>
              }
            />
            {moved !== null && moved.service.id === selected && (
              <p className={styles.notice} role="status">
                {movedNotice(moved)}
              </p>
            )}
            {/* The panels are keyed by service: a panel is about one service, so another one gets a
                fresh set — nothing typed, read or failed for the last one is left to show under
                this one's name while its own read is out. **One key, on the group.** The same key
                on each of four siblings is a duplicate key, and React then fails to remove the old
                panels on a change of service: they pile up, one set per service visited. */}
            <Fragment key={selected}>
              {/* First, and it **draws nothing at all** for a service that is not a database — so
                  for nginx or a php-fpm pool, the first thing to read is still Autostart. It sits
                  on top because it is specific to this service, while the three panels below ask
                  the same question of every service. */}
              <DatabasePanel service={selected} />
              {/* Side by side and in this order: autostart answers "what is running when I sit
                  down", idle answers "what keeps running when I am not using it". Two different
                  questions about one service, and anyone turning both on has to see both at once. */}
              <AutostartPanel service={selected} />
              <IdlePanel service={selected} />
              <LimitsPanel service={selected} />
            </Fragment>
          </>
        )}
      </div>
      {creating && <ServiceForm onCancel={() => setCreating(false)} onCreated={created} />}

      {deleteTarget !== null && (
        <ConfirmDialog
          title={t("mixengine.servicesDetail.deleteTitle", { service: deleteTarget })}
          message={forceHint ?? t("mixengine.servicesDetail.deleteMessage")}
          confirmLabel={
            forceHint !== null ? t("mixengine.servicesDetail.deleteForceConfirm") : undefined
          }
          danger
          onCancel={() => {
            setDeleteTarget(null);
            setForceHint(null);
          }}
          onConfirm={() => void deleteService(deleteTarget, forceHint !== null)}
        />
      )}
    </div>
  );
}
