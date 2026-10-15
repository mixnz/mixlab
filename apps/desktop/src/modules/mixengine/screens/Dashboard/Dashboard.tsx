import { useState } from "react";

import ConfirmDialog from "../../../../components/ConfirmDialog";
import ErrorBanner from "../../../../components/ErrorBanner";
import { useTranslation } from "../../../../i18n";
import CredentialDialog from "../../components/CredentialDialog";
import ElevationDialog from "../../components/ElevationDialog";
import ServiceForm from "../../components/ServiceForm";
import { TERMINAL_MODULE_ID } from "../../nextSteps";
import { shouldOfferQuickStart } from "../../quickStart";
import CleanupDialog from "./CleanupDialog";
import DashboardHeader from "./DashboardHeader";
import DiskUsagePanel from "./DiskUsagePanel";
import FoundServices from "./FoundServices";
import PathNudge from "./PathNudge";
import QuickStart from "./QuickStart";
import RestorePrevious from "./RestorePrevious";
import ServiceRowMenu from "./ServiceRowMenu";
import ServicesCard from "./ServicesCard";
import styles from "./Dashboard.module.css";
import { isServing } from "./serviceView";
import { useDatabaseClients } from "./useDatabaseClients";
import { useDiskUsage } from "./useDiskUsage";
import { useMetrics } from "./useMetrics";
import { useServices } from "./useServices";
import { useSites } from "./useSites";

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
  const [cleaning, setCleaning] = useState(false);
  const [error, setError] = useState("");
  const [creating, setCreating] = useState(false);
  /** A row's menu, and where it was opened. `null` means no menu is open. */
  const [menu, setMenu] = useState<{ id: string; x: number; y: number } | null>(null);
  const { t } = useTranslation();

  const { disk, refreshingDisk, readDisk, refreshDisk } = useDiskUsage({ setError });
  const {
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
  } = useServices({ active, readDisk, setError });

  const { sites, readSites } = useSites(active);

  const frame = useMetrics(active);

  const { databases, credentials, setCredentials, showCredentials, resetTarget, setResetTarget, resetCredential } =
    useDatabaseClients({ rows, reload, setError });

  const runningCount = rows.filter((row) => isServing(row.state)).length;

  return (
    <div className={`mixengine-page ${styles.dashboard}`}>
      {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}

      <DashboardHeader
        status={status}
        rows={rows}
        busy={busy}
        frame={frame}
        runningCount={runningCount}
        waiting={waiting}
        pending={pending}
        showWaiting={showWaiting}
        reload={reload}
        stopAll={stopAll}
        onNew={() => setCreating(true)}
      />

      {/* Above the service table, and only when this home has no site yet — T117. */}
      {shouldOfferQuickStart(sites) && (
        <QuickStart
          onCreated={() => void readSites()}
          terminalVisible={isModuleVisible(TERMINAL_MODULE_ID)}
        />
      )}
      <PathNudge active={active} />
      {/* T182h: a copy of an earlier install's state, while this home has nothing of its own. */}
      <RestorePrevious active={active} onRestored={() => void reload()} />
      {/* T182g: service data an earlier install left, until nothing is left to adopt. */}
      <FoundServices active={active} onAdopted={() => void reload()} />

      <ServicesCard
        loaded={loaded}
        rows={rows}
        runningCount={runningCount}
        jobs={jobs}
        cancelling={cancelling}
        busy={busy}
        frame={frame}
        cancelJob={cancelJob}
        act={act}
        setAutostart={setAutostart}
        onOpenMenu={setMenu}
      />

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

      <ServiceRowMenu
        menu={menu}
        rows={rows}
        databases={databases}
        isModuleVisible={isModuleVisible}
        onClose={() => setMenu(null)}
        onViewLogs={onViewLogs}
        setAutostart={setAutostart}
        showCredentials={showCredentials}
        setResetTarget={setResetTarget}
        setError={setError}
      />

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

