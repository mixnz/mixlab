import { useCallback, useEffect, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import Button from "../../../../components/Button";
import Card from "../../../../components/Card";
import ConfirmDialog from "../../../../components/ConfirmDialog";
import EmptyState from "../../../../components/EmptyState";
import ErrorBanner from "../../../../components/ErrorBanner";
import LoadingState from "../../../../components/LoadingState";
import MonogramBadge from "../../../../components/MonogramBadge";
import PageHeader from "../../../../components/PageHeader";
import StatusPill, { type StatusTone } from "../../../../components/StatusPill";
import Table from "../../../../components/Table";
import { FolderIcon } from "../../../../icons";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { ExtensionOffer } from "@mixengine/api";
import type { ExtensionOrigin } from "@mixengine/api";
import type { ExtensionSummary } from "@mixengine/api";
import StaleBadge from "../../components/StaleBadge";
import Checkbox from "../../../../components/Checkbox";
import { serviceStateKey, serviceStateTone } from "../../serviceStateLabel";
import PlanDialog from "./PlanDialog";
import styles from "./Extensions.module.css";

/** The state as a pill tone: whether it is serving, not which of the seven states it is in. */
function pillTone(state: string | null | undefined): StatusTone {
  const tone = serviceStateTone(state);
  if (tone === "ok") return "success";
  if (tone === "bad") return "danger";
  if (tone === "busy") return "warning";
  return "neutral";
}

/**
 * The registry, what is installed, installing (from the registry or from a local directory),
 * removing, and starting or stopping an extension of `kind: "service"`.
 *
 * There is no "configure" screen — `extension.configure` does not exist (decision D1, spec).
 */
export default function Extensions({ active }: { active: boolean }) {
  const [installed, setInstalled] = useState<ExtensionSummary[]>([]);
  const [available, setAvailable] = useState<ExtensionOffer[]>([]);
  /** False until the first read has answered — until then both lists being empty means "not known
   *  yet", not "nothing installed" or "an empty registry". */
  const [loaded, setLoaded] = useState(false);
  const [unreadable, setUnreadable] = useState(0);
  const [stale, setStale] = useState(false);
  const [serviceState, setServiceState] = useState<Record<string, string | null | undefined>>({});
  const [error, setError] = useState("");
  const [installingSource, setInstallingSource] = useState<ExtensionOrigin | null>(null);
  const [uninstalling, setUninstalling] = useState<ExtensionSummary | null>(null);
  const [deleteData, setDeleteData] = useState(false);
  const { t } = useTranslation();

  const reload = useCallback(async () => {
    try {
      const [inst, avail, services] = await Promise.all([
        api.extensionsInstalled(),
        api.extensionsAvailable(),
        api.services(),
      ]);
      setInstalled(inst.extensions);
      setAvailable(avail.extensions);
      setUnreadable(avail.unreadable);
      setStale(avail.stale);
      const states: Record<string, string | null | undefined> = {};
      for (const svc of services.services) states[svc.id] = svc.state;
      setServiceState(states);
      setError("");
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setLoaded(true);
    }
  }, [t]);

  // Read again on mount and on every return to this screen — the same reason `Dashboard.tsx` has.
  useEffect(() => {
    if (active) void reload();
  }, [active, reload]);

  async function browseInstallFromPath() {
    const picked = await openDialog({ directory: true, multiple: false });
    if (typeof picked === "string") setInstallingSource({ type: "path", path: picked });
  }

  /** `extension.*` and not `service.*` — see this plan's Global Constraints. */
  async function toggle(row: ExtensionSummary, action: "start" | "stop") {
    setError("");
    try {
      if (action === "start") await api.extensionStart(row.id);
      else await api.extensionStop(row.id);
      void reload();
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }

  async function confirmUninstall() {
    if (!uninstalling) return;
    setError("");
    try {
      await api.extensionUninstall({ id: uninstalling.id, delete_data: deleteData });
      void reload();
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      // **Closed either way.** This screen has no second question to ask on a refusal, and a
      // dialog kept up past its own answer is one nobody can see or dismiss — it has already
      // animated out. The banner behind it is where the refusal is read.
      setUninstalling(null);
      setDeleteData(false);
    }
  }


  /** The translated state; one nobody knows is shown as the daemon wrote it. See
   *  `serviceStateLabel.ts`. */
  function stateLabel(state: string | null | undefined): string {
    const key = serviceStateKey(state);
    return key === null ? (state ?? "—") : t(key);
  }

  return (
    <div className={`mixengine-page ${styles.extensions}`}>
      {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}

      <PageHeader
        title={t("mixengine.sidebar.extensions")}
        description={t("mixengine.extensions.about")}
        actions={
          <Button size="large" onClick={() => void browseInstallFromPath()}>
            <FolderIcon size={15} />
            {t("mixengine.extensions.installFromPath")}
          </Button>
        }
      />

      <Card title={t("mixengine.extensions.installedTitle")} count={loaded ? installed.length : undefined} flush>
        <Table aria-label={t("mixengine.extensions.installedTitle")}>
          <thead>
            <tr>
              <th>{t("mixengine.extensions.columnName")}</th>
              <th>{t("mixengine.extensions.columnVersion")}</th>
              <th>{t("mixengine.extensions.columnKind")}</th>
              <th>{t("mixengine.extensions.columnState")}</th>
              <th data-align="end" />
            </tr>
          </thead>
          <tbody>
            {installed.map((row) => (
              <tr key={row.id}>
                <td data-nowrap>
                  <span className={styles.name}>
                    <MonogramBadge name={row.name} size={28} />
                    {row.name}
                  </span>
                </td>
                <td className={styles.version}>{row.version}</td>
                <td>
                  <span className={styles.tag}>{row.kind}</span>
                </td>
                <td>
                  {row.kind === "service" ? (
                    <StatusPill tone={pillTone(serviceState[row.id])}>{stateLabel(serviceState[row.id])}</StatusPill>
                  ) : (
                    <span className={styles.none}>—</span>
                  )}
                </td>
                <td data-align="end" data-nowrap>
                  <span className={styles.rowActions}>
                    {row.kind === "service" && (
                      <>
                        <Button size="small" variant="positive" onClick={() => void toggle(row, "start")}>
                          {t("mixengine.extensions.start")}
                        </Button>
                        <Button size="small" onClick={() => void toggle(row, "stop")}>
                          {t("mixengine.extensions.stop")}
                        </Button>
                      </>
                    )}
                    <Button size="small" variant="danger" onClick={() => setUninstalling(row)}>
                      {t("mixengine.extensions.uninstall")}
                    </Button>
                  </span>
                </td>
              </tr>
            ))}
          </tbody>
        </Table>
        {!loaded && <LoadingState />}
      </Card>

      <Card
        title={t("mixengine.extensions.registryTitle")}
        count={
          loaded ? (
            <>
              {available.length}
              <StaleBadge stale={stale} />
            </>
          ) : undefined
        }
        description={unreadable > 0 ? t("mixengine.extensions.unreadable", { count: unreadable }) : undefined}
        flush
      >
        {!loaded ? (
          <LoadingState />
        ) : available.length === 0 ? (
          <EmptyState title={t("mixengine.extensions.registryEmpty")} />
        ) : (
          <Table aria-label={t("mixengine.extensions.registryTitle")}>
            <tbody>
              {available.map((offer) => (
                <tr key={offer.id}>
                  <td data-nowrap>
                    <span className={styles.name}>
                      <MonogramBadge name={offer.name} size={28} />
                      {offer.name}
                    </span>
                  </td>
                  <td className={styles.version}>{offer.version}</td>
                  <td>
                    <span className={styles.tag}>{offer.kind}</span>
                  </td>
                  <td data-align="end" data-nowrap>
                    {offer.installed ? (
                      <StatusPill tone="success">{t("mixengine.extensions.installed")}</StatusPill>
                    ) : (
                      <Button
                        size="small"
                        variant="soft"
                        onClick={() => setInstallingSource({ type: "registry", id: offer.id })}
                      >
                        {t("mixengine.extensions.install")}
                      </Button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </Table>
        )}
      </Card>
      {installingSource && (
        <PlanDialog
          source={installingSource}
          onCancel={() => setInstallingSource(null)}
          onInstalled={() => {
            setInstallingSource(null);
            void reload();
          }}
        />
      )}

      {uninstalling && (
        <ConfirmDialog
          title={t("mixengine.extensions.uninstallTitle", { name: uninstalling.name })}
          message={t("mixengine.extensions.uninstallMessage")}
          danger
          onCancel={() => {
            setUninstalling(null);
            setDeleteData(false);
          }}
          onConfirm={() => void confirmUninstall()}
        >
          <Checkbox
            className={styles.checkbox}
            label={t("mixengine.extensions.deleteData")}
            checked={deleteData}
            onChange={(e) => setDeleteData(e.target.checked)}
          />
        </ConfirmDialog>
      )}
    </div>
  );
}
