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
  jobWasCancelled,
  scaffoldConsentState,
  scaffoldLeftCommand,
  scaffoldStepIndex,
  dotenvKeys,
  dotenvLeftKeys,
} from "../../blueprintPlan";
import { applyJob, type JobRow } from "../../daemonState";
import { subscribeDaemonWatch } from "../../daemonWatch";
import { applyLogFrame, type LogEntry } from "../../logState";
import { jobFor } from "../../runtimeState";
import { installDevkit } from "../../devkitInstall";
import { planDevkitNeed } from "../../devkitNeed";
import { formatBytes } from "../../metricsState";
import { DEVKIT_PACKAGE, devkitOffer } from "../Packages/devkit";
import type { PackageRelease } from "@mixengine/api";
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
}: Props) {
  const { t } = useTranslation();
  const [project, setProject] = useState(initialProject);
  const [root, setRoot] = useState(initialRoot);
  const [phase, setPhase] = useState<Phase>({ kind: "form" });
  const [choices, setChoices] = useState<Record<number, MismatchAnswer>>({});
  const [scaffoldAgreed, setScaffoldAgreed] = useState(false);
  // T205a: one box per `.env` key the plan offers, keyed by the key.
  const [dotenvAgreed, setDotenvAgreed] = useState<Record<string, boolean>>({});
  // Agreement to install what the plan's releases lack on this machine — T152.
  const [prerequisitesAgreed, setPrerequisitesAgreed] = useState(false);
  /* A Ruby this blueprint pins that cannot build gems with C extensions, and the devkit to install
     with the apply — T206a, asked where the decision is made. Ticked by default: the steps after
     the apply need it, and a project made without it is half built. */
  const [devkit, setDevkit] = useState<{ offer: PackageRelease | null } | null>(null);
  const [devkitAgreed, setDevkitAgreed] = useState(true);
  const [busy, setBusy] = useState(false);
  /** Cancel was pressed on the running apply. Kept past the ending: an apply cancelled between two
   *  steps stops there and still **succeeds** with the steps it ran (the daemon's apply loop), so
   *  the done view is the only place left to say the list is short because somebody asked. */
  const [cancelAsked, setCancelAsked] = useState(false);
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
        // **Always on** — T205, D7. MixLab's question is always *give me a site that works*, and a
        // home that already has a front end is left as it is by the daemon.
        front_end: true,
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
        // Ticked for a blueprint someone vouches for, which is what the person picked it for; left
        // for them to tick on one nobody vouches for — T205.
        setScaffoldAgreed(response.plan.trusted);
        // Ticked for a signed blueprint, as the scaffold's box is; empty for one nobody vouches
        // for, which could name a key the project reads for something else (T205a).
        setDotenvAgreed(
          Object.fromEntries(
            dotenvKeys(response.plan.steps).map((key) => [key, response.plan.trusted]),
          ),
        );
        setPrerequisitesAgreed(false);
        setPhase({ kind: "plan", plan: response.plan, needs: response.needs ?? [] });
        setDevkit(null);
        setDevkitAgreed(true);
        // Beside the plan and never at its expense: a listing that fails costs the offer.
        void Promise.all([api.runtimesAvailable("ruby"), api.packagesAvailable(DEVKIT_PACKAGE)])
          .then(([runtimes, packages]) =>
            setDevkit(planDevkitNeed(response.plan, runtimes.runtimes, devkitOffer(packages))),
          )
          .catch(() => setDevkit(null));
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
        dotenv: dotenvKeys(plan.steps).filter((key) => dotenvAgreed[key]),
        // Sent on both passes: the plan people read must be the plan that runs, so a flag that
        // changes the plan must not be added after the dry run.
        front_end: true,
        // Sent on both passes, for the same reason: see the dry run above.
        autostart: false,
      });
      if (response.outcome === "started") {
        setPhase({ kind: "running", jobId: response.job.id });
        // The devkit installs beside the apply, as its own job: nothing in the apply waits on it.
        if (devkitAgreed && devkit?.offer) {
          const offer = devkit.offer;
          installDevkit(offer).catch((e: unknown) => setError(errorMessage(t, e)));
        }
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
        // A step that gave up because of the cancellation ends the job as `cancelled`, which is
        // neither of the two above: without this the dialog would stay on "Running…" for good.
        else if (jobWasCancelled(job))
          setPhase({ kind: "failed", message: t("mixengine.blueprints.apply.cancelled") });
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
      demo: "apply-preview",
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
      demo: "apply-run",
      onClick: () => void apply(plan, prerequisitesAgreed),
      disabled:
        !canApply(plan.steps, choices) || !requirementsAllowApply(phase.needs, prerequisitesAgreed),
      busy: busy ? t("mixengine.blueprints.apply.applying") : undefined,
    });
  }
  if (phase.kind === "running") {
    const jobId = phase.jobId;
    actions.push({
      kind: "secondary",
      label: t("mixengine.blueprints.apply.cancelApply"),
      disabled: cancelAsked,
      busy: cancelAsked ? t("mixengine.blueprints.apply.cancelling") : undefined,
      onClick: () => {
        setCancelAsked(true);
        api.jobCancel(jobId).catch((e: unknown) => {
          setCancelAsked(false);
          setError(errorMessage(t, e));
        });
      },
    });
  }
  if (phase.kind === "done" || phase.kind === "failed") {
    const applied = phase.kind === "done" ? phase.applied : null;
    actions.push({
      kind: "confirm",
      label: t("mixengine.blueprints.apply.close"),
      demo: "apply-close",
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
                      {step.action.action === "fetch_archive" &&
                        step.disposition.disposition === "satisfied" && (
                          <p className={styles.hint}>
                            {t("mixengine.blueprints.apply.archiveSkipped")}
                          </p>
                        )}
                      {step.action.action === "write_dotenv" &&
                        step.disposition.disposition === "confirm" && (
                          <Checkbox
                            className={styles.checkbox}
                            label={t("mixengine.blueprints.apply.dotenvConsent", {
                              key: step.action.key,
                            })}
                            checked={dotenvAgreed[step.action.key] ?? false}
                            onChange={(e) => {
                              const key = step.action.action === "write_dotenv" ? step.action.key : "";
                              const checked = e.target.checked;
                              setDotenvAgreed((agreed) => ({ ...agreed, [key]: checked }));
                            }}
                          />
                        )}
                      {(step.action.action === "run_scaffold" ||
                        step.action.action === "fetch_archive") &&
                        i === scaffoldStepIndex(phase.plan.steps) && (
                        <div className={styles.scaffold}>
                          {step.action.action === "run_scaffold" ? (
                            <>
                              <p>{t("mixengine.blueprints.apply.scaffoldTitle")}</p>
                              <code>{step.action.command}</code>
                            </>
                          ) : (
                            <>
                              <p>{t("mixengine.blueprints.apply.archiveTitle")}</p>
                              <code>{step.action.url}</code>
                            </>
                          )}
                          {!phase.plan.trusted && (
                            <p className={styles.blocked}>
                              {t("mixengine.blueprints.apply.scaffoldUntrusted")}
                            </p>
                          )}
                          <Checkbox
                            className={styles.checkbox}
                            label={t("mixengine.blueprints.apply.scaffoldConsent")}
                            checked={scaffoldAgreed}
                            data-demo="apply-scaffold"
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
                {devkit?.offer && (
                  <div className={styles.scaffold}>
                    <p>{t("mixengine.blueprints.apply.devkitTitle")}</p>
                    <Checkbox
                      className={styles.checkbox}
                      label={t("mixengine.blueprints.apply.devkitConsent", {
                        size: formatBytes(devkit.offer.bytes),
                      })}
                      checked={devkitAgreed}
                      onChange={(e) => setDevkitAgreed(e.target.checked)}
                    />
                  </div>
                )}
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
                {cancelAsked && <NoticeBanner message={t("mixengine.blueprints.apply.cancelNote")} />}
              </div>
            )}

            {/* Outlives the "running" phase — T120, D6. A failed apply is when the output is most
                worth reading, and the button used to sit in the block above, so it vanished
                exactly then. */}
            {(phase.kind === "running" || logEntries.length > 0) && (
              <div className={styles.output}>
                <Button data-demo="apply-log" onClick={() => setShowLog((v) => !v)}>
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

                {cancelAsked && <NoticeBanner message={t("mixengine.blueprints.apply.cancelled")} />}

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

                {/* T205a: a `.env` key left unwritten, in MixLab's words; the daemon's `why`
                    names a `mix` flag. */}
                {dotenvLeftKeys(phase.applied).map((key) => (
                  <p key={key} className={styles.leftUnrun} role="status">
                    {t("mixengine.blueprints.apply.dotenvLeft", { key })}
                  </p>
                ))}

                <ul className={styles.steps}>
                  {phase.applied.steps.map((outcome, i) => (
                    <li key={i} className={styles.step}>
                      <p>{describePlanAction(t, outcome.action)}</p>
                      <p>
                        {outcome.result.result === "done" && t("mixengine.blueprints.apply.stepDone")}
                        {outcome.result.result === "already_true" &&
                          t("mixengine.blueprints.apply.stepAlreadyTrue")}
                        {outcome.result.result === "not_run" &&
                          t("mixengine.blueprints.apply.stepNotRun", {
                            why:
                              outcome.action.action === "write_dotenv"
                                ? t("mixengine.blueprints.apply.dotenvLeft", {
                                    key: outcome.action.key,
                                  })
                                : outcome.result.why,
                          })}
                        {outcome.result.result === "failed" &&
                          t("mixengine.blueprints.apply.stepFailed", { why: outcome.result.why })}
                      </p>
                      {/* T202, D2: what differed from the plan — the account the project ended up
                          with, a database that was already there — in the daemon's own sentence,
                          under the step it is about. */}
                      {outcome.result.result === "done" && outcome.result.note != null && (
                        <p className={styles.note}>{outcome.result.note}</p>
                      )}
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
