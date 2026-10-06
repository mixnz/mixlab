import { Fragment, useCallback, useEffect, useRef, useState, type ReactNode } from "react";

import Button from "../../../../components/Button";
import Card from "../../../../components/Card";
import ConfirmDialog from "../../../../components/ConfirmDialog";
import EmptyState from "../../../../components/EmptyState";
import ErrorBanner from "../../../../components/ErrorBanner";
import LoadingState from "../../../../components/LoadingState";
import { runtimeRowsFrom, type OnDiskRow } from "../../onDisk";
import OnDiskCard from "./OnDiskCard";
import Input from "../../../../components/Input";
import MonogramBadge from "../../../../components/MonogramBadge";
import NoticeBanner from "../../../../components/NoticeBanner";
import Table from "../../../../components/Table";
import { errorMessage } from "../../../../core/errors";
import { ChevronDownIcon } from "../../../../icons";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { PackageVersion, RuntimeKind, RuntimeRelease } from "@mixengine/api";
import type { RuntimeSummary, RuntimeUpdate, UpgradePlan } from "@mixengine/api";
import type { CatalogueGap } from "@mixengine/api";
import RequirementDialog from "../../components/RequirementDialog";
import UpdateRow from "../../components/UpdateRow";
import UpgradeDialog from "../../components/UpgradeDialog";
import {
  askingStep,
  needLabel,
  requirementStep,
  splitLibraries,
  type AskingStep,
} from "../../requirementStep";
import { applyJob, type JobRow } from "../../daemonState";
import { subscribeDaemonWatch } from "../../daemonWatch";
import { afterRefusal } from "../../forceStep";
import { takePendingLanguageFilter } from "../../packagesNavigation";
import {
  formatInstalledAt,
  jobFinished,
  jobFor,
  newestFirst,
  versionKey,
} from "../../runtimeState";
import StaleBadge from "../../components/StaleBadge";
import UnavailableNote from "../../components/UnavailableNote";
import { matchesAvailable } from "./availableFilter";
import { groupByLine } from "./availableLines";
import { updateRowState } from "../../updateRow";
import ExtensionsPanel from "./ExtensionsPanel";
import styles from "./Catalogue.module.css";

