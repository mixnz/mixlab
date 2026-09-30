import { useCallback, useEffect, useState, useSyncExternalStore } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "../../../../i18n";
import type { TranslationKey } from "../../../../i18n";
import { CheckIcon } from "../../../../icons";
import { errorMessage } from "../../../../core/errors";
import {
  subscribeToolInstall,
  toolInstallState,
  toolsInstall,
  toolsSetPath,
  toolsStatus,
  toolsUninstall,
} from "../../tools";
import type { ToolStage, ToolStatus, ToolSuite } from "../../tools";
import styles from "./ToolsSection.module.css";

/** The three suites, what each one is called where it is downloaded from, and what to say instead
 * of a download button where there is nothing to download — which names the packages to install,
 * and so differs per engine. */
const SUITES: {
  suite: ToolSuite;
  labelKey: "tools.mysqlSuite" | "tools.postgresSuite" | "tools.mongoSuite";
  noDownloadKey: "tools.noDownload" | "tools.noDownloadPostgres";
}[] = [
  { suite: "mysql", labelKey: "tools.mysqlSuite", noDownloadKey: "tools.noDownload" },
  {
    suite: "postgres",
    labelKey: "tools.postgresSuite",
    // Names the PostgreSQL packages rather than MySQL's, where there is nothing to fetch.
    noDownloadKey: "tools.noDownloadPostgres",
  },
  { suite: "mongo", labelKey: "tools.mongoSuite", noDownloadKey: "tools.noDownload" },
];

/** What each way of finding a tool is called. Spelled out rather than built from the value, so
 * every key the translations need is one a search for it finds. */
const SOURCE_LABEL = {
  custom: "tools.sourceCustom",
  downloaded: "tools.sourceDownloaded",
  system: "tools.sourceSystem",
} as const;

/** What each stage of an install is called while it is happening. */
const STAGE_LABEL: Record<ToolStage, TranslationKey> = {
  downloading: "tools.stageDownloading",
  verifying: "tools.stageVerifying",
  unpacking: "tools.stageUnpacking",
  installing: "tools.stageInstalling",
};

/** How long the "it worked" line stays up. Long enough to be read on the way back from somewhere
 *  else, short enough that it is gone before it stops meaning "just now". */
const DONE_VISIBLE_MS = 8000;

function megabytes(bytes: number): string {
  return (bytes / 1024 / 1024).toFixed(1);
}

/**
 * The dump and restore tools: where each one was found, and what can be done about it — download
 * MixLab's own copy, delete that copy again, or point the app at one already on this machine.
 *
 * A download is tens of megabytes and takes minutes, so it is not left to a button that says
 * "working": the bar below the suite carries the stage it is in, the megabytes so far, and — once
 * it is over — a line saying so, since the badges changing from "missing" to "downloaded" is a
 * quiet thing to have waited that long for.
 */
