import { useCallback, useEffect, useState } from "react";

import Button from "../../../../components/Button";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { DoctorReport } from "@mixengine/api";
import ElevationDialog from "../../components/ElevationDialog";
import { isJobFinished, needsResync } from "../../daemonState";
import { subscribeDaemonWatch } from "../../daemonWatch";
import { doctorChecksInOrder } from "../../settingsState";
import styles from "./Settings.module.css";

/**
 * `daemon.doctor` + `daemon.doctor_repair`.
 *
 * **Repairing reuses exactly the `elevation.status`/`ElevationDialog` queue the Dashboard built in
 * Phase 1** (Decision D3, Metrics/Settings spec) — no second elevation dialog is written. After
 * calling `doctorRepair({ grant: false })`, read `elevation.status`: if something is waiting, open
 * that very dialog; if nothing is (the repair is inside `MIXENGINE_HOME` and needs no rights), just
 * reread the report.
 *
 * **The report is reread every time we come back to this screen and every time a job ends**, not
 * only on mount. `MixEngineTab` keeps every opened screen in the DOM instead of unmounting, so
 * "mount" happens only once for the tab's whole life — a report read exactly once would stand
 * still until the MixEngine tab is closed for good, even though the user has just granted rights
 * on the Dashboard (the "N waiting" button) or from the CLI. A finished `elevation.grant` is a
 * `job_finished`, and the daemon emits no event of its own for "the queue just got shorter" (see
 * `isJobFinished`), so that is the signal to reread; `active` is the fallback when events are
 * dropped.
 */
export default function DoctorSection({
  active,
  onError,
}: {
  active: boolean;
  onError: (message: string) => void;
}) {
  const [report, setReport] = useState<DoctorReport | null>(null);
  const [repairing, setRepairing] = useState(false);
  const [pending, setPending] = useState<unknown[] | null>(null);
  const [canPrompt, setCanPrompt] = useState(true);
  const [reason, setReason] = useState<string | null | undefined>(null);
  const { t } = useTranslation();

  const reload = useCallback(async () => {
    try {
      setReport(await api.doctor());
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

  async function repair() {
    setRepairing(true);
    try {
      await api.doctorRepair({ grant: false });
      const queue = await api.elevationStatus();
      if (queue.pending.length > 0) {
        setCanPrompt(queue.can_prompt);
        setReason(queue.reason);
        setPending(queue.pending);
      } else {
        await reload();
      }
    } catch (e) {
      onError(errorMessage(t, e));
    } finally {
      setRepairing(false);
    }
  }

  if (report === null) return null;

  return (
    <section className={styles.section}>
      <h3 className={styles.sectionTitle}>{t("mixengine.settings.doctor.title")}</h3>

      <ul className={styles.list}>
        {doctorChecksInOrder(report).map((check, index) => (
          // The position is the key: the report has no other id, and the fixed order is exactly
          // what is being tested.
          <li key={index} className={styles.listItem}>
            <span>{check.name}</span>
            <span
              className={
                check.outcome.outcome === "ok"
                  ? styles.ok
                  : check.outcome.outcome === "problem"
                    ? styles.bad
                    : styles.muted
              }
            >
              {check.outcome.outcome === "ok"
                ? t("mixengine.settings.doctor.ok")
                : "because" in check.outcome
                  ? check.outcome.because
                  : ""}
            </span>
          </li>
        ))}
      </ul>

      <Button onClick={() => void repair()} disabled={repairing}>
        {t("mixengine.settings.doctor.repair")}
      </Button>

      {pending && (
        <ElevationDialog
          pending={pending}
          canPrompt={canPrompt}
          reason={reason}
          onClose={() => {
            setPending(null);
            void reload();
          }}
        />
      )}
    </section>
  );
}
