import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";

import Button from "../../../../components/Button";
import NoticeBanner from "../../../../components/NoticeBanner";
import StatusPill from "../../../../components/StatusPill";
import { copyText } from "../../../../core/clipboard";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { AppliedDatabase, NextStep, NextSteps } from "@mixengine/api";
import {
  draftState,
  oneShotState,
  openAddress,
  requiredRuns,
  type Toolchain,
} from "../../nextSteps";
import styles from "./NextStepsPanel.module.css";

interface Props {
  project: string;
  root: string;
  /** The site's address (`siteUrl` from `siteState.ts`), for `open` rows. */
  siteUrl: string | null;
  steps: NextSteps;
  /** Present only right after an apply (D8): the credentials block is drawn from it. */
  database?: AppliedDatabase | null;
  /** Whether this window draws the Terminal module. Hidden, the buttons say they turn it on: the
   *  shell turns a module on for a tab that names it (`Workspace.tsx`, T110). */
  terminalVisible: boolean;
  /** A step with `credentials`, outside `AfterApply`, links here (D8). */
  onShowDatabase?: () => void;
  /** Draws its own heading. Off inside a `Card`, whose title already says it. */
  titled?: boolean;
}

/**
 * What a blueprint says is left to do once it has been applied — roadmap task **T205**, D8.
 *
 * **Guidance, not supervision.** The daemon runs none of this: each command goes to a Terminal tab
 * (`api.openTerminal`, a one-shot run or a drafted target), where the person sees it run and stops
 * it. A step from a blueprint nobody vouches for is typed and left for their Enter.
 */
export default function NextStepsPanel({
  project,
  root,
  siteUrl,
  steps,
  database,
  terminalVisible,
  onShowDatabase,
  titled = true,
}: Props) {
  const { t } = useTranslation();
  // Where the project's runtimes are, for the shell a step runs in (D9). `null` while it is read.
  const [toolchain, setToolchain] = useState<Toolchain | null>(null);
  const [password, setPassword] = useState<string | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    let live = true;
    Promise.all([api.pathStatus(), api.status()])
      .then(([path, status]) => {
        if (live) setToolchain({ bin: path.directory, home: status.home });
      })
      .catch((e: unknown) => live && setError(errorMessage(t, e)));
    return () => {
      live = false;
    };
  }, [t]);

  const required = requiredRuns(steps.steps);
  const wantsCredentials = steps.steps.some((step) => step.credentials === true);

  async function hand(states: unknown[]) {
    setError("");
    try {
      for (const state of states) await api.openTerminal(state);
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }

  function runRequired() {
    if (toolchain === null) return;
    void hand([
      oneShotState(required.first, root, toolchain, steps.trusted),
      ...required.furtherServes.map((serve) => oneShotState([serve], root, toolchain, steps.trusted)),
    ]);
  }

  async function reveal() {
    if (!database) return;
    setError("");
    try {
      const credentials = await api.databaseCredentials(database.service, database.user);
      setPassword(credentials.password);
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }

  function copy(text: string) {
    void copyText(text).catch((e: unknown) => setError(errorMessage(t, e)));
  }

  function row(step: NextStep, i: number) {
    if (step.kind === "open") {
      if (siteUrl === null) return null;
      const address = openAddress(siteUrl, [step]);
      return (
        <li key={i} className={styles.step}>
          <div className={styles.command}>
            <code>{address}</code>
            {step.note && <p className={styles.note}>{step.note}</p>}
          </div>
          <div className={styles.actions}>
            <Button size="small" onClick={() => void openUrl(address)}>
              {t("mixengine.nextSteps.open")}
            </Button>
          </div>
        </li>
      );
    }

    const run = step.run ?? "";
    if (run === "") return null;
    return (
      <li key={i} className={styles.step}>
        <div className={styles.command}>
          <div className={styles.line}>
            <code>{run}</code>
            {step.optional && <StatusPill tone="neutral">{t("mixengine.nextSteps.optional")}</StatusPill>}
          </div>
          {step.note && <p className={styles.note}>{step.note}</p>}
          {step.credentials && !database && onShowDatabase && (
            <Button size="small" variant="link" onClick={onShowDatabase}>
              {t("mixengine.nextSteps.showDatabase")}
            </Button>
          )}
        </div>
        <div className={styles.actions}>
          <Button size="small" variant="ghost" onClick={() => copy(run)}>
            {t("mixengine.nextSteps.copy")}
          </Button>
          <Button
            size="small"
            disabled={toolchain === null}
            onClick={() =>
              toolchain && void hand([oneShotState([run], root, toolchain, steps.trusted)])
            }
          >
            {terminalVisible ? t("mixengine.nextSteps.run") : t("mixengine.nextSteps.runEnabling")}
          </Button>
          {step.kind === "serve" && (
            <Button
              size="small"
              disabled={toolchain === null}
              onClick={() =>
                toolchain &&
                void hand([draftState(project, root, run, toolchain, steps.trusted)])
              }
            >
              {terminalVisible
                ? t("mixengine.nextSteps.save")
                : t("mixengine.nextSteps.saveEnabling")}
            </Button>
          )}
        </div>
      </li>
    );
  }

  return (
    <section
      className={titled ? `${styles.panel} ${styles.titled}` : styles.panel}
      aria-label={t("mixengine.nextSteps.title")}
    >
      {(titled || required.first.length > 0) && (
        <div className={styles.header}>
          {titled && <h3>{t("mixengine.nextSteps.title")}</h3>}
          {required.first.length > 0 && (
            <Button variant="primary" size="small" disabled={toolchain === null} onClick={runRequired}>
              {terminalVisible
                ? t("mixengine.nextSteps.runRequired")
                : t("mixengine.nextSteps.runRequiredEnabling")}
            </Button>
          )}
        </div>
      )}

      {!steps.trusted && <NoticeBanner message={t("mixengine.nextSteps.untrusted")} />}

      <ul className={styles.steps}>{steps.steps.map(row)}</ul>

      {database && wantsCredentials && (
        <div className={styles.credentials}>
          <p>{t("mixengine.nextSteps.credentialsTitle")}</p>
          <dl>
            {(
              [
                ["host", "127.0.0.1"],
                ["database", database.database],
                ["user", database.user],
              ] as const
            ).map(([label, value]) => (
              <div key={label} className={styles.field}>
                <dt>{t(`mixengine.nextSteps.${label}`)}</dt>
                <dd>
                  <code>{value}</code>
                  <Button size="small" variant="ghost" onClick={() => copy(value)}>
                    {t("mixengine.nextSteps.copy")}
                  </Button>
                </dd>
              </div>
            ))}
            <div className={styles.field}>
              <dt>{t("mixengine.nextSteps.password")}</dt>
              <dd>
                {password === null ? (
                  <Button size="small" onClick={() => void reveal()}>
                    {t("mixengine.nextSteps.revealPassword")}
                  </Button>
                ) : (
                  <>
                    <code>{password}</code>
                    <Button size="small" variant="ghost" onClick={() => copy(password)}>
                      {t("mixengine.nextSteps.copy")}
                    </Button>
                  </>
                )}
              </dd>
            </div>
          </dl>
        </div>
      )}

      {error && (
        <p className={styles.error} role="alert">
          {error}
        </p>
      )}
    </section>
  );
}
