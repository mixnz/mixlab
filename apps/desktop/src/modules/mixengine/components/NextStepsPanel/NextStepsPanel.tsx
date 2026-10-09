import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";

import Button from "../../../../components/Button";
import NoticeBanner from "../../../../components/NoticeBanner";
import StatusPill from "../../../../components/StatusPill";
import { copyText } from "../../../../core/clipboard";
import { callModuleAction } from "../../../../core/moduleActions";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { AppliedDatabase, NextStep, NextSteps, PackageRelease } from "@mixengine/api";
import { subscribeDaemonWatch } from "../../daemonWatch";
import { installDevkit, useDevkitInstall } from "../../devkitInstall";
import { jobFinished } from "../../runtimeState";
import { devkitNeed } from "../../devkitNeed";
import { formatBytes } from "../../metricsState";
import { DEVKIT_PACKAGE, devkitOffer } from "../../screens/Packages/devkit";
import {
  oneShotState,
  openAddress,
  requiredRuns,
  targetToSave,
  TERMINAL_MODULE_ID,
  type Toolchain,
} from "../../nextSteps";
import { addressFor, stepsBySite, type SiteAddresses } from "../../siteGroups";
import styles from "./NextStepsPanel.module.css";

interface Props {
  project: string;
  root: string;
  /** Where `open` rows go: the step's own site when it names one, else the first — T204a. */
  addresses: SiteAddresses;
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
  addresses,
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
  /** The database service's port, read from `service.list`: a framework's settings need it, and
   *  `BlueprintApplied.database` does not carry it. */
  const [port, setPort] = useState<number | null>(null);
  /** The `serve` lines saved as Terminal targets while this panel has been up. */
  const [saved, setSaved] = useState<ReadonlySet<string>>(new Set());
  const [error, setError] = useState("");
  /** A Ruby here that cannot build gems with C extensions, and the devkit that fixes it — `null`
   *  when nothing stands in the way (`devkitNeed.ts`). Read again when it has been installed. */
  const [devkit, setDevkit] = useState<{ offer: PackageRelease | null } | null>(null);
  /** The devkit's install job, while it runs — wherever it was started (`devkitInstall.ts`). */
  const installing = useDevkitInstall();
  const installingId = installing?.id ?? null;
  /** Bumped whenever a job ends, so the devkit is read again whoever installed it — the Languages
   *  tab, `mix package install`, or this window before a reload forgot it. */
  const [jobsEnded, setJobsEnded] = useState(0);

  useEffect(
    () =>
      subscribeDaemonWatch((raw) => {
        if (jobFinished(raw) !== null) setJobsEnded((n) => n + 1);
      }),
    [],
  );

  useEffect(() => {
    let live = true;
    Promise.all([api.projectShow(project), api.runtimesAvailable("ruby"), api.packagesAvailable(DEVKIT_PACKAGE)])
      .then(([shown, runtimes, packages]) => {
        if (live) setDevkit(devkitNeed(shown.pins, runtimes.runtimes, devkitOffer(packages)));
      })
      // Not knowing costs the warning, never the panel.
      .catch(() => live && setDevkit(null));
    return () => {
      live = false;
    };
    // Read again when an install ends, wherever it was started.
  }, [project, installingId, jobsEnded]);

  async function startDevkit(release: PackageRelease) {
    setError("");
    try {
      await installDevkit(release);
    } catch (e) {
      // Installed somewhere this window did not see: read again rather than report it.
      setJobsEnded((n) => n + 1);
      setError(errorMessage(t, e));
    }
  }

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

  const service = database?.service ?? null;
  useEffect(() => {
    if (service === null) return;
    let live = true;
    api
      .services()
      .then((list) => {
        const row = list.services.find((candidate) => candidate.id === service);
        if (live) setPort(row?.port ?? null);
      })
      // Without it the block still names the host, the database and the account.
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [service]);

  const required = requiredRuns(steps.steps);
  const groups = stepsBySite(steps.steps);
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

  /** Saves the line as a Terminal target, here and now — no tab is opened over this panel. */
  async function save(run: string) {
    if (toolchain === null) return;
    setError("");
    try {
      await callModuleAction(TERMINAL_MODULE_ID, "saveTarget", targetToSave(project, root, run, toolchain));
      setSaved((current) => new Set(current).add(run));
    } catch (e) {
      setError(errorMessage(t, e));
    }
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
      const base = addressFor(addresses, step);
      if (base === null) return null;
      const address = openAddress(base, [step]);
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
            // Without the devkit a step that builds gems leaves the project half built; every step
            // here runs on that Ruby, so none runs until it is installed. Copy still works.
            disabled={toolchain === null || devkit !== null}
            title={devkit !== null ? t("mixengine.nextSteps.devkitFirst") : undefined}
            onClick={() =>
              toolchain && void hand([oneShotState([run], root, toolchain, steps.trusted)])
            }
          >
            {terminalVisible ? t("mixengine.nextSteps.run") : t("mixengine.nextSteps.runEnabling")}
          </Button>
          {step.kind === "serve" && (
            <Button
              size="small"
              disabled={toolchain === null || saved.has(run)}
              onClick={() => void save(run)}
            >
              {saved.has(run) ? t("mixengine.nextSteps.saved") : t("mixengine.nextSteps.save")}
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
            <Button
              variant="primary"
              size="small"
              // Run against a Ruby that cannot build them, the steps leave a broken project behind.
              disabled={toolchain === null || devkit !== null}
              title={devkit !== null ? t("mixengine.nextSteps.devkitFirst") : undefined}
              onClick={runRequired}
            >
              {terminalVisible
                ? t("mixengine.nextSteps.runRequired")
                : t("mixengine.nextSteps.runRequiredEnabling")}
            </Button>
          )}
        </div>
      )}

      {devkit !== null && (
        <div className={styles.devkit} role="alert">
          <p>{t("mixengine.nextSteps.devkitMissing")}</p>
          {devkit.offer !== null &&
            (installing !== null ? (
              <span className={styles.devkitProgress}>
                <progress value={installing.percent} max={100} />
                <span>
                  {t("mixengine.nextSteps.devkitInstalling")} {installing.percent}%
                  {installing.message !== "" && ` · ${installing.message}`}
                </span>
              </span>
            ) : (
              <Button size="small" variant="primary" onClick={() => devkit.offer && void startDevkit(devkit.offer)}>
                {t("mixengine.packages.lacks.installDevkit", { size: formatBytes(devkit.offer.bytes) })}
              </Button>
            ))}
        </div>
      )}

      {!steps.trusted && <NoticeBanner message={t("mixengine.nextSteps.untrusted")} />}

      {/* T204a, D5: with several sites, each site's steps sit under its name. Run the required
          steps stays over the whole list, since a project's tabs are one project's work. */}
      {groups.length > 1 ? (
        groups.map((group) => (
          <div key={group.site ?? ""} className={styles.siteGroup}>
            <p className={styles.siteName}>{t("mixengine.nextSteps.forSite", { domain: group.site ?? "" })}</p>
            <ul className={styles.steps}>{group.steps.map(row)}</ul>
          </div>
        ))
      ) : (
        <ul className={styles.steps}>{steps.steps.map(row)}</ul>
      )}

      {database && wantsCredentials && (
        <div className={styles.credentials}>
          <p>{t("mixengine.nextSteps.credentialsTitle")}</p>
          <dl>
            {(
              [
                ["host", "127.0.0.1"],
                ...(port === null ? [] : [["port", String(port)] as const]),
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
