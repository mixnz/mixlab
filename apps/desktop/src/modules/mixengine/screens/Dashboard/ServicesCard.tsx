import { useState } from "react";
import type { MetricsFrame } from "@mixengine/api";
import ActionBar from "../../../../components/ActionBar";
import Button from "../../../../components/Button";
import Card from "../../../../components/Card";
import EmptyState from "../../../../components/EmptyState";
import LoadingState from "../../../../components/LoadingState";
import MonogramBadge from "../../../../components/MonogramBadge";
import SegmentedControl from "../../../../components/SegmentedControl";
import StatusPill from "../../../../components/StatusPill";
import Switch from "../../../../components/Switch";
import Table from "../../../../components/Table";
import { useTranslation } from "../../../../i18n";
import { MoreIcon, PlayIcon, ReloadIcon, StopIcon } from "../../../../icons";
import type * as api from "../../api";
import CpuRing from "../../components/CpuRing";
import { useCpuScale } from "../../cpuScale";
import type { JobRow, ServiceRow } from "../../daemonState";
import { cpuShare, formatBytes, formatCpu, metricsSubjectFor, readingFor } from "../../metricsState";
import { serviceBadge } from "../../serviceBadge";
import { toggleMode } from "../../serviceStateLabel";
import styles from "./Dashboard.module.css";
import { PENDING_LABEL, isServing, pillTone, stateHint, stateLabel, type ServiceFilter } from "./serviceView";

interface Props {
  loaded: boolean;
  rows: ServiceRow[];
  runningCount: number;
  jobs: JobRow[];
  cancelling: ReadonlySet<number>;
  busy: Record<string, api.ServiceAction>;
  frame: MetricsFrame | null;
  cancelJob: (id: number) => Promise<void>;
  act: (id: string, action: api.ServiceAction) => Promise<void>;
  setAutostart: (id: string, autostart: boolean) => Promise<void>;
  /** Opens a row's menu where its ⋮ button is. */
  onOpenMenu: (menu: { id: string; x: number; y: number }) => void;
}

/** The services card: its filter, the jobs in progress, and a row per service with its state,
 *  autostart, port, usage and actions. */
export default function ServicesCard({
  loaded,
  rows,
  runningCount,
  jobs,
  cancelling,
  busy,
  frame,
  cancelJob,
  act,
  setAutostart,
  onOpenMenu,
}: Props) {
  const { t } = useTranslation();
  const cpuScale = useCpuScale();
  /** Which rows the Services card shows. */
  const [filter, setFilter] = useState<ServiceFilter>("all");
  const shown = rows.filter((row) =>
    filter === "all" ? true : filter === "running" ? isServing(row.state) : !isServing(row.state),
  );
  const rowIds = rows.map((row) => row.id);

  return (
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
              {/* Next to State: what is running, and what will run after the next login. */}
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
                        title={stateHint(t, row.state, row.stoppedBy, row.lastFailure)}
                      >
                        {stateLabel(t, row.state, row.stoppedBy)}
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
                          busy={busy[row.id] ? t(PENDING_LABEL[busy[row.id]]) : stateLabel(t, row.state, row.stoppedBy)}
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
                              onOpenMenu({ id: row.id, x: at.left, y: at.bottom });
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
  );
}
