import { useCallback, useEffect, useRef, useState } from "react";

import Button from "../../../../components/Button";
import ConfirmDialog from "../../../../components/ConfirmDialog";
import Select from "../../../../components/Select";
import type { AppError } from "../../../../core/errors";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { Error as WireError } from "@mixengine/api";
import type { FrontEndReport } from "@mixengine/api";
import type { FrontEndServer } from "@mixengine/api";
import type { JobSummary } from "@mixengine/api";
import type { ServiceSummary } from "@mixengine/api";
import ElevationDialog from "../../components/ElevationDialog";
import { isJobFinished, needsResync } from "../../daemonState";
import { subscribeDaemonWatch } from "../../daemonWatch";
import styles from "./Settings.module.css";
import { useRunningDots } from "./useRunningDots";

/** The two `FrontEndServer` values — exactly the contract's closed list, not read from any
 *  package. */
const SERVERS: FrontEndServer[] = ["caddy", "nginx"];

/** `JobOutcome.error` as a translatable `AppError`. */
function refusal(error: WireError): AppError {
  const params: Record<string, string> = { code: error.code, message: error.message };
  if (error.hint) params.hint = error.hint;
  return { code: "error.mixengineRefused", params };
}

/** The active server, or `null` when no row is the front end. `undefined` when this daemon was
 *  built before `role` existed (ADR 0019: an absent member means "old daemon", not "unknown"). */
function activeFrontEnd(services: ServiceSummary[]): FrontEndServer | null | undefined {
  if (services.every((service) => service.role === undefined || service.role === null)) {
    return undefined;
  }
  for (const service of services) {
    if (service.role?.role === "front_end") return service.role.server;
  }
  return null;
}

/**
 * "Default web server" — `service.set_front_end`, T97 / ADR 0026.
 *
 * **Read from `ServiceSummary.role`; there is no separate read method.** The row with
 * `role: front_end` also carries `server`, exactly the value `FrontEndSwitch.server` takes — MixDB
 * does not map package names to meanings (ADR 0026: "no client may map a package name to a role").
 * The choice list is the two closed values of `FrontEndServer`; for a server not yet installed the
 * daemon refuses with the install command in `hint`, shown through `errorMessage` like every other
 * error, with no package check done here.
 *
 * **Writing is a job**, not a setting: the daemon stops the old server, brings up the new one, and
 * may need a prompt (the port 80/443 grant on Linux follows the binary). Poll `jobStatus` like any
 * other job, then read the `FrontEndReport` in `result` — the five `outcome`s are drawn as five
 * different outcomes, not lumped together as "error":
 *
 * - `switched`/`unchanged` → reread; show `not_carried` (overrides that could not be carried over
 *   — this field exists so they are not silently swallowed) and `kept_data`.
 * - `not_granted` → the same T64 flow as everywhere in the app: read `elevation.status`, open
 *   `ElevationDialog`. When the dialog closes, read the queue again: empty (granted) means calling
 *   switch again by itself — the daemon says "allowing it and asking again works"; something still
 *   waiting (the user only closed it) stops at one sentence, with no endless loop.
 * - `rolled_back`/`failed` → `because` in red, and the choice returns to the active server.
 *
 * No `version` is passed: the daemon takes the newest installed version, exactly as the contract
 * notes ("nobody choosing a web server is choosing a patch release").
 */
