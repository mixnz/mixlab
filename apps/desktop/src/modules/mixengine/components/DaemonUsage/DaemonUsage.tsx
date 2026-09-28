import type { MetricsSample } from "@mixengine/api";

import { useTranslation } from "../../../../i18n";
import { formatBytes, formatCpu, machineShare } from "../../metricsState";
import styles from "./DaemonUsage.module.css";

/**
 * CPU and memory in one strip — the daemon's in the Dashboard's header, and since T168 the daemon's
 * and every service's added up in the tray panel.
 *
 * Always drawn, before the first frame too: `null` shows "—" in a strip that is already the size
 * it will be, instead of one that appears later and pushes everything under it down.
 */
export default function DaemonUsage({
  reading,
  cores,
  label,
  className,
}: {
  reading: MetricsSample | null;
  /** Số luồng logic của máy (`MetricsFrame.cores`): CPU hiện theo phần trăm cả máy như Task Manager (T190c). */
  cores: number;
  /** What the strip measures, already translated. The daemon's by default. */
  label?: string;
  /** For a caller that lays the strip out differently — the tray stretches it across its card. */
  className?: string;
}) {
  const { t } = useTranslation();
  const name = label ?? t("mixengine.dashboard.daemon");
  return (
    <div className={className ? `${styles.usage} ${className}` : styles.usage} role="group" aria-label={name}>
      <span className={styles.cell}>
        <span className={reading ? styles.liveDot : `${styles.liveDot} ${styles.liveDotIdle}`} aria-hidden="true" />
        <strong>{name}</strong>
      </span>
      <span className={styles.cell}>
        <span className={styles.label}>{t("mixengine.dashboard.cpu")}</span>
        <span className={styles.value}>
          {formatCpu(reading?.cpu_percent ?? null, cores)}
        </span>
        <span className={styles.bar} aria-hidden="true">
          <span
            style={{
              width:
                reading === null
                  ? 0
                  : `${Math.min(100, Math.max(3, machineShare(reading.cpu_percent ?? 0, cores)))}%`,
            }}
          />
        </span>
      </span>
      <span className={styles.cell}>
        <span className={styles.label}>{t("mixengine.dashboard.memory")}</span>
        <span className={styles.value}>{reading === null ? "—" : formatBytes(reading.rss_bytes)}</span>
      </span>
    </div>
  );
}
