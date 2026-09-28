import { openUrl } from "@tauri-apps/plugin-opener";
import Button from "../../../components/Button";
import { errorMessage } from "../../../core/errors";
import { useTranslation } from "../../../i18n";
import type { Updates } from "../../update";
import { RELEASES_PAGE } from "../../version";
import styles from "./UpdatePanel.module.css";

const MB = 1_000_000;

/**
 * *Open installer* on macOS and Linux leaves the next step, *Finish*, in Settings → Updates (T187
 * D5), so Settings opens there once the installer is open. A Windows install relaunches instead,
 * and a step that failed leaves the panel on its failure.
 */
export async function installThenFollow(
  install: () => Promise<boolean>,
  onInstaller: boolean,
  onInstallerOpened: () => void,
): Promise<void> {
  if ((await install()) && onInstaller) onInstallerOpened();
}

interface Props {
  updates: Updates;
  /** The installer is open: show the pane where *Finish* is. */
  onInstallerOpened: () => void;
}

/**
 * A MixLab release, offered in the corner: T188, `docs/specs/2026-09-27-t188-an-update-offered-in-the-corner-design.md`.
 *
 * A corner and not a bar across the top: this arrives while somebody is in the middle of something,
 * and nothing here is urgent enough to move what they are looking at. Every step forward is a click
 * (spec D1, D3). What it draws is `updates.panel`, decided in `shell/update/view.ts`; the pane in
 * Settings → Updates reads the same state.
 */
function UpdatePanel({ updates, onInstallerOpened }: Props) {
  const { t } = useTranslation();
  const { panel, status, progress } = updates;
  const feed = status?.feed ?? null;
  if (panel === "hidden" || !status || !feed) return null;
  const onInstaller = status.placement.kind === "installer";

  const known = progress !== null && progress.total > 0;
  const percent = known ? Math.min(100, (progress.received / progress.total) * 100) : null;

  return (
    <div className={styles.panel} role="status" aria-live="polite">
      {panel === "offer" && (
        <>
          <span className={styles.title}>{t("update.offered", { version: feed.version })}</span>
          {feed.size !== null && (
            <span className={styles.meta}>{t("update.size", { size: Math.max(1, Math.round(feed.size / MB)) })}</span>
          )}
          {/* The feed's notes are the tag's commit subjects (packaging/feed.sh), written for the
              repository rather than for the user, so the panel links to the release page instead. */}
          <Button
            variant="link"
            size="small"
            className={styles.notes}
            onClick={() => void openUrl(feed.notesUrl ?? RELEASES_PAGE)}
          >
            {t("update.notesLink")}
          </Button>
          <div className={styles.actions}>
            <Button variant="primary" size="small" onClick={() => void updates.download()}>
              {t("update.download")}
            </Button>
            <Button size="small" onClick={updates.remindLater}>
              {t("update.later")}
            </Button>
            <Button variant="link" size="small" className={styles.skip} onClick={() => void updates.skip()}>
              {t("update.skip")}
            </Button>
          </div>
        </>
      )}

      {panel === "downloading" && (
        <>
          <span className={styles.title}>
            {t("update.downloading", { percent: percent === null ? 0 : Math.floor(percent) })}
          </span>
          {/* Raw markup rather than a shared component: this is the only progress bar in the shell,
              and a determinate width is all of it. A second one makes it a component. */}
          <div
            className={styles.track}
            role="progressbar"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={percent === null ? undefined : Math.floor(percent)}
          >
            <div
              className={percent === null ? `${styles.bar} ${styles.indeterminate}` : styles.bar}
              style={percent === null ? undefined : { width: `${percent}%` }}
            />
          </div>
          {known && (
            <span className={styles.meta}>
              {t("update.received", {
                received: (progress.received / MB).toFixed(1),
                total: (progress.total / MB).toFixed(1),
              })}
            </span>
          )}
          <div className={styles.actions}>
            <Button size="small" onClick={() => void updates.cancelDownload()}>
              {t("update.cancel")}
            </Button>
          </div>
        </>
      )}

      {panel === "ready" && (
        <>
          <span className={styles.title}>
            {t(onInstaller ? "update.installerDownloaded" : "update.ready", { version: feed.version })}
          </span>
          {!onInstaller && updates.restarts !== null && (
            <span className={styles.meta}>
              {updates.restarts > 0
                ? t("update.readyRestartsServices", { count: updates.restarts })
                : t("update.readyRestartsDaemon")}
            </span>
          )}
          <div className={styles.actions}>
            <Button
              variant="primary"
              size="small"
              onClick={() => void installThenFollow(updates.install, onInstaller, onInstallerOpened)}
            >
              {t(onInstaller ? "update.openInstaller" : "update.installRestart")}
            </Button>
            <Button size="small" onClick={updates.remindLater}>
              {t("update.later")}
            </Button>
          </div>
        </>
      )}

      {panel === "installing" && (
        <>
          <span className={styles.title}>{t("update.installing")}</span>
          {!onInstaller && <span className={styles.meta}>{t("update.daemonRestarts")}</span>}
        </>
      )}

      {panel === "failed" && (
        <>
          <span className={styles.meta}>{errorMessage(t, updates.failure)}</span>
          <div className={styles.actions}>
            <Button variant="primary" size="small" onClick={() => void updates.retry()}>
              {t("update.tryAgain")}
            </Button>
            <Button size="small" onClick={updates.closeFailure}>
              {t("update.close")}
            </Button>
          </div>
        </>
      )}
    </div>
  );
}

export default UpdatePanel;
