import { useTranslation } from "../../../../i18n";
import styles from "./StaleBadge.module.css";

/**
 * "Danh sách có thể cũ" — cho `RuntimeCatalogue.stale`/`PackageCatalogue.stale`. Một component cho
 * cả hai (D3): cùng hình dạng, cùng lý do tồn tại — cache không refresh được vẫn dùng được, im lặng
 * về nó là nói dối đã hỏi được mạng.
 */
export default function StaleBadge({ stale }: { stale: boolean }) {
  const { t } = useTranslation();
  if (!stale) return null;
  return <span className={styles.badge}>{t("mixengine.packages.stale")}</span>;
}
