import { useState } from "react";
import type { DaemonStatus, MetricsFrame } from "@mixengine/api";
import Button from "../../../../components/Button";
import PageHeader from "../../../../components/PageHeader";
import StatusPill from "../../../../components/StatusPill";
import { copyText } from "../../../../core/clipboard";
import { useTranslation } from "../../../../i18n";
import { CopyIcon, FolderIcon, PlusIcon, ReloadIcon, StopIcon } from "../../../../icons";
import type * as api from "../../api";
import DaemonUsage from "../../components/DaemonUsage";
import type { ServiceRow } from "../../daemonState";
import { DAEMON_SUBJECT, readingFor } from "../../metricsState";
import { toggleMode } from "../../serviceStateLabel";
import styles from "./Dashboard.module.css";

interface Props {
  status: DaemonStatus | null;
  rows: ServiceRow[];
  busy: Record<string, api.ServiceAction>;
  frame: MetricsFrame | null;
  runningCount: number;
  waiting: number;
  /** The operations waiting for rights, while their dialog is open. */
  pending: unknown[] | null;
  showWaiting: () => Promise<void>;
  reload: () => Promise<void>;
  stopAll: () => Promise<void>;
  onNew: () => void;
}

/** The Dashboard's header: the daemon's version and the services' summary, its home and its own
 *  usage, and the buttons that act on every service at once. */
export default function DashboardHeader({
  status,
  rows,
  busy,
  frame,
  runningCount,
  waiting,
  pending,
  showWaiting,
  reload,
  stopAll,
  onNew,
}: Props) {
  const { t } = useTranslation();
  /** The home path was just copied, for the button to say so for a moment. */
  const [homeCopied, setHomeCopied] = useState(false);
  const movingCount = rows.filter(
    (row) => busy[row.id] !== undefined || toggleMode(row.state, false) === "moving",
  ).length;
  const daemon = readingFor(frame, DAEMON_SUBJECT);

  function copyHome(home: string) {
    void copyText(home).then(() => {
      setHomeCopied(true);
      window.setTimeout(() => setHomeCopied(false), 1600);
    });
  }

  return (
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
          <Button size="large" variant="primary" onClick={() => onNew()}>
            <PlusIcon size={15} />
            {t("mixengine.dashboard.newService")}
          </Button>
        </div>
      }
    />
  );
}
