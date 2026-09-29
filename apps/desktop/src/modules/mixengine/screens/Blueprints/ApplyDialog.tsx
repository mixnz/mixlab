import { useEffect, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import Button from "../../../../components/Button";
import Input from "../../../../components/Input";
import Modal, { ModalBody, ModalErrors, type ModalAction } from "../../../../components/Modal";
import Checkbox from "../../../../components/Checkbox";
import NoticeBanner from "../../../../components/NoticeBanner";
import { errorMessage } from "../../../../core/errors";
import { useTailScroll } from "../../../../core/tailScroll";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { BlueprintApplied } from "@mixengine/api";
import type { BlueprintPlan } from "@mixengine/api";
import type { BlueprintSummary } from "@mixengine/api";
import type { MismatchAnswer } from "@mixengine/api";
import type { Requirement } from "@mixengine/api";
import {
  needLabel,
  requirementStep,
  requirementsAllowApply,
  splitLibraries,
} from "../../requirementStep";
import {
  answerSubjectFor,
  blueprintAppliedFrom,
  buildAnswers,
  buildScaffoldConsent,
  canApply,
  describePlanAction,
  jobFailureMessage,
  scaffoldConsentState,
  scaffoldLeftCommand,
  scaffoldStepIndex,
} from "../../blueprintPlan";
import { applyJob, type JobRow } from "../../daemonState";
import { subscribeDaemonWatch } from "../../daemonWatch";
import { applyLogFrame, type LogEntry } from "../../logState";
import { jobFor } from "../../runtimeState";
import styles from "./ApplyDialog.module.css";

interface Props {
  blueprint: BlueprintSummary;
  onCancel: () => void;

  /**
   * The dialog has closed after an apply. `applied` is the result when it succeeded, `null` when it
   * failed — the caller needs it to decide what comes next (`AfterApply`), and above all to know
   * whether the init command ran.
   */
  onDone: (applied: BlueprintApplied | null) => void;

  /** Prefills the project name. Quick Start has already asked, so the user does not type it again
   *  — T117. */
  initialProject?: string;

  /** Prefills the directory, for the same reason. */
  initialRoot?: string;

  /**
   * Also installs a web server if this home has none yet — `BlueprintApply.front_end`, T115.
   *
   * **Off** by default: an apply is about one project, while setting up the machine it runs on is
   * a broader matter and has to be asked. Quick Start is the only place that turns it on, because
   * its question really is *give me a site that works*.
   */
  withFrontEnd?: boolean;
}

type Phase =
  | { kind: "form" }
  | { kind: "plan"; plan: BlueprintPlan; needs: Requirement[] }
  | { kind: "running"; jobId: number }
  | { kind: "done"; applied: BlueprintApplied }
  | { kind: "failed"; message: string };

/**
 * One method (`blueprint.apply`), called twice. Pass 1 (`dry_run: true`) only reads; pass 2
 * (`dry_run: false`) is the only one that really does anything, and can only be sent once
 * `canApply` agrees.
 */
export default function ApplyDialog({
  blueprint,
  onCancel,
  onDone,
  initialProject = "",
  initialRoot = "",
  withFrontEnd = false,
}: Props) {
  const { t } = useTranslation();
  const [project, setProject] = useState(initialProject);
  const [root, setRoot] = useState(initialRoot);
  const [phase, setPhase] = useState<Phase>({ kind: "form" });
  const [choices, setChoices] = useState<Record<number, MismatchAnswer>>({});
  const [scaffoldAgreed, setScaffoldAgreed] = useState(false);
  // Agreement to install what the plan's releases lack on this machine — T152.
  const [prerequisitesAgreed, setPrerequisitesAgreed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [jobs, setJobs] = useState<JobRow[]>([]);
  const [showLog, setShowLog] = useState(false);
  const [logEntries, setLogEntries] = useState<LogEntry[]>([]);
  const logPane = useTailScroll<HTMLDivElement>(logEntries);

  async function browseRoot() {
    const picked = await openDialog({ directory: true, multiple: false });
    if (typeof picked === "string") setRoot(picked);
  }

  async function preview() {
    setBusy(true);
    setError("");
    try {
      const response = await api.blueprintApply({
        blueprint: blueprint.slug,
        project,
        root,
        // **The directory the user picks is the project's directory** — T120c, D1. Both windows
        // understand "pick a directory" the same way; the only place still joining paths is `mix`
        // without `--path`, because there nobody picks a directory at all.
        root_is_parent: false,
        dry_run: true,
        front_end: withFrontEnd,
        // **Always off, and no prop turns it on** — `BlueprintApply.autostart` (T116) is a required
        // field, so it is sent rather than left out. This window does not decide on anyone's behalf
        // which services start along with MixEngine: an apply flagging everything it *creates* is
        // an apply answering a question the user has not asked. That question is asked in exactly
        // one place, the Dashboard's ⋮ menu, on exactly the service people are looking at.
        // `mix blueprint apply --autostart` still sends this flag for anyone who wants it in one
        // command.
        autostart: false,
      });
      if (response.outcome === "planned") {
        setChoices({});
        setScaffoldAgreed(false);
        setPrerequisitesAgreed(false);
        setPhase({ kind: "plan", plan: response.plan, needs: response.needs ?? [] });
      }
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setBusy(false);
    }
  }

  async function apply(plan: BlueprintPlan, installPrerequisites: boolean) {
    setBusy(true);
    setError("");
    try {
      const scaffoldIndex = scaffoldStepIndex(plan.steps);
      const scaffold =
        scaffoldAgreed && scaffoldIndex >= 0
          ? buildScaffoldConsent(plan, plan.steps[scaffoldIndex])
          : undefined;
      const response = await api.blueprintApply({
        blueprint: blueprint.slug,
        project,
        root,
        // Sent on both passes, for the same reason as `front_end` below: the plan people read must
        // be the plan that runs, so what decides the directory must not change after the dry run.
        root_is_parent: false,
        dry_run: false,
        install_prerequisites: installPrerequisites,
        answers: buildAnswers(plan.steps, choices),
        scaffold: scaffold ?? undefined,
        // Sent on both passes: the plan people read must be the plan that runs, so a flag that
        // changes the plan must not be added after the dry run.
        front_end: withFrontEnd,
        // Sent on both passes, for the same reason: see the dry run above.
        autostart: false,
      });
      if (response.outcome === "started") {
        setPhase({ kind: "running", jobId: response.job.id });
      }
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setBusy(false);
    }
  }

  // Follow the job while it runs — subscribe exactly once, unsubscribe on leaving the "running"
  // phase. Through `subscribeDaemonWatch` rather than calling `api.watch()` directly: that channel
  // is shared by the whole app (see `daemonWatch.ts`) — Dashboard/Sites/Packages may well be open
  // alongside this dialog, and a separate `api.watch()`/`api.unwatch()` here would steal or close
  // their channel.
  useEffect(() => {
    if (phase.kind !== "running") return;
    return subscribeDaemonWatch((raw) => setJobs((current) => applyJob(current, raw)));
  }, [phase.kind]);

  // The job has left JobRow[] (job_finished) — read the full result again through job.status.
  useEffect(() => {
    if (phase.kind !== "running") return;
    if (jobFor(jobs, phase.jobId) !== undefined) return;
    let live = true;
    api
      .jobStatus(phase.jobId)
      .then((job) => {
        if (!live) return;
        const applied = blueprintAppliedFrom(job);
        const failure = jobFailureMessage(job);
        if (applied) setPhase({ kind: "done", applied });
        else if (failure !== null) setPhase({ kind: "failed", message: failure });
      })
      .catch((e: unknown) => setError(errorMessage(t, e)));
    return () => {
      live = false;
    };
  }, [phase, jobs, t]);

  // Open the stream **as soon as the apply starts running**, without waiting for the button — T120,
  // D6. The stream used to open only when `showLog` was on, so every line printed before the click
  // was lost for good: an apply that failed at the second step and only then got "Show output"
  // clicked showed a blank panel. The button now only toggles **display**, while collection runs
  // throughout the "running" phase — and `logEntries` stays after the phase changes, so it can
  // still be read once the apply has failed.
  useEffect(() => {
    if (phase.kind !== "running") return;
    const job = phase.jobId;
    setLogEntries([]);
    api
      .jobLogsWatch(job, 200, true, (raw) => {
        setLogEntries((current) => applyLogFrame(current, raw, 2000));
      })
      .catch((e: unknown) => setError(errorMessage(t, e)));
    return () => {
      void api.jobLogsUnwatch();
    };
  }, [phase, t]);

  const running = phase.kind === "running" ? jobFor(jobs, phase.jobId) : undefined;

  // The buttons for the phase the dialog is in: a question has Cancel and its answer; a finished
  // apply has only Close, which is the answer — the caller's start-up chain hangs on `onDone`.
  const actions: ModalAction[] = [];
  if (phase.kind === "form" || phase.kind === "plan") {
    actions.push({ kind: "cancel", label: t("common.cancel"), disabled: busy });
  }
  if (phase.kind === "form") {
    actions.push({
      kind: "confirm",
      label: t("mixengine.blueprints.apply.preview"),
      onClick: () => void preview(),
      disabled: project.trim() === "" || root.trim() === "",
      busy: busy ? t("mixengine.blueprints.apply.previewing") : undefined,
    });
  }
  if (phase.kind === "plan") {
    // The button says exactly what it is about to do. A generic "Apply" on a plan with an init
    // command not yet agreed to is a button promising to build a project and then building an empty
    // directory — so while the checkbox is empty, the label changes outright rather than just
    // adding a caption above.
    const plan = phase.plan;
    actions.push({
      kind: "confirm",
      label:
        scaffoldConsentState(plan.steps, scaffoldAgreed) === "declined"
          ? t("mixengine.blueprints.apply.applyWithoutCommand")
          : t("mixengine.blueprints.apply.applyButton"),
      onClick: () => void apply(plan, prerequisitesAgreed),
      disabled:
        !canApply(plan.steps, choices) || !requirementsAllowApply(phase.needs, prerequisitesAgreed),
      busy: busy ? t("mixengine.blueprints.apply.applying") : undefined,
    });
  }
  if (phase.kind === "done" || phase.kind === "failed") {
    const applied = phase.kind === "done" ? phase.applied : null;
    actions.push({
      kind: "confirm",
      label: t("mixengine.blueprints.apply.close"),
      onClick: () => onDone(applied),
      closes: true,
    });
  }

  return (
    <Modal
      // Escape and clicking outside take the same path as the button below: an apply that has
      // finished is "done" however it is closed, not "cancelled" — the caller still has a whole
      // start-up chain hanging on `onDone`, and losing it to an Escape key loses the site too.
      onClose={() => {
        if (phase.kind === "done") onDone(phase.applied);
        else if (phase.kind === "failed") onDone(null);
        else onCancel();
      }}
      locked={busy}
      title={t("mixengine.blueprints.apply.title", { blueprint: blueprint.name })}
      actions={actions}
    >
      {() => (
        <>
          <ModalBody>
            {phase.kind === "form" && (
              <div className={styles.form}>
                <label className={styles.field}>
                  {t("mixengine.blueprints.apply.project")}
                  <Input value={project} disabled={busy} onChange={(e) => setProject(e.target.value)} />
                </label>
                <label className={styles.field}>
                  {t("mixengine.blueprints.apply.root")}
                  <div className={styles.rootRow}>
                    <Input value={root} disabled={busy} onChange={(e) => setRoot(e.target.value)} />
                    <Button onClick={() => void browseRoot()} disabled={busy}>
                      {t("common.browse")}
                    </Button>
                  </div>
                </label>
              </div>
            )}

            {phase.kind === "plan" && (
              <div className={styles.plan}>
                <h4>{t("mixengine.blueprints.apply.planTitle")}</h4>
                <ul className={styles.steps}>
                  {phase.plan.steps.map((step, i) => (
                    <li key={i} className={styles.step}>
                      <p>{describePlanAction(t, step.action)}</p>
                      {step.disposition.disposition === "blocked" && (
                        <p className={styles.blocked}>
                          {t("mixengine.blueprints.apply.stepBlocked", {
                            reason: step.disposition.reason,
                          })}
                        </p>
                      )}
                      {step.disposition.disposition === "unsupported" && (
                        <p className={styles.blocked}>
                          {t("mixengine.blueprints.apply.stepUnsupported", {
                            reason: step.disposition.reason,
                          })}
                        </p>
                      )}
                      {step.disposition.disposition === "choice" && answerSubjectFor(step) && (
                        <div className={styles.choice}>
                          <p>
                            {t("mixengine.blueprints.apply.choiceInstalled", {
                              installed: step.disposition.installed,
                            })}{" "}
                            —{" "}
                            {t("mixengine.blueprints.apply.choiceWanted", {
                              wanted: step.disposition.wanted,
                            })}
                          </p>
                          <Button
                            variant={choices[i] === "install" ? "primary" : "default"}
                            onClick={() => setChoices((c) => ({ ...c, [i]: "install" }))}
                          >
                            {t("mixengine.blueprints.apply.choiceInstall")}
                          </Button>
                          <Button
                            variant={choices[i] === "use_installed" ? "primary" : "default"}
                            onClick={() => setChoices((c) => ({ ...c, [i]: "use_installed" }))}
                          >
                            {t("mixengine.blueprints.apply.choiceUseInstalled")}
                          </Button>
                        </div>
                      )}
                      {step.action.action === "run_scaffold" && (
                        <div className={styles.scaffold}>
                          <p>{t("mixengine.blueprints.apply.scaffoldTitle")}</p>
                          <code>{step.action.command}</code>
                          {!phase.plan.trusted && (
                            <p className={styles.blocked}>
                              {t("mixengine.blueprints.apply.scaffoldUntrusted")}
                            </p>
                          )}
                          <Checkbox
                            className={styles.checkbox}
                            label={t("mixengine.blueprints.apply.scaffoldConsent")}
                            checked={scaffoldAgreed}
                            onChange={(e) => setScaffoldAgreed(e.target.checked)}
                          />
                          {/* `mix`'s `[y/N]` question, drawn as an interface. Not ticking is an
                              answer — the apply still installs the runtime, DB, site and domain,
                              only the project directory stays empty — and until T121 the desktop
                              said so nowhere: the user found out on the "Done" screen, lost among
                              ten other lines. */}
                          {!scaffoldAgreed && (
                            <p className={styles.consentWarning} role="status">
                              {t("mixengine.blueprints.apply.scaffoldDeclined")}
                            </p>
                          )}
                        </div>
                      )}
                    </li>
                  ))}
                </ul>
                <PrerequisitesNotice
                  needs={phase.needs}
                  agreed={prerequisitesAgreed}
                  onAgree={setPrerequisitesAgreed}
                />
              </div>
            )}

            {phase.kind === "running" && (
              <div className={styles.running}>
                <p>{t("mixengine.blueprints.apply.running")}</p>
                <progress value={running?.percent ?? 0} max={100} />
                <p>{running?.message ?? ""}</p>
              </div>
            )}

            {/* Outlives the "running" phase — T120, D6. A failed apply is when the output is most
                worth reading, and the button used to sit in the block above, so it vanished
                exactly then. */}
            {(phase.kind === "running" || logEntries.length > 0) && (
              <div className={styles.output}>
                <Button onClick={() => setShowLog((v) => !v)}>
                  {showLog
                    ? t("mixengine.blueprints.apply.hideLog")
                    : t("mixengine.blueprints.apply.viewLog")}
                </Button>
                {showLog && (
                  <div className={styles.log} {...logPane}>
                    {logEntries.map((entry, i) =>
                      entry.kind === "gap" ? (
                        <div key={i} className={styles.gap}>
                          {t("mixengine.logs.gap", { count: entry.missed })}
                        </div>
                      ) : (
                        <div key={i} className={styles.logLine}>
                          {entry.text}
                        </div>
                      ),
                    )}
                  </div>
                )}
              </div>
            )}

            {phase.kind === "done" && (
              <div className={styles.done}>
                <h4>{t("mixengine.blueprints.apply.doneTitle")}</h4>

                {/* At the **top** of the list, not the tenth line. And in the app's own words: the
                    `why` the daemon returns ends with a hint to run
                    `mix blueprint apply --run-scaffold`, a command-line flag that means nothing to
                    someone who has just clicked through four screens. */}
                {scaffoldLeftCommand(phase.applied) !== null && (
                  <div className={styles.leftUnrun} role="alert">
                    <p>{t("mixengine.blueprints.apply.leftUnrunTitle")}</p>
                    <code>{scaffoldLeftCommand(phase.applied)}</code>
                    <p>
                      {t("mixengine.blueprints.apply.leftUnrunHow", { root: phase.applied.root })}
                    </p>
                  </div>
                )}

                <ul className={styles.steps}>
                  {phase.applied.steps.map((outcome, i) => (
                    <li key={i} className={styles.step}>
                      <p>{describePlanAction(t, outcome.action)}</p>
                      <p>
                        {outcome.result.result === "done" && t("mixengine.blueprints.apply.stepDone")}
                        {outcome.result.result === "already_true" &&
                          t("mixengine.blueprints.apply.stepAlreadyTrue")}
                        {outcome.result.result === "not_run" &&
                          t("mixengine.blueprints.apply.stepNotRun", { why: outcome.result.why })}
                        {outcome.result.result === "failed" &&
                          t("mixengine.blueprints.apply.stepFailed", { why: outcome.result.why })}
                      </p>
                    </li>
                  ))}
                </ul>
              </div>
            )}
          </ModalBody>
          <ModalErrors messages={[phase.kind === "failed" ? phase.message : "", error]} />
        </>
      )}
    </Modal>
  );
}

/**
 * What the plan's releases lack on this machine — T152. One block for the whole plan, because one
 * approval covers every step that needs the same runtime.
 */
function PrerequisitesNotice({
  needs,
  agreed,
  onAgree,
}: {
  needs: readonly Requirement[];
  agreed: boolean;
  onAgree: (agreed: boolean) => void;
}) {
  const { t } = useTranslation();
  const step = requirementStep(needs);
  if (step.kind === "proceed") return null;

  // **Said, not asked** — T27e: the apply is not held up by a library the distribution provides.
  if (step.kind === "notice") {
    return (
      <NoticeBanner
        message={t("mixengine.requirements.applyLibraries", {
          libraries: splitLibraries(step.needs).libraries.join(", "),
        })}
      />
    );
  }

  const labels = step.needs.map(needLabel).join(", ");
  if (step.kind === "choose") {
    return (
      <p className={styles.blocked} role="alert">
        {t("mixengine.requirements.applyBlocked", { needs: labels })}
      </p>
    );
  }

  return (
    <div className={styles.scaffold}>
      <Checkbox
        className={styles.checkbox}
        label={t("mixengine.requirements.applyConsent", { arch: step.arches.join(", ") })}
        checked={agreed}
        onChange={(e) => onAgree(e.target.checked)}
      />
    </div>
  );
}
