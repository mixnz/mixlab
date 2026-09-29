import Button from "../../../../components/Button";
import Card from "../../../../components/Card";
import { LockIcon, ReloadIcon } from "../../../../icons";
import { useTranslation } from "../../../../i18n";
import type { CategoryUsage } from "@mixengine/api";
import type { DiskUsage } from "@mixengine/api";
import { isCleanupReclaimable } from "../../diskUsageState";
import { formatBytes } from "../../metricsState";
import styles from "./DiskUsagePanel.module.css";

interface Props {
  disk: DiskUsage | null;
  refreshing: boolean;
  onRefresh: () => void;
  onCleanup: () => void;
}

/** Why a category can or cannot be cleaned — the four `Reclaim` variants, one sentence each; the
 *  daemon says so rather than MixDB inferring it from the category name (just as the `Reclaim`
 *  doc comment says). */
function reclaimNote(category: CategoryUsage, t: ReturnType<typeof useTranslation>["t"]): string {
  const { reclaim } = category;
  switch (reclaim.reclaim) {
    case "by_cleanup":
      return t("mixengine.dashboard.diskUsage.reclaimByCleanup", {
        size: formatBytes(reclaim.bytes),
      });
    case "by_method":
      return reclaim.because;
    case "at_a_cost":
      return reclaim.because;
    case "never":
      return reclaim.because;
  }
}

/** The tag beside a category: what `Reclaim` says can be done about it, in two words. Drawn from
 *  the daemon's answer, never from the category's name. */
function ReclaimTag({ category }: { category: CategoryUsage }) {
  const { t } = useTranslation();
  const { reclaim } = category;
  switch (reclaim.reclaim) {
    case "by_cleanup":
      return reclaim.bytes > 0 ? (
        <span className={`${styles.tag} ${styles.tagAccent}`}>
          {t("mixengine.dashboard.diskUsage.tagReclaimable", { size: formatBytes(reclaim.bytes) })}
        </span>
      ) : (
        <span className={`${styles.tag} ${styles.tagMuted}`}>
          {t("mixengine.dashboard.diskUsage.tagNothing")}
        </span>
      );
    case "never":
      return (
        <span className={`${styles.tag} ${styles.tagLocked}`}>
          <LockIcon size={11} />
          {t("mixengine.dashboard.diskUsage.tagProtected")}
        </span>
      );
    case "by_method":
      return <span className={styles.tag}>{t("mixengine.dashboard.diskUsage.tagElsewhere")}</span>;
    case "at_a_cost":
      return <span className={styles.tag}>{t("mixengine.dashboard.diskUsage.tagAtACost")}</span>;
  }
}

/**
 * The disk usage table with 5 fixed categories (T96), each drawn differently according to the
 * `Reclaim` the daemon returns.
 *
 * **Only two categories (`logs`, `cache`) have a clean-up button** — the other three (`runtimes`,
 * `data`, `certs`) are read-only and point the user to the right place to clean them. The "Clean
 * up" button always shows — the dialog itself filters down to exactly the two cleanable rows.
 */
export default function DiskUsagePanel({ disk, refreshing, onRefresh, onCleanup }: Props) {
  const { t } = useTranslation();

  if (disk === null) return null;

  const total = disk.categories.reduce((sum, category) => sum + category.bytes, 0) + disk.other_bytes;

  return (
    <Card
      title={t("mixengine.dashboard.diskUsage.title")}
      count={t("mixengine.dashboard.diskUsage.measuredAt", {
        time: new Date(disk.measured_at).toLocaleTimeString(),
      })}
      description={t("mixengine.dashboard.diskUsage.total", { size: formatBytes(total) })}
      actions={
        <>
          <Button onClick={onRefresh} disabled={refreshing}>
            <ReloadIcon size={14} className={refreshing ? styles.spin : undefined} />
            {t("mixengine.dashboard.diskUsage.refresh")}
          </Button>
          <Button
            variant="soft"
            onClick={onCleanup}
            disabled={!disk.categories.some((category) => isCleanupReclaimable(category))}
          >
            {t("mixengine.dashboard.diskUsage.cleanup")}
          </Button>
        </>
      }
    >
      <div className={styles.bar} aria-hidden="true">
        {disk.categories.map((category) => (
          <span
            key={category.id}
            className={styles[category.id]}
            style={{ flexGrow: Math.max(category.bytes, 1) }}
          />
        ))}
      </div>

      <ul className={styles.categories}>
        {disk.categories.map((category) => (
          <li key={category.id} className={styles.category}>
            <span className={`${styles.swatch} ${styles[category.id]}`} aria-hidden="true" />
            <span className={styles.name}>{t(`mixengine.dashboard.diskUsage.category.${category.id}`)}</span>
            <span className={styles.size} title={category.location}>
              {formatBytes(category.bytes)}
            </span>
            <span className={styles.note}>{reclaimNote(category, t)}</span>
            <ReclaimTag category={category} />
            {category.unreadable != null && (
              <span className={styles.unreadable}>
                {t("mixengine.dashboard.diskUsage.unreadable", { reason: category.unreadable })}
              </span>
            )}
          </li>
        ))}
      </ul>

      <p className={styles.other}>
        {t("mixengine.dashboard.diskUsage.other", { size: formatBytes(disk.other_bytes) })}
      </p>
    </Card>
  );
}
