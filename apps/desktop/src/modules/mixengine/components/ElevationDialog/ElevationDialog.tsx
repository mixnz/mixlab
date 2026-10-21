import { useEffect, useRef, useState } from "react";

import Button from "../../../../components/Button";
import Modal, { ModalBody } from "../../../../components/Modal";
import { copyText } from "../../../../core/clipboard";
import { errorMessage } from "../../../../core/errors";
import { CopyIcon } from "../../../../icons";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { GrantOutcome } from "@mixengine/api";
import type { JobSummary } from "@mixengine/api";
import {
  describeOp,
  isGrantInFlight,
  isNothingWaiting,
  otherGrant,
  terminalCommand,
  whatTheGrantWaitsFor,
} from "../../pendingOps";
import { useRunningDots } from "../../screens/Settings/useRunningDots";
import styles from "./ElevationDialog.module.css";
import { leadKeys } from "./lead";

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
 * the sentence the daemon writes for each platform (e.g. Linux: what is missing, and that
 * `mix elevation grant` works from a terminal).
 *
 * **A grant `mix` started from a terminal is shown, never run** (ADR 0065). While the daemon waits
 * for one, `elevation.status` carries the command, and this dialog shows it with a copy button on
 * every system — on a Mac `canPrompt` is true and the command is still the way forward. The window
 * has no terminal for `sudo` to ask in, so it never starts a grant of that kind itself. While it
 * follows such a grant, the footer says it waits for a terminal, because no system prompt exists;
 * and a row that grant left for the screen says so under it, in the daemon's words.
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
 * the queue (`elevation_required` then fired only when the queue *grew*), so the "N waiting" count sat
 * still until someone switched tabs. So this polls `jobStatus` every second until the job finishes,
 * and only then calls `onClose` — and reads the `GrantOutcome` in `result`: `declined` (the
 * password box was closed) keeps the dialog open with a line saying nothing has changed yet and
 * re-enables the Allow button, instead of closing silently and leaving the old count unexplained.
 *
 * **A grant somebody else started is followed, not refused.** The daemon raises the helper's
 * prompt itself on its first start after an update, and on macOS and Linux its password box leaves
 * this dialog clickable; "Allow" then comes back `conflict`. Rather than show that, the dialog
 * waits on `elevation.status` until that grant ends and answers with its outcome. Close stays
 * available meanwhile — that grant is not this dialog's to wait out, and a grant that fails before
 * recording an outcome would otherwise hold it open for good.
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
  /** Waiting on a grant this dialog did not start. */
  const [following, setFollowing] = useState(false);
  /** The outcome of the last Allow when it did not lead to closing the dialog — a translated
   *  sentence, or `null`. */
  const [notice, setNotice] = useState<string | null>(null);
  /** The command a grant started from a terminal waits for, while one does. */
  const [command, setCommand] = useState<string | null>(null);
  /** Per row id, why a terminal's grant leaves it for the screen — from the same reading. */
  const [screenOnly, setScreenOnly] = useState<Map<number, string>>(new Map());
  const dots = useRunningDots(busy || following);
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

  // A terminal's grant can be waiting on any system, whatever `canPrompt` says. Read again every
  // two seconds: the grant may be started, or end, while this dialog is open.
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    let stopped = false;
    const read = async () => {
      try {
        const status = await api.elevationStatus();
        if (!stopped) {
          setCommand(terminalCommand(status.waiting));
          setScreenOnly(
            new Map(
              status.pending.flatMap((row) =>
                row.needs_the_screen ? [[row.id, row.needs_the_screen] as const] : [],
              ),
            ),
          );
        }
      } catch {
        // The command is a convenience; the dialog is still right without it.
      }
      if (!stopped) timer = setTimeout(() => void read(), 2000);
    };
    void read();
    return () => {
      stopped = true;
      clearTimeout(timer);
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
    settleGrant(summary.outcome.result as GrantOutcome);
  }

  function settleGrant(grant: GrantOutcome) {
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

  async function followOther(before: number | null) {
    let status;
    try {
      status = await api.elevationStatus();
    } catch (e) {
      if (!live.current) return;
      setFollowing(false);
      setNotice(errorMessage(t, e));
      return;
    }
    if (!live.current) return;
    const other = otherGrant(before, status);
    if (other.state === "waiting") {
      setTimeout(() => void followOther(before), 1000);
      return;
    }
    setFollowing(false);
    if (other.state === "emptied") {
      onClose();
      return;
    }
    settleGrant(other.grant);
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
      // Somebody allowed this list already — the daemon's own prompt after an update, or another
      // window. What the person wanted is done, so close as a completed grant would.
      if (isNothingWaiting(e)) {
        onClose();
        return;
      }
      if (isGrantInFlight(e)) {
        setFollowing(true);
        let before: number | null = null;
        try {
          before = (await api.elevationStatus()).last?.job ?? null;
        } catch {
          // Unknown: any outcome followOther reads is then taken as the one being waited for.
        }
        if (!live.current) return;
        void followOther(before);
        return;
      }
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
        busy || following ? (
          <>
            {whatTheGrantWaitsFor(command) === "terminal"
              ? t("mixengine.elevation.waitingForTerminal")
              : t("mixengine.elevation.prompting")}
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
                disabled: busy || following,
              },
            ]
          : []),
      ]}
    >
      {() => (
        <>
          <ModalBody>
            {/* A terminal's grant carries only what a terminal can, so "one prompt" is not true of
                the list while one waits; nor where no prompt can be raised (T211h). */}
            <p className={styles.lead}>
              {leadKeys({
                canPrompt,
                terminalWaits: command !== null,
                anyMarked: pending.some((op) => {
                  const id = (op as { id?: unknown }).id;
                  return typeof id === "number" && screenOnly.has(id);
                }),
              })
                .map((key) => t(key))
                .join(" ")}
            </p>
            <ul className={styles.ops}>
              {pending.map((op, at) => {
                const { kind, description, detail } = describeOp(op);
                const id = (op as { id?: unknown }).id;
                const leftForTheScreen =
                  command !== null && typeof id === "number" ? screenOnly.get(id) : undefined;
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
                    {leftForTheScreen && (
                      <p className={styles.cannotPrompt}>{leftForTheScreen}</p>
                    )}
                  </li>
                );
              })}
            </ul>
            {/* Not while a terminal's grant waits: the way out this sentence gives (on a Mac,
                `mix daemon stop`) would cancel that grant, and the command below is the way on. */}
            {!canPrompt && command === null && (
              <p className={styles.cannotPrompt}>
                {reason
                  ? t("mixengine.elevation.cannotPromptReason", { reason })
                  : t("mixengine.elevation.cannotPrompt")}
              </p>
            )}
            {command !== null && (
              <div className={styles.waiting}>
                <p className={styles.lead}>{t("mixengine.elevation.waitingCommand")}</p>
                <pre className={`${styles.detail} ${styles.command}`}>{command}</pre>
                <Button
                  size="small"
                  variant="ghost"
                  onClick={() =>
                    void copyText(command).catch((e: unknown) => setNotice(errorMessage(t, e)))
                  }
                >
                  <CopyIcon size={14} />
                  {t("mixengine.elevation.copyCommand")}
                </Button>
              </div>
            )}
            {notice !== null && !busy && !following && (
              <p className={styles.cannotPrompt}>{notice}</p>
            )}
          </ModalBody>
        </>
      )}
    </Modal>
  );
}
