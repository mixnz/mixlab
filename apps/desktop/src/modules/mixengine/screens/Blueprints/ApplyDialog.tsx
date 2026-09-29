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
   * Hộp thoại đã đóng sau một lượt apply. `applied` là kết quả khi nó thành công, `null` khi
   * thất bại — người gọi cần nó để quyết định đoạn tiếp theo (`AfterApply`), và nhất là để biết
   * lệnh khởi tạo có chạy hay không.
   */
  onDone: (applied: BlueprintApplied | null) => void;

  /** Điền sẵn tên project. Quick Start đã hỏi rồi, người dùng không phải gõ lại — T117. */
  initialProject?: string;

  /** Điền sẵn thư mục, cùng lý do. */
  initialRoot?: string;

  /**
   * Cài luôn web server nếu home này chưa có — `BlueprintApply.front_end`, T115.
   *
   * Mặc định **tắt**: một apply là chuyện của một project, còn dựng sẵn cái máy nó chạy trên là
   * chuyện rộng hơn và phải được hỏi. Quick Start là chỗ duy nhất bật nó, vì câu của nó đúng là
   * *cho tôi một site chạy được*.
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
 * Một method (`blueprint.apply`), gọi hai lượt. Lượt 1 (`dry_run: true`) chỉ đọc; lượt 2
 * (`dry_run: false`) là lượt duy nhất thật sự làm gì, và chỉ gửi được sau khi `canApply` đồng ý.
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
        // **Thư mục người dùng chọn chính là thư mục của project** — T120c, D1. Hai cửa sổ hiểu
        // "chọn thư mục" giống hệt nhau; chỗ duy nhất còn ghép đường dẫn là `mix` khi không có
        // `--path`, vì ở đó không ai chọn thư mục nào cả.
        root_is_parent: false,
        dry_run: true,
        front_end: withFrontEnd,
        // **Luôn tắt, và không có prop nào bật nó** — `BlueprintApply.autostart` (T116) là một
        // trường bắt buộc, nên nó được gửi chứ không được bỏ trống. Cửa sổ này không quyết định hộ
        // ai service nào khởi động cùng MixEngine: một apply đánh cờ lên mọi thứ nó *tạo ra* là
        // một apply trả lời một câu người dùng chưa hỏi. Câu đó được hỏi ở đúng một chỗ, menu ⋮
        // của Dashboard, trên đúng service người ta đang nhìn. `mix blueprint apply --autostart`
        // vẫn gửi được cờ này cho ai muốn nó trong một lệnh.
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
        // Gửi ở cả hai lượt, cùng lý do với `front_end` phía dưới: kế hoạch người ta đọc phải là
        // kế hoạch chạy, nên thứ quyết định thư mục không được đổi sau lượt dry run.
        root_is_parent: false,
        dry_run: false,
        install_prerequisites: installPrerequisites,
        answers: buildAnswers(plan.steps, choices),
        scaffold: scaffold ?? undefined,
        // Gửi ở cả hai lượt: kế hoạch người ta đọc phải là kế hoạch chạy, nên một cờ đổi kế hoạch
        // không được thêm vào sau lượt dry run.
        front_end: withFrontEnd,
        // Gửi ở cả hai lượt, cùng lý do: xem lượt dry run phía trên.
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

  // Theo dõi job khi đang chạy — đăng ký đúng một lần, gỡ khi rời phase "running". Qua
  // `subscribeDaemonWatch` chứ không gọi thẳng `api.watch()`: kênh đó dùng chung cho cả app (xem
  // `daemonWatch.ts`) — Dashboard/Sites/Packages rất có thể đang mở cùng lúc dialog này, và một
  // `api.watch()`/`api.unwatch()` riêng ở đây sẽ giành mất hoặc đóng luôn kênh của chúng.
  useEffect(() => {
    if (phase.kind !== "running") return;
    return subscribeDaemonWatch((raw) => setJobs((current) => applyJob(current, raw)));
  }, [phase.kind]);

  // Job đã biến khỏi JobRow[] (job_finished) — đọc lại kết quả đầy đủ qua job.status.
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

  // Mở stream **ngay khi apply bắt đầu chạy**, không đợi bấm nút — T120, D6. Trước đây stream chỉ
  // mở khi `showLog` bật, nên mọi dòng in ra trước cú bấm là mất hẳn: một apply hỏng ở bước thứ hai
  // rồi mới được bấm "Xem output" hiển thị một panel trắng. Nút giờ chỉ bật/tắt **hiển thị**, còn
  // việc thu thập thì chạy suốt phase "running" — và `logEntries` ở lại sau khi phase đổi, nên đọc
  // được cả khi apply đã hỏng.
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
    // Nút nói đúng việc nó sắp làm. Một "Apply" chung chung trên một plan có lệnh khởi tạo chưa
    // được đồng ý là một nút hứa dựng project rồi dựng ra thư mục rỗng — nên khi ô tick còn trống,
    // nhãn đổi hẳn chứ không chỉ thêm một dòng chú thích ở trên.
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
      // Escape và cú bấm ra ngoài đi cùng đường với nút ở dưới: một apply đã chạy xong thì đóng
      // bằng cách nào cũng là "xong", không phải "huỷ" — người gọi còn cả một chuỗi khởi động
      // treo trên `onDone`, và đánh mất nó vì một phím Escape là đánh mất luôn cái site.
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
                          {/* Câu `[y/N]` của `mix`, vẽ ra thành giao diện. Không tick là một câu trả
                              lời — apply vẫn cài runtime, DB, site và domain, chỉ là thư mục
                              project ở lại rỗng — và cho tới T121 thì desktop không nói câu ấy ở
                              đâu cả: người dùng biết được ở màn "Xong", lẫn giữa mười dòng khác. */}
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

            {/* Sống lâu hơn phase "running" — T120, D6. Một apply hỏng là lúc output đáng đọc nhất,
                mà trước đây nút nằm trong khối trên nên biến mất đúng lúc đó. */}
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

                {/* Ở **đầu** danh sách, không phải dòng thứ mười. Và bằng câu của chính app: `why`
                    daemon trả về kết bằng một gợi ý `mix blueprint apply --run-scaffold`, một cờ
                    dòng lệnh vô nghĩa với người vừa bấm chuột qua bốn màn hình. */}
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
