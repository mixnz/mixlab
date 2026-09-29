import { useTranslation } from "../../../../i18n";
import styles from "./StaleBadge.module.css";

/**
 * "The list may be out of date" — for `RuntimeCatalogue.stale`/`PackageCatalogue.stale`. One
 * component for both (D3): the same shape, the same reason to exist — a cache that could not be
 * refreshed is still usable, and staying silent about it is lying that the network was reached.
 */
export default function StaleBadge({ stale }: { stale: boolean }) {
  const { t } = useTranslation();
  if (!stale) return null;
  return <span className={styles.badge}>{t("mixengine.packages.stale")}</span>;
}
