import type { MetricsSample } from "@mixengine/api";

import { useTranslation } from "../../../../i18n";
import { useCpuScale } from "../../cpuScale";
import { cpuShare, formatBytes, formatCpu } from "../../metricsState";
import CpuRing from "../CpuRing";
import styles from "./DaemonUsage.module.css";

/**
 * CPU and memory in one strip — the daemon's in the Dashboard's header, and since T168 the daemon's
 * and every service's added up in the tray panel. The Dashboard draws it `inline`, as a few words on
 * the line under its title with a ring for the CPU; the tray keeps the boxed strip with a bar.
 *
 * Always drawn, before the first frame too: `null` shows "—" in a strip that is already the size
 * it will be, instead of one that appears later and pushes everything under it down.
 */
export default function DaemonUsage({
  reading,
  cores,
  label,
  className,
  inline = false,
}: {
  reading: MetricsSample | null;
  /** The machine's logical thread count (`MetricsFrame.cores`): CPU is shown as a percentage of the
   *  whole machine, like Task Manager (T190c). */
  cores: number;
  /** What the strip measures, already translated. The daemon's by default. */
  label?: string;
  /** For a caller that lays the strip out differently — the tray stretches it across its card. */
  className?: string;
  /** Words on a line rather than a boxed strip: the Dashboard's header. */
  inline?: boolean;
}) {
  const { t } = useTranslation();
  const name = label ?? t("mixengine.dashboard.daemon");
  const scale = useCpuScale();
  const dot = <span className={reading ? styles.liveDot : `${styles.liveDot} ${styles.liveDotIdle}`} aria-hidden="true" />;
  if (inline) {
    return (
      <div className={className ? `${styles.inline} ${className}` : styles.inline} role="group" aria-label={name}>
        <span className={styles.inlineCell}>
          {dot}
          <span className={styles.inlineName}>{name}</span>
        </span>
        <span className={styles.inlineCell}>
          <CpuRing share={reading === null ? null : cpuShare(reading.cpu_percent ?? 0, cores, scale)} />
          <span className={styles.inlineValue}>
            {t("mixengine.dashboard.cpu")} {formatCpu(reading?.cpu_percent ?? null, cores, scale)}
          </span>
        </span>
        <span className={styles.inlineValue}>
          {t("mixengine.dashboard.ram")} {reading === null ? "—" : formatBytes(reading.rss_bytes)}
        </span>
      </div>
    );
  }
  return (
    <div className={className ? `${styles.usage} ${className}` : styles.usage} role="group" aria-label={name}>
      <span className={styles.cell}>
        {dot}
        <strong>{name}</strong>
      </span>
      <span className={styles.cell}>
        <span className={styles.label}>{t("mixengine.dashboard.cpu")}</span>
        <span className={styles.value}>
          {formatCpu(reading?.cpu_percent ?? null, cores, scale)}
        </span>
        <span className={styles.bar} aria-hidden="true">
          <span
            style={{
              width:
                reading === null
                  ? 0
                  : `${Math.min(100, Math.max(3, cpuShare(reading.cpu_percent ?? 0, cores, scale)))}%`,
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