export default function Languages({ active }: { active: boolean }) {
  const [installed, setInstalled] = useState<RuntimeSummary[]>([]);
  const [available, setAvailable] = useState<RuntimeRelease[]>([]);
  /** False until the first read has answered — until then both lists being empty means "not known
   *  yet", not "nothing installed". */
  const [loaded, setLoaded] = useState(false);
  const [stale, setStale] = useState(false);
  const [unavailable, setUnavailable] = useState<CatalogueGap[]>([]);
  const [jobs, setJobs] = useState<JobRow[]>([]);
  const [installingJob, setInstallingJob] = useState<Record<string, number>>({});
  const [uninstallTarget, setUninstallTarget] = useState<RuntimeSummary | null>(null);
  const [forceHint, setForceHint] = useState<string | null>(null);
  // The one question an install is waiting on, with the release it is about — T151.
  const [asking, setAsking] = useState<{ release: RuntimeRelease; step: AskingStep } | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);
  // Filters the "not installed" table only — the same reason `Packages.tsx` has.
  const [filter, setFilter] = useState("");
  // Lines the person opened to see their older releases — T193a.
  const [openLines, setOpenLines] = useState<Set<string>>(new Set());
  const [error, setError] = useState("");
  const [onDisk, setOnDisk] = useState<OnDiskRow[]>([]);
  const [adopting, setAdopting] = useState<string | null>(null);
  // What an install said about this machine and went on anyway — T27e.
  const [notice, setNotice] = useState("");
  // T193b: what the index has newer in each installed version's line, and the update being asked.
  const [updates, setUpdates] = useState<RuntimeUpdate[]>([]);
  const [upgrading, setUpgrading] = useState<{ update: RuntimeUpdate; plan: UpgradePlan } | null>(null);
  const [askingUpgrade, setAskingUpgrade] = useState<
    { update: RuntimeUpdate; keep: boolean; step: AskingStep } | null
  >(null);
  const { t } = useTranslation();

  // Reads the latest `installingJob` from inside a `watch` callback registered once — the effect
  // below has no `installingJob` in its deps (re-registering the watch every time that map
  // changes buys nothing).
  const installingJobRef = useRef(installingJob);
  useEffect(() => {
    installingJobRef.current = installingJob;
  }, [installingJob]);

  // `stillShow` is the error that has to survive this reload. A failed install job still has to
  // be told even when the read right after it answers normally: a read that works says nothing
  // about whether the install finished. Empty — the default — means "a finished read leaves the
  // screen clean", exactly as before.
  const reload = useCallback(
    async (stillShow = "") => {
      try {
        const [inst, avail, found] = await Promise.all([
          api.runtimesInstalled(),
          api.runtimesAvailable(),
          api.runtimesFound(),
        ]);
        setInstalled(inst.runtimes);
        setOnDisk(runtimeRowsFrom(found));
        setAvailable(avail.runtimes);
        setStale(avail.stale);
        setUnavailable(avail.unavailable ?? []);
        setUpdates(avail.updates ?? []);
        setError(stillShow);
      } catch (e) {
        setError(errorMessage(t, e));
      } finally {
        setLoaded(true);
      }
    },
    [t],
  );

  // Read again on mount and on every return to this tab — the same reason `Dashboard.tsx` has.
  // Kept out of the watch effect below: the watch has to live for the component's whole life (an
  // install job still has to be followed while the user is over in Packages or another screen),
  // while the reload only has to run while the screen is *being looked at*.
  useEffect(() => {
    if (active) void reload();
  }, [active, reload]);

  // A navigation request from `packagesNavigation.ts` — "Install a PHP" on the PHP extensions
  // screen means "open Packages already searching for php", and the search box lives here. Read on
  // the `active` edge rather than on mount: this component stays mounted between visits, so mount
  // happens once while the request can arrive any number of times afterwards.
  useEffect(() => {
    if (!active) return;
    const requested = takePendingLanguageFilter();
    if (requested !== null) setFilter(requested);
  }, [active]);

  useEffect(() => {
    return subscribeDaemonWatch((raw) => {
      setJobs((current) => applyJob(current, raw));
      // A job being followed has just finished: the "installed" table does not learn of the new
      // build unless it reads again — no other API carries this news (T3, `daemonState.ts`: an
      // event is never the only path, but here it is the *first* one, and reopening the tab
      // stays the fallback).
      const finished = jobFinished(raw);
      if (finished !== null && Object.values(installingJobRef.current).includes(finished.id)) {
        // When a job fails, `job_finished` is the only place that says why — see `jobFinished`.
        // The reload still has to run (a job that failed partway may well have changed
        // something), but it may not wipe out the sentence that has just arrived.
        void reload(finished.error === null ? "" : errorMessage(t, finished.error));
        setInstallingJob((current) => {
          const next = { ...current };
          for (const key of Object.keys(next)) {
            if (next[key] === finished.id) delete next[key];
          }
          return next;
        });
      }
    });
  }, [reload, t]);

  // Ask what this machine lacks first, so a click ends in a working runtime or in one question —
  // never in a download that fails at its last step (T151).
  async function install(release: RuntimeRelease) {
    setError("");
    let unmet;
    try {
      ({ unmet } = await api.runtimeRequirements({ kind: release.kind, version: release.version }));
    } catch {
      // A question the daemon cannot answer asks nothing: the install reports the same failure in
      // its own words, as `mix` does.
      await start(release.kind, release.version, false);
      return;
    }

    const step = requirementStep(unmet);
    if (step.kind === "proceed") {
      await start(release.kind, release.version, false);
      return;
    }

    // **Said, not asked** — T27e: `mix` prints the same warning and installs, so the window does
    // too rather than putting a dialog in front of a machine that can run this perfectly well.
    if (step.kind === "notice") {
      setNotice(
        t("mixengine.requirements.librariesNotice", {
          name: `${release.kind} ${release.version}`,
          libraries: splitLibraries(step.needs).libraries.join(", "),
        }),
      );
      await start(release.kind, release.version, false);
      return;
    }

    const asked = askingStep(step);
    if (asked === null) {
      setError(
        t("mixengine.requirements.unavailable", {
          name: `${release.kind} ${release.version}`,
          needs: step.needs.map(needLabel).join(", "),
        }),
      );
      return;
    }
    setAsking({ release, step: asked });
  }

  async function start(kind: RuntimeKind, version: PackageVersion, installPrerequisites: boolean) {
    setError("");
    try {
      const job = await api.runtimeInstall({
        kind,
        version,
        install_prerequisites: installPrerequisites,
      });
      setInstallingJob((current) => ({ ...current, [versionKey(kind, version)]: job.id }));
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }

  async function uninstall(target: RuntimeSummary, force: boolean) {
    setError("");
    try {
      await api.runtimeUninstall({ kind: target.kind, version: target.version, force });
      setUninstallTarget(null);
      setForceHint(null);
      void reload();
    } catch (e) {
      // No `force` sent yet and the refusal is a project pin: ask again with `force`, showing the
      // message the daemon wrote — it names the project — rather than one made up here.
      const step = afterRefusal(force, errorMessage(t, e));

      if (step.ask === "force") {
        setForceHint(step.hint);
        return;
      }

      // Closed on the way out: a dialog left up once there is nothing left to ask is a dialog
      // holding the screen with nothing to say. See `forceStep.ts`.
      setUninstallTarget(null);
      setForceHint(null);
      setError(step.error);
    }
  }

  // T193b: the plan first, then what this machine lacks for the new version (T151), then the job.
  async function askToUpgrade(update: RuntimeUpdate) {
    setError("");
    try {
      const plan = await api.runtimeUpgradePlan({ kind: update.kind, from: update.from, to: update.to });
      setUpgrading({ update, plan });
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }

  // After the plan was agreed: what the new version lacks is asked exactly as `install` asks it.
  async function upgrade(update: RuntimeUpdate, keep: boolean) {
    setUpgrading(null);
    const step = requirementStep(update.needs ?? []);
    if (step.kind === "proceed" || step.kind === "notice") {
      await startUpgrade(update, keep, false);
      return;
    }

    const asked = askingStep(step);
    // Only agreeing to install what MixEngine can install continues an update. Choosing another
    // version is a different question — which release of the line to go to — and is answered by
    // the version list, so it is said rather than offered here.
    if (asked === null || asked.kind !== "consent") {
      setError(
        t("mixengine.requirements.unavailable", {
          name: `${update.kind} ${update.to}`,
          needs: step.needs.map(needLabel).join(", "),
        }),
      );
      return;
    }
    setAskingUpgrade({ update, keep, step: asked });
  }

  async function startUpgrade(update: RuntimeUpdate, keep: boolean, installPrerequisites: boolean) {
    setError("");
    try {
      const job = await api.runtimeUpgrade({
        kind: update.kind,
        from: update.from,
        to: update.to,
        keep,
        install_prerequisites: installPrerequisites,
      });
      setInstallingJob((current) => ({ ...current, [versionKey(update.kind, update.from)]: job.id }));
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }

  async function adopt(row: OnDiskRow) {
    setAdopting(row.key);
    setError("");
    try {
      await api.runtimeAdopt(row.name, row.version);
      void reload();
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setAdopting(null);
    }
  }

  async function setDefault(target: RuntimeSummary) {
    setError("");
    try {
      await api.runtimeSetDefault({ kind: target.kind, version: target.version });
      void reload();
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }

  const shownInstalled = newestFirst(installed, (row) => row.kind);
  // One row per line, the rest behind a toggle — T193a. Which line a release is in and which is
  // newest are the daemon's answers; `groupByLine` only groups by them.
  const lineGroups = groupByLine(
    newestFirst(
      available.filter((release) => !release.installed),
      (release) => release.kind,
    ),
    (release) => release.kind,
    (release) => matchesAvailable([release.kind, release.version, release.channel], filter),
    openLines,
  );
  const toggleLine = (key: string) =>
    setOpenLines((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  function releaseRow(release: RuntimeRelease, nested: boolean, toggle: ReactNode) {
    const key = versionKey(release.kind, release.version);
    const job = jobFor(jobs, installingJob[key]);
    const { others, libraries } = splitLibraries(
      (release.needs ?? []).map((requirement) => requirement.need),
    );
    const needs =
      libraries.length > 0
        ? [...others, t("mixengine.requirements.systemLibraries", { count: libraries.length })]
        : others;
    return (
      <li key={key} className={nested ? styles.releaseNested : styles.release}>
        <MonogramBadge name={release.kind} size={28} />
        <span className={styles.releaseName}>
          {release.kind}
          {toggle}
        </span>
        <span className={styles.version}>{release.version}</span>
        <span className={styles.tag}>{release.channel}</span>
        <span
          className={styles.needs}
          title={libraries.length > 0 ? libraries.join(", ") : t("mixengine.requirements.columnNeeds")}
        >
          {needs.join(", ")}
        </span>
        {job ? (
          <span className={styles.progress}>
            <progress value={job.percent} max={100} />
            <span className={styles.progressText}>{job.message}</span>
          </span>
        ) : (
          <Button variant="soft" className={styles.install} onClick={() => void install(release)}>
            {t("mixengine.packages.install")}
          </Button>
        )}
      </li>
    );
  }

  return (
    <div className={styles.catalogue}>
      {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}
      {notice !== "" && <NoticeBanner message={notice} onDismiss={() => setNotice("")} />}

      <OnDiskCard rows={onDisk} adopting={adopting} onAdopt={(row) => void adopt(row)} />

      <Card title={t("mixengine.packages.installedTitle")} count={loaded ? installed.length : undefined} flush>
        {!loaded ? (
          <LoadingState />
        ) : installed.length === 0 ? (
          <EmptyState title={t("mixengine.packages.installedEmpty")} />
        ) : (
          <Table aria-label={t("mixengine.packages.installedTitle")}>
            <thead>
              <tr>
                <th>{t("mixengine.packages.columnRuntime")}</th>
                <th>{t("mixengine.packages.columnVersion")}</th>
                <th>{t("mixengine.packages.columnChannel")}</th>
                <th>{t("mixengine.packages.columnInstalledAt")}</th>
                <th>{t("mixengine.packages.columnDefault")}</th>
                <th data-align="end">{t("mixengine.packages.columnActions")}</th>
              </tr>
            </thead>
            <tbody>
              {shownInstalled.map((row) => {
                const key = versionKey(row.kind, row.version);
                const open = expanded === key;
                const update = updateRowState(
                  updates.find(
                    (candidate) => candidate.kind === row.kind && candidate.from === row.version,
                  ),
                  jobFor(jobs, installingJob[key]),
                );
                return (
                  <Fragment key={key}>
                    <tr className={update.kind === "none" ? undefined : styles.withUpdate}>
                      <td data-nowrap>
                        <span className={styles.name}>
                          <MonogramBadge name={row.kind} size={34} />
                          {row.kind === "php" ? (
                            // PHP alone has extensions to show under its row.
                            <Button
                              variant="link"
                              aria-expanded={open}
                              onClick={() => setExpanded(open ? null : key)}
                            >
                              {row.kind}
                              <ChevronDownIcon
                                size={14}
                                className={open ? styles.chevronOpen : styles.chevron}
                              />
                            </Button>
                          ) : (
                            row.kind
                          )}
                        </span>
                      </td>
                      <td className={styles.version}>{row.version}</td>
                      <td>
                        <span className={styles.tag}>{row.channel}</span>
                      </td>
                      <td className={styles.muted}>{formatInstalledAt(row.installed_at)}</td>
                      <td>
                        {row.default ? (
                          <span className={styles.defaultPill}>{t("mixengine.packages.columnDefault")}</span>
                        ) : (
                          <Button size="small" variant="ghost" onClick={() => void setDefault(row)}>
                            {t("mixengine.packages.setDefault")}
                          </Button>
                        )}
                      </td>
                      <td data-align="end" data-nowrap>
                        <Button size="small" variant="danger" onClick={() => setUninstallTarget(row)}>
                          {t("mixengine.packages.uninstall")}
                        </Button>
                      </td>
                    </tr>
                    <UpdateRow
                      columns={6}
                      state={update}
                      onUpdate={(update) => void askToUpgrade(update)}
                    />
                    {open && row.kind === "php" && (
                      <tr className={styles.expansion}>
                        <td colSpan={6}>
                          <ExtensionsPanel target={{ kind: row.kind, version: row.version }} />
                        </td>
                      </tr>
                    )}
                  </Fragment>
                );
              })}
            </tbody>
          </Table>
        )}
      </Card>

      <Card
        title={t("mixengine.packages.availableTitle")}
        count={
          loaded ? (
            <>
              {lineGroups.length}
              <StaleBadge stale={stale} />
              <UnavailableNote gaps={unavailable} />
            </>
          ) : undefined
        }
        actions={
          <Input
            allowClear
            className={styles.filter}
            placeholder={t("mixengine.packages.searchAvailable")}
            aria-label={t("mixengine.packages.searchAvailable")}
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            onKeyDown={(e) => {
              // Escape clears the search and stops here — it must not bubble up and close the tab.
              if (e.key !== "Escape" || filter === "") return;
              e.preventDefault();
              e.stopPropagation();
              setFilter("");
            }}
          />
        }
        flush
      >
        {!loaded ? (
          <LoadingState />
        ) : lineGroups.length === 0 ? (
          filter.trim() !== "" && <EmptyState title={t("mixengine.packages.noMatches")} />
        ) : (
          <ul className={styles.available}>
            {lineGroups.map((group) => (
              <Fragment key={group.key}>
                {releaseRow(
                  group.head,
                  false,
                  group.others.length > 0 && (
                    <Button
                      variant="link"
                      className={styles.moreInLine}
                      aria-expanded={group.open}
                      onClick={() => toggleLine(group.key)}
                    >
                      {t("mixengine.packages.moreInLine", { count: group.others.length })}
                    </Button>
                  ),
                )}
                {group.open && group.others.map((release) => releaseRow(release, true, null))}
              </Fragment>
            ))}
          </ul>
        )}
      </Card>

      {uninstallTarget && (
        <ConfirmDialog
          title={t("mixengine.packages.uninstallConfirmTitle", { version: uninstallTarget.version })}
          message={forceHint ?? uninstallTarget.version}
          confirmLabel={
            forceHint !== null ? t("mixengine.packages.uninstallForceConfirm") : undefined
          }
          danger
          onCancel={() => {
            setUninstallTarget(null);
            setForceHint(null);
          }}
          onConfirm={() => void uninstall(uninstallTarget, forceHint !== null)}
        />
      )}

      {upgrading && (
        <UpgradeDialog
          plan={upgrading.plan}
          onCancel={() => setUpgrading(null)}
          onConfirm={(keep) => void upgrade(upgrading.update, keep)}
        />
      )}

      {askingUpgrade && (
        <RequirementDialog
          name={`${askingUpgrade.update.kind} ${askingUpgrade.update.to}`}
          step={askingUpgrade.step}
          onCancel={() => setAskingUpgrade(null)}
          onInstall={() => {
            const { update, keep } = askingUpgrade;
            setAskingUpgrade(null);
            void startUpgrade(update, keep, true);
          }}
          onChoose={() => setAskingUpgrade(null)}
        />
      )}

      {asking && (
        <RequirementDialog
          name={`${asking.release.kind} ${asking.release.version}`}
          step={asking.step}
          onCancel={() => setAsking(null)}
          onInstall={() => {
            const { release } = asking;
            setAsking(null);
            void start(release.kind, release.version, true);
          }}
          onChoose={(version) => {
            const { release } = asking;
            setAsking(null);
            void start(release.kind, version, false);
          }}
        />
      )}
    </div>
  );
}
