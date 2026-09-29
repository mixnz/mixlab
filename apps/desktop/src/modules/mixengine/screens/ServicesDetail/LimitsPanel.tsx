import { useCallback, useEffect, useState } from "react";

import Button from "../../../../components/Button";
import Card from "../../../../components/Card";
import ErrorBanner from "../../../../components/ErrorBanner";
import LoadingState from "../../../../components/LoadingState";
import Input from "../../../../components/Input";
import Select from "../../../../components/Select";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { ServiceLimitsReport } from "@mixengine/api";
import { enforcementKind, enforcementReason } from "../../limitsState";
import styles from "./LimitsPanel.module.css";

export default function LimitsPanel({ service }: { service: string }) {
  const [report, setReport] = useState<ServiceLimitsReport | null>(null);
  /** False until the first read has answered, failed or not. */
  const [loaded, setLoaded] = useState(false);
  const [cpu, setCpu] = useState("");
  const [memory, setMemory] = useState("");
  const [priority, setPriority] = useState<"normal" | "background">("normal");
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState("");
  const { t } = useTranslation();

  const reload = useCallback(async () => {
    try {
      const next = await api.serviceLimits(service);
      setReport(next);
      setCpu(next.limits.cpu_percent === null ? "" : String(next.limits.cpu_percent));
      setMemory(next.limits.memory_mb === null ? "" : String(next.limits.memory_mb));
      setPriority(next.limits.priority);
      setError("");
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setLoaded(true);
    }
  }, [service, t]);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function save() {
    setSaving(true);
    setSaved(false);
    setError("");
    try {
      const next = await api.serviceSetLimits(service, {
        cpu_percent: cpu.trim() === "" ? null : Number(cpu),
        memory_mb: memory.trim() === "" ? null : Number(memory),
        priority,
      });
      setReport(next);
      setSaved(true);
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setSaving(false);
    }
  }

  if (report === null) {
    return loaded ? null : (
      <Card headingLevel={3} title={t("mixengine.servicesDetail.limits.title")}>
        <LoadingState compact />
      </Card>
    );
  }

  const cpuKind = enforcementKind(report.support.cpu);
  const memoryKind = enforcementKind(report.support.memory);

  return (
    <Card headingLevel={3} title={t("mixengine.servicesDetail.limits.title")}>
      {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}

      <div className={styles.fields}>
        <label className={styles.field}>
          {t("mixengine.servicesDetail.limits.cpu")}
          {cpuKind === "unsupported" ? (
            <p className={styles.hint}>{t("mixengine.servicesDetail.limits.unsupported")}</p>
          ) : (
            <>
              <Input
                type="number"
                mono
                value={cpu}
                disabled={saving}
                onChange={(e) => setCpu(e.target.value)}
              />
              {/* Limits are declared per core, unlike how the tray and the Dashboard show usage
                  (T190c). */}
              <p className={styles.hint}>{t("mixengine.servicesDetail.limits.cpuHint")}</p>
              {cpuKind === "unavailable" && (
                <p className={styles.hint}>
                  {t("mixengine.servicesDetail.limits.unavailable", {
                    reason: enforcementReason(report.support.cpu) ?? "",
                  })}
                </p>
              )}
              {cpuKind === "advisory" && (
                <p className={styles.hint}>{t("mixengine.servicesDetail.limits.advisoryNote")}</p>
              )}
            </>
          )}
        </label>

        <label className={styles.field}>
          {t("mixengine.servicesDetail.limits.memory")}
          {memoryKind === "unsupported" ? (
            <p className={styles.hint}>{t("mixengine.servicesDetail.limits.unsupported")}</p>
          ) : (
            <>
              <Input
                type="number"
                mono
                value={memory}
                disabled={saving}
                onChange={(e) => setMemory(e.target.value)}
              />
              {memoryKind === "unavailable" && (
                <p className={styles.hint}>
                  {t("mixengine.servicesDetail.limits.unavailable", {
                    reason: enforcementReason(report.support.memory) ?? "",
                  })}
                </p>
              )}
              {memoryKind === "advisory" && (
                <p className={styles.hint}>{t("mixengine.servicesDetail.limits.advisoryNote")}</p>
              )}
            </>
          )}
        </label>

        <label className={styles.field}>
          {t("mixengine.servicesDetail.limits.priority")}
          <Select
            value={priority}
            disabled={saving || !report.support.priority}
            onChange={setPriority}
            options={[
              { value: "normal", label: t("mixengine.servicesDetail.limits.priorityNormal") },
              { value: "background", label: t("mixengine.servicesDetail.limits.priorityBackground") },
            ]}
          />
        </label>
      </div>

      <div className={styles.footer}>
        {/* `watchdog: null` lumps two different cases together (the machine can enforce it itself,
            or no memory_mb was declared) — neither is inferred; it only says "nothing is
            watching". */}
        <p className={styles.watchdog}>
          {report.watchdog === null || report.watchdog === undefined
            ? t("mixengine.servicesDetail.limits.watchdogNone")
            : report.watchdog.restarts
              ? t("mixengine.servicesDetail.limits.watchdogRestarts", {
                  minutes: report.watchdog.after_minutes,
                })
              : t("mixengine.servicesDetail.limits.watchdogWarnsOnly", {
                  minutes: report.watchdog.after_minutes,
                })}
        </p>
        {saved && <span className={styles.saved}>{t("mixengine.servicesDetail.limits.saved")}</span>}
        <Button variant="primary" onClick={() => void save()} disabled={saving}>
          {t("mixengine.servicesDetail.limits.save")}
        </Button>
      </div>
    </Card>
  );
}