function ToolsSection() {
  const { t } = useTranslation();
  const [tools, setTools] = useState<ToolStatus[]>([]);
  /** The install, wherever it was started from — this screen may have been closed and reopened
   *  since, and the download went on without it. */
  const install = useSyncExternalStore(subscribeToolInstall, toolInstallState);
  /** Removing is quick and needs nothing following it, so it stays a local flag. */
  const [removing, setRemoving] = useState<ToolSuite | null>(null);
  const [error, setError] = useState("");
  /** Beats once each time a confirmation below has been up long enough to come down. */
  const [beat, tick] = useState(0);

  const onError = useCallback((message: string) => setError(message), []);

  const refresh = useCallback(() => {
    toolsStatus()
      .then(setTools)
      .catch((e) => onError(errorMessage(t, e)));
  }, [onError]);

  // On the way in, and whenever an install starts or ends — including one this screen was closed
  // for and knows nothing about. Keyed by which suites are running rather than by the map itself,
  // which is replaced with every progress reading and would ask the backend four times a second.
  const runningKey = [...install.running.keys()].sort().join(",");
  useEffect(() => {
    refresh();
  }, [runningKey, refresh]);

  const finished = install.finished;
  /** Whether a suite's "it worked" line is still within its few seconds. */
  function justInstalled(suite: ToolSuite): boolean {
    const at = finished.get(suite);
    return at !== undefined && Date.now() - at < DONE_VISIBLE_MS;
  }

  // One timer for whichever confirmation runs out first; the beat brings the effect back for the
  // next one, so two downloads finishing minutes apart each get their own few seconds.
  useEffect(() => {
    const now = Date.now();
    const remaining = [...finished.values()]
      .map((at) => DONE_VISIBLE_MS - (now - at))
      .filter((left) => left > 0);
    if (remaining.length === 0) return;
    const timer = window.setTimeout(() => tick((n) => n + 1), Math.min(...remaining));
    return () => window.clearTimeout(timer);
  }, [finished, beat]);

  /** Runs one of the buttons' errands, with whatever it goes wrong with put on screen. */
  async function act(work: () => Promise<void>) {
    setError("");
    try {
      await work();
    } catch (e) {
      onError(errorMessage(t, e));
    }
  }

  async function remove(suite: ToolSuite) {
    setRemoving(suite);
    await act(() => toolsUninstall(suite));
    setRemoving(null);
    refresh();
  }

  async function choose(tool: ToolStatus) {
    const path = await open({ multiple: false, directory: false });
    if (typeof path !== "string") return;
    try {
      await toolsSetPath(tool.name, path);
    } catch (e) {
      onError(errorMessage(t, e));
    }
    refresh();
  }

  async function forget(tool: ToolStatus) {
    try {
      await toolsSetPath(tool.name, null);
    } catch (e) {
      onError(errorMessage(t, e));
    }
    refresh();
  }

  /** The bar under a suite that is being fetched: what it is doing, and how far in it is. */
  function installProgress(suite: ToolSuite) {
    const live = install.running.get(suite) ?? null;
    // Until the first event lands there is no stage to name, and no download has begun either —
    // "downloading" is the honest guess for that gap.
    const stage: ToolStage = live?.stage ?? "downloading";
    const measured = live !== null && stage === "downloading" && live.total > 0;
    const percent = measured ? Math.min(100, Math.round((live.done / live.total) * 100)) : 0;

    return (
      /* Deliberately not a live region: it changes four times a second, and the bar below already
         carries the number for anything reading it. What is worth announcing is the end, which the
         line under this one does. */
      <div className={styles.progress}>
        <div className={styles.progressHead}>
          <span>{t(STAGE_LABEL[stage])}</span>
          {live !== null && stage === "downloading" && live.done > 0 && (
            <span className={styles.progressCount}>
              {measured
                ? t("tools.sizeKnown", {
                    done: megabytes(live.done),
                    total: megabytes(live.total),
                    percent,
                  })
                : t("tools.sizeUnknown", { done: megabytes(live.done) })}
            </span>
          )}
        </div>
        <div
          className={styles.progressTrack}
          role="progressbar"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={measured ? percent : undefined}
        >
          {/* Without a total from the server there is no honest percentage, so the bar sweeps
              instead of filling: movement says "still going" without claiming to know how far. */}
          <div
            className={measured ? styles.progressFill : `${styles.progressFill} ${styles.progressSweep}`}
            style={measured ? { width: `${percent}%` } : undefined}
          />
        </div>
      </div>
    );
  }

  return (
    <div className={styles.section}>
      {/* The pane is the database module's and is named after it, so what is in the pane names
          itself — the way the REST module's groups do. Everything here is still the dump tools and
          nothing else; the day something else joins them it goes under a heading of its own. */}
      <span className={styles.sectionLabel}>{t("tools.title")}</span>
      <p className={styles.hint}>{t("tools.intro")}</p>
      {error !== "" && (
        <p className={styles.toolError} role="alert">
          {error}
        </p>
      )}

      {SUITES.map(({ suite, labelKey, noDownloadKey }) => {
        const members = tools.filter((tool) => tool.suite === suite);
        // Only a copy MixLab downloaded can be removed; what was already on the machine is not
        // MixLab's to delete.
        const downloaded = members.some((tool) => tool.source === "downloaded");
        // Only this suite's own errand quiets this suite's buttons: the other one downloads to a
        // staging directory and unpacks to an install directory of its own, so the two never meet.
        const fetching = install.running.has(suite);
        const busy = fetching || removing === suite;
        // Every tool of a suite carries the same answer, and an empty list — the moment before the
        // first status lands — leaves the button where it is rather than having it appear late.
        const downloadable = members.every((tool) => tool.downloadable);
        return (
          <div key={suite} className={styles.toolSuite}>
            <div className={styles.toolSuiteHeader}>
              <span className={styles.toolSuiteName}>{t(labelKey)}</span>
              <div className={styles.toolSuiteActions}>
                {/* No archive for this platform means no download to offer: a button here could
                    only ever answer with an error, so the line below says where the tools come
                    from instead. */}
                {downloadable && (
                  <button
                    type="button"
                    className={styles.toolButton}
                    disabled={busy}
                    onClick={() => void act(() => toolsInstall(suite))}
                  >
                    {busy
                      ? t("tools.working")
                      : t(downloaded ? "tools.redownload" : "tools.download")}
                  </button>
                )}
                {downloaded && (
                  <button
                    type="button"
                    className={`${styles.toolButton} ${styles.toolButtonDanger}`}
                    disabled={busy}
                    onClick={() => void remove(suite)}
                  >
                    {t("tools.remove")}
                  </button>
                )}
              </div>
            </div>

            {!downloadable && <p className={styles.hint}>{t(noDownloadKey)}</p>}

            {fetching && installProgress(suite)}

            {justInstalled(suite) && (
              <p className={styles.toolDone} role="status">
                <CheckIcon size={14} />
                {t("tools.installed")}
              </p>
            )}

            {members.map((tool) => (
              <div key={tool.name} className={styles.tool}>
                <div className={styles.toolText}>
                  <span className={styles.toolName}>
                    {tool.name}
                    <span
                      className={
                        tool.source === null
                          ? `${styles.toolBadge} ${styles.toolBadgeMissing}`
                          : styles.toolBadge
                      }
                    >
                      {tool.source === null ? t("tools.missing") : t(SOURCE_LABEL[tool.source])}
                    </span>
                  </span>
                  <span className={styles.toolPath} title={tool.path ?? ""}>
                    {tool.path ?? t("tools.missingHint")}
                  </span>
                </div>
                <div className={styles.toolSuiteActions}>
                  <button
                    type="button"
                    className={styles.toolButton}
                    onClick={() => void choose(tool)}
                  >
                    {t("tools.choose")}
                  </button>
                  {tool.source === "custom" && (
                    <button
                      type="button"
                      className={styles.toolButton}
                      onClick={() => void forget(tool)}
                    >
                      {t("tools.forget")}
                    </button>
                  )}
                </div>
              </div>
            ))}
          </div>
        );
      })}
    </div>
  );
}

export default ToolsSection;
