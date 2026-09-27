import { useState } from "react";

import Button from "../../../../components/Button";
import Card from "../../../../components/Card";
import EmptyState from "../../../../components/EmptyState";
import ErrorBanner from "../../../../components/ErrorBanner";
import { packageRowsFrom } from "../../onDisk";
import OnDiskCard from "./OnDiskCard";
import Input from "../../../../components/Input";
import MonogramBadge from "../../../../components/MonogramBadge";
import NoticeBanner from "../../../../components/NoticeBanner";
import Table from "../../../../components/Table";
import { useTranslation } from "../../../../i18n";
import { formatInstalledAt, jobFor, versionKey } from "../../runtimeState";
import RequirementDialog from "../../components/RequirementDialog";
import StaleBadge from "../../components/StaleBadge";
import { splitLibraries } from "../../requirementStep";
import { matchesAvailable } from "./availableFilter";
import { packageCategory, type PackageCategory } from "./packageCategories";
import type { PackagesState } from "./usePackages";
import styles from "./Catalogue.module.css";

/**
 * Một nhóm package — nhóm nào là do dải tab của `Runtimes.tsx` quyết định, không phải một tab con
 * ở đây: bốn nhóm đứng ngang hàng với Ngôn ngữ trên đúng một dải tab, không lồng hai tầng.
 * State thì ở `usePackages`, cao hơn mọi tab, nên đổi nhóm không làm mất dấu một job đang cài.
 */
export default function Packages({
  category,
  state,
}: {
  category: PackageCategory;
  state: PackagesState;
}) {
  const { t } = useTranslation();
  const { installed, available, stale, jobs, installingJob, error, clearError, notice, clearNotice } =
    state;
  const onDisk = packageRowsFrom(state.onDisk, (name) => packageCategory(name) === category);

  // Chỉ lọc bảng "chưa cài": bảng trên là những bản máy này đang giữ, thường vài hàng, và giấu bớt
  // chúng sau một câu tìm kiếm là giấu đúng thứ người dùng cần thấy đủ trước khi gỡ.
  const [filter, setFilter] = useState("");

  const installedInCategory = installed.filter((row) => packageCategory(row.package) === category);
  const availableInCategory = available.filter(
    (release) =>
      packageCategory(release.package) === category &&
      !release.installed &&
      matchesAvailable([release.package, release.version, release.channel], filter),
  );

  return (
    <div className={styles.catalogue}>
      {error !== "" && <ErrorBanner message={error} onDismiss={clearError} />}
      {notice !== "" && <NoticeBanner message={notice} onDismiss={clearNotice} />}

      <OnDiskCard
        rows={onDisk}
        adopting={state.adopting}
        onAdopt={(row) => void state.adopt(row.name, row.version)}
      />

      <Card title={t("mixengine.runtimes.installedTitle")} count={installedInCategory.length} flush>
        {installedInCategory.length === 0 ? (
          <EmptyState title={t("mixengine.runtimes.installedEmpty")} />
        ) : (
          <Table aria-label={t("mixengine.runtimes.installedTitle")}>
            <thead>
              <tr>
                <th>{t("mixengine.runtimes.columnPackage")}</th>
                <th>{t("mixengine.runtimes.columnVersion")}</th>
                <th>{t("mixengine.runtimes.columnInstalledAt")}</th>
                <th>{t("mixengine.runtimes.columnServices")}</th>
                <th data-align="end">{t("mixengine.runtimes.columnActions")}</th>
              </tr>
            </thead>
            <tbody>
              {installedInCategory.map((row) => {
                const key = versionKey(row.package, row.version);
                const inUse = row.services.length > 0;
                return (
                  <tr key={key}>
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
                            ? t("mixengine.runtimes.uninstallBlockedMessage", {
                                services: row.services.join(", "),
                              })
                            : undefined
                        }
                      >
                        {t("mixengine.runtimes.uninstall")}
                      </Button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </Table>
        )}
      </Card>

      <Card
        title={t("mixengine.runtimes.availableTitle")}
        count={
          <>
            {availableInCategory.length}
            <StaleBadge stale={stale} />
          </>
        }
        actions={
          <Input
            allowClear
            className={styles.filter}
            placeholder={t("mixengine.runtimes.searchAvailable")}
            aria-label={t("mixengine.runtimes.searchAvailable")}
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            onKeyDown={(e) => {
              // Escape xoá câu tìm, và dừng ở đây — không để nó nổi lên đóng cả tab đang mở.
              if (e.key !== "Escape" || filter === "") return;
              e.preventDefault();
              e.stopPropagation();
              setFilter("");
            }}
          />
        }
        flush
      >
        {availableInCategory.length === 0 ? (
          filter.trim() !== "" && <EmptyState title={t("mixengine.runtimes.noMatches")} />
        ) : (
          <ul className={styles.available}>
            {availableInCategory.map((release) => {
              const key = versionKey(release.package, release.version);
              const job = jobFor(jobs, installingJob[key]);
              const { others, libraries } = splitLibraries(
                (release.needs ?? []).map((requirement) => requirement.need),
              );
              const needs =
                libraries.length > 0
                  ? [
                      ...others,
                      t("mixengine.requirements.systemLibraries", { count: libraries.length }),
                    ]
                  : others;
              return (
                <li key={key} className={styles.release}>
                  <MonogramBadge name={release.package} size={28} />
                  <span className={styles.releaseName}>{release.package}</span>
                  <span className={styles.version}>{release.version}</span>
                  <span className={styles.tag}>{release.channel}</span>
                  <span
                    className={styles.needs}
                    title={
                      libraries.length > 0
                        ? libraries.join(", ")
                        : t("mixengine.requirements.columnNeeds")
                    }
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
                      {t("mixengine.runtimes.install")}
                    </Button>
                  )}
                </li>
              );
            })}
          </ul>
        )}
      </Card>

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
