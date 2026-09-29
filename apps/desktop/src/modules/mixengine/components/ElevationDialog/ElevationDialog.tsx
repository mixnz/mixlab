import { useEffect, useRef, useState } from "react";

import Modal, { ModalBody } from "../../../../components/Modal";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { GrantOutcome } from "@mixengine/api";
import type { JobSummary } from "@mixengine/api";
import { describeOp } from "../../pendingOps";
import { useRunningDots } from "../../screens/Settings/useRunningDots";
import styles from "./ElevationDialog.module.css";

/**
 * Every operation waiting for administrator rights, then one prompt.
 *
 * The list is shown **before** `elevation.grant` is called, because what people are about to allow
 * is what they get to see. Each row names the operation and, when there is one, exactly what it
 * will change — untranslated, because those are real paths and ports.
 *
 * **`canPrompt: false` hides the "Allow" button entirely**, not just disables it —
 * `ElevationSummary.can_prompt` says plainly "is the helper still there", and on a machine with no
 * helper left (for example right after uninstalling MixEngine on macOS: the helper lives outside
 * `MIXENGINE_HOME` and is removed by the same Uninstall, but an old `hosts-apply` may still be
 * stuck in the elevation queue from before) `elevation.grant` has nothing left to call — an
 * "Allow" button still drawn in that situation promises something the app cannot do. `reason` is
 * the sentence the daemon writes for each platform (e.g. Linux: the whole `pkexec` command to type
 * by hand).
 *
 * **There is no "Skip" button.** `elevation.drop` drops the whole batch without leaving any way to
 * grant it again — closing the dialog (Escape, clicking outside, or the Close button) only hides
 * it; the waiting count on the Dashboard stays as it is, and clicking it again opens this same
 * list.
 *
 * **`elevation.grant` is a job, and the dialog waits for that job to finish before closing.** The
 * RPC answers as soon as the job row is created, while the password box comes up *afterwards*
 * inside the job — closing the dialog as soon as the RPC answered was a real bug: `onClose`
 * triggered the Dashboard's `reload()`, which read `daemon.status` while the queue was still intact
 * (the user had not typed the password yet), and once they had, the daemon emitted no event about
 * the queue (`elevation_required` only fires when the queue *grows*), so the "N waiting" count sat
 * still until someone switched tabs. So this polls `jobStatus` every second until the job finishes,
 * and only then calls `onClose` — and reads the `GrantOutcome` in `result`: `declined` (the
 * password box was closed) keeps the dialog open with a line saying nothing has changed yet and
 * re-enables the Allow button, instead of closing silently and leaving the old count unexplained.
 */
export default function ElevationDialog({
  pending,
  canPrompt,
  reason,
  onClose,
}: {
  pending: unknown[];
  canPrompt: boolean;
  reason?: string | null;
  onClose: () => void;
}) {
  const [busy, setBusy] = useState(false);
  /** The outcome of the last Allow when it did not lead to closing the dialog — a translated
   *  sentence, or `null`. */
  const [notice, setNotice] = useState<string | null>(null);
  const dots = useRunningDots(busy);
  const { t } = useTranslation();
  /** Whether still mounted — set to `true` in the effect body rather than only `false` in the
   *  cleanup, because `React.StrictMode` (dev) runs mount → fake unmount → mount again. */
  const live = useRef(true);
  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
    };
  }, []);

  function settle(summary: JobSummary) {
    setBusy(false);
    if (summary.state === "cancelled") return;
    if (summary.outcome?.ending === "failed") {
      setNotice(summary.outcome.error.message);
      return;
    }
    if (summary.outcome?.ending !== "succeeded") return;
    const grant = summary.outcome.result as GrantOutcome;
    if (grant.outcome === "declined") {
      setNotice(t("mixengine.elevation.declined"));
      return;
    }
    if (grant.outcome === "unavailable") {
      setNotice(t("mixengine.elevation.cannotPromptReason", { reason: grant.reason }));
      return;
    }
    // `completed` — the queue has changed; the Dashboard rereads the count in `onClose`.
    onClose();
  }

  async function pollJob(id: number) {
    let summary: JobSummary;
    try {
      summary = await api.jobStatus(id);
    } catch (e) {
      if (!live.current) return;
      setBusy(false);
      setNotice(errorMessage(t, e));
      return;
    }
    if (!live.current) return;
    if (summary.state === "running") {
      setTimeout(() => void pollJob(id), 1000);
      return;
    }
    settle(summary);
  }

  async function grant() {
    setBusy(true);
    setNotice(null);
    let started: JobSummary;
    try {
      started = await api.elevationGrant();
    } catch (e) {
      if (!live.current) return;
      setBusy(false);
      setNotice(errorMessage(t, e));
      return;
    }
    if (!live.current) return;
    void pollJob(started.id);
  }

  return (
    <Modal
      title={t("mixengine.elevation.title")}
      onClose={onClose}
      locked={busy}
      footerNote={
        busy ? (
          <>
            {t("mixengine.elevation.prompting")}
            {dots}
          </>
        ) : undefined
      }
      actions={[
        { kind: "cancel", label: t("common.close"), disabled: busy },
        ...(canPrompt
          ? [
              {
                kind: "confirm" as const,
                label: t("mixengine.elevation.grant"),
                onClick: () => void grant(),
                disabled: busy,
              },
            ]
          : []),
      ]}
    >
      {() => (
        <>
          <ModalBody>
            <p className={styles.lead}>{t("mixengine.elevation.lead")}</p>
            <ul className={styles.ops}>
              {pending.map((op, at) => {
                const { kind, description, detail } = describeOp(op);
                return (
                  // The position is the key: `PendingOp.id` does exist, but the order is what the
                  // daemon sends and the list is never re-sorted, so both give the same result and
                  // this one does not have to trust a field.
                  <li key={at}>
                    {/* The daemon's sentence first; the technical name after, for anyone who wants
                        to look it up. */}
                    {description && <div>{description}</div>}
                    <code className={styles.kind}>{kind}</code>
                    {detail && <pre className={styles.detail}>{detail}</pre>}
                  </li>
                );
              })}
            </ul>
            {!canPrompt && (
              <p className={styles.cannotPrompt}>
                {reason
                  ? t("mixengine.elevation.cannotPromptReason", { reason })
                  : t("mixengine.elevation.cannotPrompt")}
              </p>
            )}
            {notice !== null && !busy && <p className={styles.cannotPrompt}>{notice}</p>}
          </ModalBody>
        </>
      )}
    </Modal>
  );
}
