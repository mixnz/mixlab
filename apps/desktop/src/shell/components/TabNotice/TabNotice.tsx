import Button from "../../../components/Button";
import { CloseIcon } from "../../../icons";
import { useTranslation } from "../../../i18n";
import styles from "./TabNotice.module.css";

interface Props {
  /** Already translated — this draws a sentence and does not compose one. */
  message: string;
  onDismiss: () => void;
  /** One thing to do about it, drawn beside the sentence — MixLab's update notice. */
  action?: { label: string; onClick: () => void };
  /** What the close button says to a screen reader; the module notice's words when absent. */
  dismissLabel?: string;
}

/**
 * One line at the top of a tab, about the tab.
 *
 * `role="status"` and not `role="alert"`: nothing failed and nothing is interrupted — this says
 * that the window changed shape on the way to opening this tab. No timer either; `ErrorBanner`'s
 * sixty seconds are there because an error stops being news, and this stays true until someone acts
 * on it.
 */
function TabNotice({ message, onDismiss, action, dismissLabel }: Props) {
  const { t } = useTranslation();
  return (
    <p className={styles.notice} role="status">
      <span className={styles.message}>{message}</span>
      {action && (
        <Button size="small" onClick={action.onClick}>
          {action.label}
        </Button>
      )}
      <button
        type="button"
        className={styles.dismiss}
        aria-label={dismissLabel ?? t("profiles.turnedOnDismiss")}
        onClick={onDismiss}
      >
        <CloseIcon />
      </button>
    </p>
  );
}

export default TabNotice;