export default function FrontEndSection({
  active,
  onError,
}: {
  active: boolean;
  onError: (message: string) => void;
}) {
  const [services, setServices] = useState<ServiceSummary[] | null>(null);
  const [choice, setChoice] = useState<FrontEndServer | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [job, setJob] = useState<JobSummary | null>(null);
  const [report, setReport] = useState<FrontEndReport | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [pending, setPending] = useState<unknown[] | null>(null);
  const [canPrompt, setCanPrompt] = useState(true);
  const [reason, setReason] = useState<string | null | undefined>(null);
  const dots = useRunningDots(job !== null);
  const { t } = useTranslation();
  const live = useRef(true);
  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
    };
  }, []);

  const current = services === null ? undefined : activeFrontEnd(services);

  const reload = useCallback(async () => {
    try {
      const list = await api.services();
      setServices(list.services);
    } catch (e) {
      onError(errorMessage(t, e));
    }
  }, [t, onError]);

  useEffect(() => {
    if (active) void reload();
  }, [active, reload]);

  useEffect(() => {
    return subscribeDaemonWatch((raw) => {
      if (isJobFinished(raw) || needsResync(raw)) void reload();
    });
  }, [reload]);

  // The choice follows the active server on every reread — except while the user has picked
  // something else and the job is running, so that a `reload()` midway does not pull the Select
  // back to the old value.
  useEffect(() => {
    if (job === null && current !== undefined) setChoice(current ?? SERVERS[0]);
  }, [current, job]);

  async function pollJob(id: number) {
    let summary: JobSummary;
    try {
      summary = await api.jobStatus(id);
    } catch (e) {
      if (!live.current) return;
      setJob(null);
      onError(errorMessage(t, e));
      return;
    }
    if (!live.current) return;
    if (summary.state === "running") {
      setJob(summary);
      setTimeout(() => void pollJob(id), 1000);
      return;
    }
    setJob(null);
    if (summary.state !== "succeeded") {
      if (summary.outcome?.ending === "failed") {
        onError(errorMessage(t, refusal(summary.outcome.error)));
      }
      return;
    }
    if (summary.outcome?.ending !== "succeeded") return;
    const result = summary.outcome.result as FrontEndReport;
    setReport(result);
    if (result.outcome.outcome === "not_granted") {
      // Operations waiting in the elevation queue — show them first, then ask, T64.
      try {
        const queue = await api.elevationStatus();
        if (!live.current) return;
        if (queue.pending.length > 0) {
          setCanPrompt(queue.can_prompt);
          setReason(queue.reason);
          setPending(queue.pending);
          return;
        }
      } catch (e) {
        if (!live.current) return;
        onError(errorMessage(t, e));
      }
      setNotice(t("mixengine.settings.frontEnd.notGranted", { because: result.outcome.because }));
      return;
    }
    void reload();
  }

  async function switchTo(server: FrontEndServer) {
    setConfirming(false);
    setNotice(null);
    setReport(null);
    try {
      const started = await api.serviceSetFrontEnd({ server, grant: false });
      if (!live.current) return;
      setJob(started);
      void pollJob(started.id);
    } catch (e) {
      if (!live.current) return;
      onError(errorMessage(t, e));
    }
  }

  /** After `ElevationDialog`: an empty queue means granted — ask again; something still waiting
   *  means the user only closed it. */
  async function afterElevation() {
    setPending(null);
    try {
      const queue = await api.elevationStatus();
      if (!live.current) return;
      if (queue.pending.length === 0 && choice !== null) {
        void switchTo(choice);
        return;
      }
    } catch (e) {
      if (!live.current) return;
      onError(errorMessage(t, e));
      return;
    }
    setNotice(t("mixengine.settings.frontEnd.stillWaiting"));
  }

  if (services === null) return null;

  // A daemon built before T97: keep this row visible with a reason, instead of disappearing.
  if (current === undefined) {
    return (
      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>{t("mixengine.settings.frontEnd.title")}</h3>
        <p className={styles.muted}>{t("mixengine.settings.frontEnd.unsupported")}</p>
      </section>
    );
  }

  const outcome = report?.outcome;
  const outcomeLine =
    outcome === undefined || outcome.outcome === "not_granted"
      ? null
      : outcome.outcome === "unchanged"
        ? t("mixengine.settings.frontEnd.unchanged", { server: report?.now ?? "" })
        : outcome.outcome === "switched"
          ? t(
              outcome.started
                ? "mixengine.settings.frontEnd.switched"
                : "mixengine.settings.frontEnd.switchedNotStarted",
              { server: report?.now ?? "" },
            )
          : outcome.outcome === "rolled_back"
            ? t("mixengine.settings.frontEnd.rolledBack", { because: outcome.because })
            : t("mixengine.settings.frontEnd.failed", { because: outcome.because });
  const outcomeBad = outcome?.outcome === "rolled_back" || outcome?.outcome === "failed";

  return (
    <section className={styles.section}>
      <h3 className={styles.sectionTitle}>{t("mixengine.settings.frontEnd.title")}</h3>

      <p className={styles.muted}>
        {current === null
          ? t("mixengine.settings.frontEnd.none")
          : t("mixengine.settings.frontEnd.current", { server: current })}
      </p>

      <div className={styles.row}>
        <Select
          value={choice ?? SERVERS[0]}
          onChange={setChoice}
          disabled={job !== null}
          options={SERVERS.map((server) => ({ value: server, label: server }))}
        />
        {job !== null ? (
          <span className={styles.muted}>
            {t("mixengine.settings.frontEnd.switching")}
            {dots}
            {job.message && ` ${job.message}`}
          </span>
        ) : (
          <Button
            variant="primary"
            disabled={choice === null || choice === current}
            onClick={() => setConfirming(true)}
          >
            {t("mixengine.settings.frontEnd.switch")}
          </Button>
        )}
      </div>

      {notice !== null && <p className={styles.warn}>{notice}</p>}

      {outcomeLine !== null && (
        <p className={outcomeBad ? styles.bad : styles.muted}>{outcomeLine}</p>
      )}
      {report && !report.answering && report.now && (
        <p className={styles.warn}>{t("mixengine.settings.frontEnd.notAnswering")}</p>
      )}
      {report?.kept_data && (
        <p className={styles.muted}>
          {t("mixengine.settings.frontEnd.keptData", { path: report.kept_data })}
        </p>
      )}
      {report && report.not_carried.length > 0 && (
        <>
          <p className={styles.warn}>{t("mixengine.settings.frontEnd.notCarried")}</p>
          <ul className={styles.list}>
            {report.not_carried.map((line, index) => (
              // The position is the key: each line is a sentence the daemon wrote, with no other
              // id.
              <li key={index} className={styles.muted}>
                {line}
              </li>
            ))}
          </ul>
        </>
      )}

      {confirming && choice !== null && (
        <ConfirmDialog
          title={t("mixengine.settings.frontEnd.confirmTitle")}
          message={
            current === null
              ? t("mixengine.settings.frontEnd.confirmFirst", { to: choice })
              : t("mixengine.settings.frontEnd.confirmSwitch", { from: current, to: choice })
          }
          confirmLabel={t("mixengine.settings.frontEnd.switch")}
          onCancel={() => setConfirming(false)}
          onConfirm={() => void switchTo(choice)}
        />
      )}

      {pending && (
        <ElevationDialog
          pending={pending}
          canPrompt={canPrompt}
          reason={reason}
          onClose={() => void afterElevation()}
        />
      )}
    </section>
  );
}
