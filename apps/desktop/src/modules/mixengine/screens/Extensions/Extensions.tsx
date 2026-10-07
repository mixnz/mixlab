import { useCallback, useEffect, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";

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
import type { SiteSummary } from "@mixengine/api";
import StaleBadge from "../../components/StaleBadge";
import Checkbox from "../../../../components/Checkbox";
import { serviceStateKey, serviceStateTone } from "../../serviceStateLabel";
import { applyJob, type JobRow } from "../../daemonState";
import { subscribeDaemonWatch } from "../../daemonWatch";
import { jobFinished, jobFor } from "../../runtimeState";
import { jobFailureMessage } from "../../blueprintPlan";
import { siteVisit } from "../../siteState";
import {
  installable,
  kindKey,
  notInstalled,
  publishedTargets,
  rowActions,
  summaryDescription,
  webAppSite,
  type RowAction,
} from "../../extensionState";
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
 * removing, and running what each installed extension runs.
 *
 * **One add-on, one row** (T200, D4): what is installed is listed under *Installed* and nowhere
 * else. **A `web-app` is its site** (D6): it is opened, turned on and off through `site.*`, because
 * `extension.start` refuses it by design (T81b, D10).
 *
 * There is no "configure" screen — `extension.configure` does not exist (decision D1, spec).
 */
export default function Extensions({ active }: { active: boolean }) {
  const [installed, setInstalled] = useState<ExtensionSummary[]>([]);
  const [available, setAvailable] = useState<ExtensionOffer[]>([]);
  const [sites, setSites] = useState<SiteSummary[]>([]);
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
  const [jobs, setJobs] = useState<JobRow[]>([]);
  /** The install job each add-on is following, by extension id — T200, D3. */
  const [installingJob, setInstallingJob] = useState<Record<string, number>>({});
  const { t } = useTranslation();

  // Read the latest `installingJob` inside the watch registered once, for `usePackages`' reason.
  const installingJobRef = useRef(installingJob);
  useEffect(() => {
    installingJobRef.current = installingJob;
  }, [installingJob]);

  // `stillShow` is the sentence that has to survive this re-read — `usePackages`' rule: a failed
  // install job's reason must not be wiped by the read that follows it.
  const reload = useCallback(
    async (stillShow = "") => {
      try {
        const [inst, avail, services, listed] = await Promise.all([
          api.extensionsInstalled(),
          api.extensionsAvailable(),
          api.services(),
          api.sites(),
        ]);
        setInstalled(inst.extensions);
        setAvailable(avail.extensions);
        setUnreadable(avail.unreadable);
        setStale(avail.stale);
        setSites(listed.sites);
        const states: Record<string, string | null | undefined> = {};
        for (const svc of services.services) states[svc.id] = svc.state;
        setServiceState(states);
        setError(stillShow);
      } catch (e) {
        setError(errorMessage(t, e));
      } finally {
        setLoaded(true);
      }
    },
    [t],
  );

  // Read again on mount and on every return to this screen — the same reason `Dashboard.tsx` has.
  useEffect(() => {
    if (active) void reload();
  }, [active, reload]);

  /** Stop following one job and read the lists again, keeping `failure` on screen. */
  const settle = useCallback(
    (job: number, failure: string) => {
      setInstallingJob((current) => {
        const next = { ...current };
        for (const key of Object.keys(next)) if (next[key] === job) delete next[key];
        return next;
      });
      void reload(failure);
    },
    [reload],
  );

  useEffect(() => {
    return subscribeDaemonWatch((raw) => {
      setJobs((current) => applyJob(current, raw));
      const finished = jobFinished(raw);
      if (finished !== null && Object.values(installingJobRef.current).includes(finished.id)) {
        // For a failed install, `job_finished` is the only place that says why.
        settle(finished.id, finished.error === null ? "" : errorMessage(t, finished.error));
      }
    });
  }, [settle, t]);

  /** The plan was accepted and the job is running. **Asked once more, at once** — a path install
   *  can end before this id is stored, and its `job_finished` would then pass unrecognised and
   *  leave the row installing for ever (T200, Review Focus 1). */
  async function started(id: string, job: number) {
    setInstallingSource(null);
    setInstallingJob((current) => ({ ...current, [id]: job }));
    try {
      const status = await api.jobStatus(job);
      if (status.state !== "running") settle(job, jobFailureMessage(status) ?? "");
    } catch {
      // The watch is still listening; an answer it delivers settles the row the same way.
    }
  }

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

  /** The Sites screen's *Open*: scheme and port are decided where they already are. */
  async function visitSite(site: SiteSummary) {
    try {
      await openUrl(siteVisit(site).url);
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }

  /** A service's own page — T200a, D3. Started first when it is not running, the way Sites' Open
   *  starts a project's services; a start that fails opens nothing and says why. */
  async function openPage(row: ExtensionSummary) {
    if (!row.ui) return;
    setError("");
    try {
      if (!["running", "starting", "degraded"].includes(serviceState[row.id] ?? "")) {
        await api.extensionStart(row.id);
        void reload();
      }
      await openUrl(row.ui);
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }

  /** A `web-app` is turned on and off as its site — T200, D6. */
  async function switchSite(site: SiteSummary, on: boolean) {
    setError("");
    try {
      if (on) await api.siteStart(site.domain);
      else await api.siteStop(site.domain);
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

  /** The kind in words; one this build does not know as the daemon wrote it. */
  function kindLabel(kind: string): string {
    const key = kindKey(kind);
    return key === null ? kind : t(key);
  }

  function nameCell(name: string, description: string | null) {
    return (
      <span className={styles.name}>
        <MonogramBadge name={name} size={34} />
        <span className={styles.nameText}>
          {name}
          {description !== null && (
            <span className={styles.description} title={description}>
              {description}
            </span>
          )}
        </span>
      </span>
    );
  }

  function stateCell(row: ExtensionSummary, site: SiteSummary | null) {
    if (row.kind === "web-app") {
      if (site === null) {
        return <StatusPill tone="danger">{t("mixengine.extensions.siteMissing")}</StatusPill>;
      }
      return site.state === "enabled" ? (
        <StatusPill tone="success">{t("mixengine.extensions.siteOn")}</StatusPill>
      ) : (
        <StatusPill tone="neutral">{t("mixengine.extensions.siteOff")}</StatusPill>
      );
    }
    if (row.kind === "service") {
      return (
        <StatusPill tone={pillTone(serviceState[row.id])}>{stateLabel(serviceState[row.id])}</StatusPill>
      );
    }
    return <span className={styles.none}>—</span>;
  }

  function actionButton(row: ExtensionSummary, site: SiteSummary | null, action: RowAction) {
    switch (action) {
      case "open":
        return (
          <Button
            key={action}
            size="small"
            variant="soft"
            onClick={() => void (site ? visitSite(site) : openPage(row))}
          >
            {t("mixengine.extensions.open")}
          </Button>
        );
      case "turnOn":
        return (
          site && (
            <Button key={action} size="small" variant="positive" onClick={() => void switchSite(site, true)}>
              {t("mixengine.extensions.turnOn")}
            </Button>
          )
        );
      case "turnOff":
        return (
          site && (
            <Button key={action} size="small" onClick={() => void switchSite(site, false)}>
              {t("mixengine.extensions.turnOff")}
            </Button>
          )
        );
      case "start":
        return (
          <Button key={action} size="small" variant="positive" onClick={() => void toggle(row, "start")}>
            {t("mixengine.extensions.start")}
          </Button>
        );
      case "stop":
        return (
          <Button key={action} size="small" onClick={() => void toggle(row, "stop")}>
            {t("mixengine.extensions.stop")}
          </Button>
        );
    }
  }

  /** What an offer's last cell holds: its install's progress, why it cannot be installed here, or
   *  the button — T200, D3 and D8. */
  function offerCell(offer: ExtensionOffer) {
    const job = jobFor(jobs, installingJob[offer.id]);
    if (job) {
      return (
        <span className={styles.progress}>
          <progress value={job.percent} max={100} />
          <span className={styles.progressText}>{job.message}</span>
        </span>
      );
    }
    if (installingJob[offer.id] !== undefined) {
      // Accepted, and no progress reported yet.
      return (
        <span className={styles.progress}>
          <progress />
        </span>
      );
    }
    if (!installable(offer.artifact)) {
      return (
        <span
          className={styles.unavailable}
          title={t("mixengine.extensions.publishedFor", {
            targets: publishedTargets(offer.artifact).join(", "),
          })}
        >
          {t("mixengine.extensions.notForThisSystem")}
          <Button size="small" variant="soft" disabled>
            {t("mixengine.extensions.install")}
          </Button>
        </span>
      );
    }
    return (
      <Button
        size="small"
        variant="soft"
        onClick={() => setInstallingSource({ type: "registry", id: offer.id })}
      >
        {t("mixengine.extensions.install")}
      </Button>
    );
  }

  const offers = notInstalled(available);

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
        {!loaded ? (
          <LoadingState />
        ) : installed.length === 0 ? (
          <EmptyState title={t("mixengine.extensions.installedEmpty")} />
        ) : (
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
              {installed.map((row) => {
                const site = row.kind === "web-app" ? webAppSite(row, sites) : null;
                return (
                  <tr key={row.id}>
                    <td data-nowrap>{nameCell(row.name, summaryDescription(row))}</td>
                    <td className={styles.version}>{row.version}</td>
                    <td>
                      <span className={styles.tag}>{kindLabel(row.kind)}</span>
                    </td>
                    <td>{stateCell(row, site)}</td>
                    <td data-align="end" data-nowrap>
                      <span className={styles.rowActions}>
                        {rowActions(row, serviceState[row.id], site).map((action) =>
                          actionButton(row, site, action),
                        )}
                        <Button size="small" variant="danger" onClick={() => setUninstalling(row)}>
                          {t("mixengine.extensions.uninstall")}
                        </Button>
                      </span>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </Table>
        )}
      </Card>

      <Card
        title={t("mixengine.extensions.availableTitle")}
        count={
          loaded ? (
            <>
              {offers.length}
              <StaleBadge stale={stale} />
            </>
          ) : undefined
        }
        description={unreadable > 0 ? t("mixengine.extensions.unreadable", { count: unreadable }) : undefined}
        flush
      >
        {!loaded ? (
          <LoadingState />
        ) : offers.length === 0 ? (
          <EmptyState title={t("mixengine.extensions.registryEmpty")} />
        ) : (
          <Table aria-label={t("mixengine.extensions.availableTitle")}>
            <tbody>
              {offers.map((offer) => (
                <tr key={offer.id}>
                  <td data-nowrap>
                    {nameCell(offer.name, offer.description.trim() === "" ? null : offer.description)}
                  </td>
                  <td className={styles.version}>{offer.version}</td>
                  <td>
                    <span className={styles.tag}>{kindLabel(offer.kind)}</span>
                  </td>
                  <td data-align="end" data-nowrap>
                    {offerCell(offer)}
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
          onStarted={(id, job) => void started(id, job)}
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
