import { useCallback, useEffect, useState } from "react";
import Button from "../../../components/Button";
import Checkbox from "../../../components/Checkbox";
import LoadingState from "../../../components/LoadingState";
import Modal, { ModalBody, ModalErrors } from "../../../components/Modal";
import NoticeBanner from "../../../components/NoticeBanner";
import { errorMessage } from "../../../core/errors";
import { useTranslation } from "../../../i18n";
import {
  blockedRows,
  canRemove,
  declined,
  failedRows,
  relocatedRows,
  uninstallPlan,
  uninstallRun,
  type Outcome,
  type Plan,
  type Stage,
} from "../../uninstall";
import styles from "./RemoveDialog.module.css";

interface Props {
  onClose: () => void;
}

/**
 * MixLab ▸ Remove MixLab from this Mac…: T182a, `docs/specs/2026-10-07-t182a-an-uninstall-path-for-macos-design.md`, D4.
 *
 * The plan first, with what is in the way, and nothing changes until **Remove MixLab**. A finished
 * removal ends the process from Rust, so this never draws "done": it draws only the cases where the
 * window is still needed — a declined prompt, a row left behind, a daemon that would not start.
 */
function RemoveDialog({ onClose }: Props) {
  const { t } = useTranslation();
  const [stage, setStage] = useState<Stage>("checking");
  const [plan, setPlan] = useState<Plan | null>(null);
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const [error, setError] = useState("");
  /* Both unticked: keeping the data is the default, as on the Windows uninstaller's page (T182, D7). */
  const [deleteData, setDeleteData] = useState(false);
  const [deleteRelocated, setDeleteRelocated] = useState(false);

  const check = useCallback(async () => {
    setStage("checking");
    setError("");
    try {
      setPlan(await uninstallPlan(!deleteData, !deleteRelocated));
      setStage("plan");
    } catch (failure) {
      setError(errorMessage(t, failure));
      setStage("failed");
    }
  }, [deleteData, deleteRelocated, t]);

  useEffect(() => {
    void check();
  }, [check]);

  const remove = async () => {
    setStage("removing");
    setError("");
    try {
      setOutcome(await uninstallRun(!deleteData, !deleteRelocated));
    } catch (failure) {
      setError(errorMessage(t, failure));
    }
    setStage("failed");
  };

  const blocked = plan ? blockedRows(plan) : [];
  const relocated = plan ? relocatedRows(plan) : [];
  const home = plan?.items.find((row) => row.id === "home")?.location ?? "";
  const left = outcome ? [...failedRows(outcome.report).map((row) => row.what), ...outcome.left] : [];

  return (
    <Modal
      title={t("remove.title")}
      size="normal"
      fixedHeight
      /* Above the MixEngine module's own "needs an administrator" dialog (the default layer), which
         the uninstall's queued operations make it draw: the one prompt is this dialog's to wait for. */
      layer={90}
      locked={stage === "removing"}
      onClose={onClose}
      footerNote={stage === "removing" ? t("remove.waiting") : undefined}
      actions={[
        { kind: "cancel", label: t("common.cancel"), disabled: stage === "removing" },
        {
          kind: "danger",
          label: t("remove.remove"),
          onClick: () => void remove(),
          disabled: !canRemove(stage, plan),
          busy: stage === "removing" ? t("remove.removing") : undefined,
        },
      ]}
    >
      {() => (
        <>
          <ModalBody>
            {stage === "checking" ? (
              <LoadingState label={t("remove.checking")} compact />
            ) : (
              <div className={styles.stack}>
                <p className={styles.intro}>{t("remove.intro")}</p>

                {outcome && declined(outcome.report) && <NoticeBanner message={t("remove.declined")} />}
                {outcome && !declined(outcome.report) && <NoticeBanner message={t("remove.failed")} />}
                {stage === "failed" && (
                  <div>
                    <Button onClick={() => void check()}>{t("remove.checkAgain")}</Button>
                  </div>
                )}
                {outcome && !declined(outcome.report) && left.length > 0 && (
                  <div>
                    <p className={styles.intro}>{t("remove.left")}</p>
                    <ul className={styles.paths}>
                      {left.map((what) => (
                        <li key={what}>{what}</li>
                      ))}
                    </ul>
                  </div>
                )}

                {blocked.map((row) => (
                  <NoticeBanner key={`${row.location}${row.by}`} message={t("remove.blocked", { what: row.what, by: row.by })} />
                ))}
                {/* `remove.blocked` is "{{what}}: {{by}}": the daemon's sentence already says what to do. */}
                {blocked.length > 0 && (
                  <div>
                    <Button onClick={() => void check()}>{t("remove.checkAgain")}</Button>
                  </div>
                )}

                {plan && (
                  <ul className={styles.rows}>
                    {plan.items
                      .filter((row) => row.outcome.removal !== "blocked")
                      .map((row) => {
                        const quiet = row.outcome.removal === "absent" || row.outcome.removal === "kept";
                        return (
                          <li key={`${row.id}${row.location}`} className={`${styles.row} ${quiet ? styles.quiet : ""}`}>
                            <span>{row.what}</span>
                            <span className={styles.location}>{row.location}</span>
                          </li>
                        );
                      })}
                  </ul>
                )}

                {stage === "plan" && (
                  <div className={styles.choices}>
                    <Checkbox
                      label={t("remove.deleteData", { path: home })}
                      checked={deleteData}
                      onChange={(e) => setDeleteData(e.target.checked)}
                    />
                    {relocated.length > 0 && (
                      <>
                        <Checkbox
                          label={t("remove.deleteRelocated")}
                          checked={deleteRelocated}
                          onChange={(e) => setDeleteRelocated(e.target.checked)}
                        />
                        <ul className={styles.paths}>
                          {relocated.map((path) => (
                            <li key={path}>{path}</li>
                          ))}
                        </ul>
                      </>
                    )}
                  </div>
                )}
              </div>
            )}
          </ModalBody>
          <ModalErrors messages={[error]} />
        </>
      )}
    </Modal>
  );
}

export default RemoveDialog;
