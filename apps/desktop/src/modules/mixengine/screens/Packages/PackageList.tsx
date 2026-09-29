import { Fragment, useState, type ReactNode } from "react";

import Button from "../../../../components/Button";
import Card from "../../../../components/Card";
import EmptyState from "../../../../components/EmptyState";
import ErrorBanner from "../../../../components/ErrorBanner";
import LoadingState from "../../../../components/LoadingState";
import { packageRowsFrom } from "../../onDisk";
import OnDiskCard from "./OnDiskCard";
import Input from "../../../../components/Input";
import MonogramBadge from "../../../../components/MonogramBadge";
import NoticeBanner from "../../../../components/NoticeBanner";
import Table from "../../../../components/Table";
import { useTranslation } from "../../../../i18n";
import { formatInstalledAt, jobFor, newestFirst, versionKey } from "../../runtimeState";
import RequirementDialog from "../../components/RequirementDialog";
import UpdateRow from "../../components/UpdateRow";
import UpgradeDialog from "../../components/UpgradeDialog";
import StaleBadge from "../../components/StaleBadge";
import { splitLibraries } from "../../requirementStep";
import { matchesAvailable } from "./availableFilter";
import { groupByLine } from "./availableLines";
import { updateRowState } from "../../updateRow";
import { packageCategory, type PackageCategory } from "./packageCategories";
import type { PackagesState } from "./usePackages";
import type { PackageRelease } from "@mixengine/api";
import styles from "./Catalogue.module.css";

/**
 * One package group — which group is decided by the tab strip in `Packages.tsx`, not by a sub-tab
 * here: the four groups stand level with Languages on a single tab strip, not nested two deep.
 * State lives in `usePackages`, above every tab, so switching groups does not lose track of a job
 * that is installing.
 */
export default function PackageList({
  category,
  state,
}: {
  category: PackageCategory;
  state: PackagesState;
}) {
  const { t } = useTranslation();
  const { installed, available, loaded, stale, jobs, installingJob, error, clearError, notice, clearNotice } =
    state;
  const onDisk = packageRowsFrom(state.onDisk, (name) => packageCategory(name) === category);

  // Only the "not installed" table is filtered: the table above is what this machine holds, usually
  // a few rows, and hiding some of them behind a search hides exactly what the user needs to see in
  // full before removing anything.
  const [filter, setFilter] = useState("");
  // Lines the person opened to see their older releases — T193a.
  const [openLines, setOpenLines] = useState<Set<string>>(new Set());

  const installedInCategory = newestFirst(
    installed.filter((row) => packageCategory(row.package) === category),
    (row) => row.package,
  );
  // One row per line, the rest behind a toggle — T193a, as `Languages.tsx` draws it.
  const lineGroups = groupByLine(
    newestFirst(
      available.filter(
        (release) => packageCategory(release.package) === category && !release.installed,
      ),
      (release) => release.package,
    ),
    (release) => release.package,
    (release) => matchesAvailable([release.package, release.version, release.channel], filter),
    openLines,
  );
  const toggleLine = (key: string) =>
    setOpenLines((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  function releaseRow(release: PackageRelease, nested: boolean, toggle: ReactNode) {
    const key = versionKey(release.package, release.version);
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
        <MonogramBadge name={release.package} size={28} />
        <span className={styles.releaseName}>
          {release.package}
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
          <Button variant="soft" className={styles.install} onClick={() => void state.install(release)}>
            {t("mixengine.packages.install")}
          </Button>
        )}
      </li>
    );
  }

  return (
    <div className={styles.catalogue}>
      {error !== "" && <ErrorBanner message={error} onDismiss={clearError} />}
      {notice !== "" && <NoticeBanner message={notice} onDismiss={clearNotice} />}

      <OnDiskCard
        rows={onDisk}
        adopting={state.adopting}
        onAdopt={(row) => void state.adopt(row.name, row.version)}
      />

      <Card title={t("mixengine.packages.installedTitle")} count={loaded ? installedInCategory.length : undefined} flush>
        {!loaded ? (
          <LoadingState />
        ) : installedInCategory.length === 0 ? (
          <EmptyState title={t("mixengine.packages.installedEmpty")} />
        ) : (
          <Table aria-label={t("mixengine.packages.installedTitle")}>
            <thead>
              <tr>
                <th>{t("mixengine.packages.columnPackage")}</th>
                <th>{t("mixengine.packages.columnVersion")}</th>
                <th>{t("mixengine.packages.columnInstalledAt")}</th>
                <th>{t("mixengine.packages.columnServices")}</th>
                <th data-align="end">{t("mixengine.packages.columnActions")}</th>
              </tr>
            </thead>
            <tbody>
              {installedInCategory.map((row) => {
                const key = versionKey(row.package, row.version);
                const inUse = row.services.length > 0;
                const update = updateRowState(
                  state.updates.find(
                    (candidate) => candidate.package === row.package && candidate.from === row.version,
                  ),
                  jobFor(jobs, installingJob[key]),
                );
                return (
                  <Fragment key={key}>
                    <tr className={update.kind === "none" ? undefined : styles.withUpdate}>
                      <td data-nowrap>
                        <span className={styles.name}>
                          <MonogramBadge name={row.package} size={28} />
                          {row.package}
                        </span>
                      </td>
                      <td className={styles.version}>{row.version}</td>
                      <td className={styles.muted}>{formatInstalledAt(row.installed_at)}</td>
                      <td className={inUse ? styles.services : styles.muted}>
                        {inUse ? row.services.join(", ") : "—"}
                      </td>
                      <td data-align="end" data-nowrap>
                        <Button
                          size="small"
                          variant="danger"
                          onClick={() => void state.uninstall(row)}
                          disabled={inUse}
                          title={
                            inUse
                              ? t("mixengine.packages.uninstallBlockedMessage", {
                                  services: row.services.join(", "),
                                })
                              : undefined
                          }
                        >
                          {t("mixengine.packages.uninstall")}
                        </Button>
                      </td>
                    </tr>
                    <UpdateRow
                      columns={5}
                      state={update}
                      onUpdate={(update) => void state.askToUpgrade(update)}
                    />
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
              // Escape clears the search and stops here — it must not bubble up and close the whole
              // open tab.
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

      {state.upgrading && (
        <UpgradeDialog
          plan={state.upgrading.plan}
          onCancel={state.dismissUpgrade}
          onConfirm={(keep) => {
            if (state.upgrading) void state.upgrade(state.upgrading.update, keep);
          }}
        />
      )}

      {state.askingUpgrade && (
        <RequirementDialog
          name={`${state.askingUpgrade.update.package} ${state.askingUpgrade.update.to}`}
          step={state.askingUpgrade.step}
          onCancel={state.dismissAskingUpgrade}
          onInstall={() => void state.agreeUpgrade()}
          onChoose={state.dismissAskingUpgrade}
        />
      )}

      {state.asking && (
        <RequirementDialog
          name={`${state.asking.release.package} ${state.asking.release.version}`}
          step={state.asking.step}
          onCancel={state.dismissAsking}
          onInstall={() => void state.agree()}
          onChoose={(version) => void state.chooseInstead(version)}
        />
      )}
    </div>
  );
}
