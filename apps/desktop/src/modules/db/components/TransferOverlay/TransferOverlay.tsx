import Button from "../../../../components/Button";
import { useTranslation } from "../../../../i18n";
import type { TransferProgress } from "../../api";
import styles from "./TransferOverlay.module.css";

interface Props {
  /** What is running, in full — "Dumping pnedu...", say. */
  label: string;
  /** How far it has got, for those that can say. Dropping a database and downloading the tools
   *  pass nothing and show the spinner alone. */
  progress?: TransferProgress | null;
  /** Stops the tool. Absent for the waits that have nothing to stop — dropping a database is one
   *  statement, and cancelling a tool download is the download panel's own business. */
  onCancel?: () => void;
}

/**
 * The veil for dumping, restoring, dropping and fetching the tools.
 *
 * Those four differ from every other wait in the app: an external tool is doing the work, it can
 * run for minutes, and nothing in the connection can be touched while it does. So unlike
 * LoadingOverlay's one quiet line over a single panel, this one dims the whole workspace, states
 * in plain size what is happening, and says the wait is expected — otherwise a locked tab reads
 * as a hung one.
 *
 * It covers its nearest positioned ancestor, which is the workspace: the tab bar and the other
 * connections stay live, so a long dump in one tab never holds up work in another.
 */
function TransferOverlay({ label, progress, onCancel }: Props) {
  const { t } = useTranslation();
  // A percentage the Rust side would not commit to. The bar still moves — a dump of a database
  // whose tables it could not measure is going somewhere, it just cannot say how far.
  const measured = progress != null && progress.percent !== null;
  // Only a MySQL dump counts anything out: a restore replays a stream rather than a list, and a
  // mongodump is measured by the archive it writes and names no parts at all. Written as pieces so
  // that whichever of them there is turns out right, rather than a stranded separator.
  const pieces =
    progress == null
      ? []
      : [
          progress.parts > 0
            ? t("dump.progressTables", { at: progress.atPart, total: progress.parts })
            : null,
          measured ? `${progress.percent}%` : null,
        ].filter((piece) => piece !== null);

  return (
    <div className={styles.overlay} role="status" aria-live="polite" aria-busy="true">
      <div className={styles.card}>
        <div className={styles.spinner} aria-hidden="true" />
        <p className={styles.label}>{label}</p>

        {progress != null && (
          /* Deliberately not announced: it changes four times a second, and the label above
             already says what is happening. */
          <div className={styles.progress} aria-hidden="true">
            <div className={styles.progressHead}>
              <span className={styles.progressTable}>{progress.part ?? ""}</span>
              <span className={styles.progressCount}>{pieces.join(" · ")}</span>
            </div>
            <div className={styles.progressTrack}>
              {/* Nothing to fill against, so the bar sweeps instead: movement says "still going"
                  without claiming to know how much is left. */}
              <div
                className={
                  measured ? styles.progressFill : `${styles.progressFill} ${styles.progressSweep}`
                }
                style={measured ? { width: `${progress.percent}%` } : undefined}
              />
            </div>
          </div>
        )}

        <p className={styles.hint}>{t("dump.transferHint")}</p>

        {onCancel && (
          /* Under the hint, not beside the label: stopping is the rarer answer, and a button level
             with "Dumping shop..." would read as the thing to press. */
          <Button className={styles.cancel} onClick={onCancel}>
            {t("dump.cancelTransfer")}
          </Button>
        )}
      </div>
    </div>
  );
}

export default TransferOverlay;
