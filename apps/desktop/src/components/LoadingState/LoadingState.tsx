import { useTranslation } from "../../i18n";
import styles from "./LoadingState.module.css";

interface Props {
  /** What is being read. Defaults to the generic "Loading..." text. */
  label?: string;
  /** Less padding, for a section inside a card rather than a card of its own. */
  compact?: boolean;
}

/** Nothing to show *yet* — drawn where `EmptyState` would be while the first read is still out.
 *
 * `LoadingOverlay` is for a refetch over data already on screen; this is for the read before any
 * data exists. Without it a list that starts empty says "nothing here" until the answer lands,
 * which is worse than a blank: it is a claim, and a wrong one. */
function LoadingState({ label, compact }: Props) {
  const { t } = useTranslation();

  return (
    /* `status`, as `LoadingOverlay` has it: worth saying, not worth interrupting for. */
    <div className={`${styles.loading}${compact ? ` ${styles.compact}` : ""}`} role="status">
      {/* Decoration on a line that already says what is happening. */}
      <span className={styles.spinner} aria-hidden="true" />
      <span className={styles.label}>{label ?? t("common.loading")}</span>
    </div>
  );
}

export default LoadingState;
