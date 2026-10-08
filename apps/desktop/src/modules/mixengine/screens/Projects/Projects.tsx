import { useCallback, useEffect, useState } from "react";

import Button from "../../../../components/Button";
import Card from "../../../../components/Card";
import ConfirmDialog from "../../../../components/ConfirmDialog";
import EmptyState from "../../../../components/EmptyState";
import ErrorBanner from "../../../../components/ErrorBanner";
import IconTile from "../../../../components/IconTile";
import LoadingState from "../../../../components/LoadingState";
import NoticeBanner from "../../../../components/NoticeBanner";
import PageHeader from "../../../../components/PageHeader";
import StatusPill from "../../../../components/StatusPill";
import Table from "../../../../components/Table";
import { copyText } from "../../../../core/clipboard";
import { errorMessage } from "../../../../core/errors";
import { CopyIcon, FolderIcon, GlobeIcon, PlusIcon } from "../../../../icons";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { ProjectDetail } from "@mixengine/api";
import type { ProjectSummary } from "@mixengine/api";
import { declaredSiteRows } from "../../declaredSites";
import { formatPins } from "../../projectPins";
import ProjectForm from "./ProjectForm";
import styles from "./Projects.module.css";

interface Props {
  active: boolean;
  /** Over to the Sites screen, filtered to this project — see `sitesNavigation.ts`. */
  onOpenSites: (project: string) => void;
}

/** Every project registered in the home — create, edit (name/root/pins), delete. */
export default function Projects({ active, onOpenSites }: Props) {
  const [rows, setRows] = useState<ProjectSummary[]>([]);
  /** False until the first read has answered — until then an empty `rows` means "not known yet",
   *  not "no projects". */
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState("");
  const [creating, setCreating] = useState(false);
  const [editing, setEditing] = useState<ProjectDetail | null>(null);
  const [detail, setDetail] = useState<ProjectDetail | null>(null);
  const [deleting, setDeleting] = useState<string | null>(null);
  /** The project whose manifest is being written. */
  const [exporting, setExporting] = useState<string | null>(null);
  /** The declared site being created from the manifest. */
  const [adopting, setAdopting] = useState<string | null>(null);
  const [notice, setNotice] = useState("");
  const { t } = useTranslation();

  const reload = useCallback(async () => {
    try {
      const list = await api.projects();
      setRows(list.projects);
      setError("");
      // The pins panel follows the list: a project that is gone — deleted here, by `mix`, or
      // forgotten by a blueprint that failed — takes its panel with it rather than leaving a card
      // about something the table no longer shows.
      setDetail((shown) =>
        shown && list.projects.some((row) => row.name === shown.project.name) ? shown : null,
      );
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setLoaded(true);
    }
  }, [t]);

  // Read again on mount and on every return to this screen — the same reason `Dashboard.tsx` has.
  useEffect(() => {
    if (active) void reload();
  }, [active, reload]);

  async function showDetail(name: string) {
    try {
      setDetail(await api.projectShow(name));
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }

  async function edit(name: string) {
    try {
      setEditing(await api.projectShow(name));
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }

  /** Creates one site the manifest declares, every field falling through to the file (T204). */
  async function adoptSite(project: string, domain: string) {
    setAdopting(domain);
    try {
      await api.siteCreate({ project: { name: project }, from: domain });
      setDetail(await api.projectShow(project));
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setAdopting(null);
    }
  }

  /**
   * Writes the project's `mixengine.toml`. No confirmation: the daemon merges into a file that is
   * there rather than rewriting it, so nothing a person wrote is lost.
   */
  async function exportManifest(name: string) {
    setExporting(name);
    try {
      const written = await api.projectExport(name);
      const said = written.created
        ? t("mixengine.projects.exportCreated", { path: written.path })
        : t("mixengine.projects.exportUpdated", { path: written.path });
      const kept = written.sites_kept ?? [];
      setNotice(
        kept.length === 0
          ? said
          : `${said} ${t("mixengine.projects.exportKept", { sites: kept.join(", ") })}`,
      );
      void reload();
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setExporting(null);
    }
  }

  async function confirmDelete(name: string) {
    try {
      await api.projectDelete(name);
      void reload();
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      // **Closed either way.** A dialog kept up on a refusal is one the screen has no second
      // question for, and the banner behind it is where the refusal is being read anyway.
      setDeleting(null);
    }
  }

  return (
    <div className={`mixengine-page ${styles.projects}`}>
      {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}
      {notice !== "" && <NoticeBanner message={notice} onDismiss={() => setNotice("")} />}

      <PageHeader
        title={t("mixengine.sidebar.projects")}
        description={t("mixengine.projects.about")}
        actions={
          <Button size="large" variant="primary" onClick={() => setCreating(true)}>
            <PlusIcon size={15} />
            {t("mixengine.projects.newProject")}
          </Button>
        }
      />

      <Card flush>
        {!loaded ? (
          <LoadingState />
        ) : rows.length === 0 ? (
          <EmptyState title={t("mixengine.projects.empty")} />
        ) : (
          <Table aria-label={t("mixengine.sidebar.projects")}>
            <thead>
              <tr>
                <th>{t("mixengine.projects.columnName")}</th>
                <th>{t("mixengine.projects.columnRoot")}</th>
                <th>{t("mixengine.projects.columnManifest")}</th>
                <th data-align="end">{t("mixengine.sites.columnActions")}</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <tr key={row.name}>
                  <td>
                    <span className={styles.name}>
                      <IconTile tone="coral">
                        <FolderIcon size={16} />
                      </IconTile>
                      <Button variant="link" onClick={() => void showDetail(row.name)}>
                        {row.name}
                      </Button>
                    </span>
                  </td>
                  <td>
                    <span className={styles.root}>
                      <span className={styles.rootPath} title={row.root}>
                        {row.root}
                      </span>
                      <Button
                        size="small"
                        variant="ghost"
                        className={styles.copy}
                        aria-label={t("mixengine.projects.copyRoot")}
                        title={t("mixengine.projects.copyRoot")}
                        onClick={() => void copyText(row.root)}
                      >
                        <CopyIcon size={13} />
                      </Button>
                    </span>
                  </td>
                  <td className={row.manifest ? undefined : styles.none}>
                    {row.manifest ? t("mixengine.projects.present") : t("mixengine.projects.notFound")}
                  </td>
                  <td data-align="end" data-nowrap>
                    <span className={styles.rowActions}>
                      <Button size="small" onClick={() => onOpenSites(row.name)}>
                        <GlobeIcon size={14} />
                        {t("mixengine.projects.openSites")}
                      </Button>
                      <Button size="small" onClick={() => void edit(row.name)}>
                        {t("mixengine.projects.edit")}
                      </Button>
                      <Button
                        size="small"
                        onClick={() => void exportManifest(row.name)}
                        disabled={exporting !== null && exporting !== row.name}
                        busy={exporting === row.name ? t("mixengine.projects.exporting") : undefined}
                      >
                        {t("mixengine.projects.exportManifest")}
                      </Button>
                      <Button size="small" variant="danger" onClick={() => setDeleting(row.name)}>
                        {t("mixengine.projects.delete")}
                      </Button>
                    </span>
                  </td>
                </tr>
              ))}
            </tbody>
          </Table>
        )}
      </Card>

      {detail && (
        <Card
          title={t("mixengine.projects.detail.pinsTitle")}
          count={detail.project.name}
          actions={
            <Button size="small" onClick={() => setDetail(null)}>
              {t("common.close")}
            </Button>
          }
        >
          <ul className={styles.pins}>
            {formatPins(detail.pins).map((pin) => (
              <li key={pin.kind}>
                <strong>{pin.kind}</strong> <code>{pin.constraint}</code> —{" "}
                {pin.sourceLabel === "manifest"
                  ? t("mixengine.projects.detail.sourceManifest", { path: pin.sourcePath ?? "" })
                  : t("mixengine.projects.detail.sourceRow")}
                {pin.resolvedVersion
                  ? ` → ${pin.resolvedVersion}`
                  : pin.hint
                    ? ` — ${t("mixengine.projects.detail.unresolved", { hint: pin.hint })}`
                    : ""}
              </li>
            ))}
          </ul>
        </Card>
      )}

      {detail && declaredSiteRows(detail.declared_sites).length > 0 && (
        <Card
          title={t("mixengine.projects.detail.declaredTitle")}
          description={t("mixengine.projects.detail.declaredDescription")}
          count={detail.project.name}
          flush
        >
          <Table aria-label={t("mixengine.projects.detail.declaredTitle")}>
            <thead>
              <tr>
                <th>{t("mixengine.projects.detail.declaredColumnDomain")}</th>
                <th>{t("mixengine.projects.detail.declaredColumnState")}</th>
                <th data-align="end">{t("mixengine.sites.columnActions")}</th>
              </tr>
            </thead>
            <tbody>
              {declaredSiteRows(detail.declared_sites).map((site) => (
                <tr key={site.domain}>
                  <td>
                    <code>{site.domain}</code>
                  </td>
                  <td>
                    {site.status === "here" && (
                      <StatusPill tone="success">
                        {t("mixengine.projects.detail.declaredHere")}
                      </StatusPill>
                    )}
                    {site.status === "missing" && (
                      <StatusPill tone="neutral">
                        {t("mixengine.projects.detail.declaredMissing")}
                      </StatusPill>
                    )}
                    {site.status === "elsewhere" && (
                      <StatusPill tone="warning">
                        {t("mixengine.projects.detail.declaredElsewhere", { owner: site.owner ?? "" })}
                      </StatusPill>
                    )}
                  </td>
                  <td data-align="end">
                    {site.canAdd && (
                      <Button
                        size="small"
                        onClick={() => void adoptSite(detail.project.name, site.domain)}
                        busy={
                          adopting === site.domain
                            ? t("mixengine.projects.detail.declaredAdding")
                            : undefined
                        }
                      >
                        {t("mixengine.projects.detail.declaredAdd")}
                      </Button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </Table>
        </Card>
      )}

      {creating && (
        <ProjectForm
          onCancel={() => setCreating(false)}
          onSaved={(warning) => {
            setCreating(false);
            if (warning) setError(warning);
            void reload();
          }}
        />
      )}

      {editing && (
        <ProjectForm
          initial={editing}
          onCancel={() => setEditing(null)}
          onSaved={() => {
            // Pins are what an edit changes, so a panel open on this project is closed rather than
            // left showing the old ones — and re-reading it by name would fail after a rename.
            if (detail?.project.name === editing.project.name) setDetail(null);
            setEditing(null);
            void reload();
          }}
        />
      )}

      {deleting && (
        <ConfirmDialog
          title={t("mixengine.projects.deleteTitle")}
          message={t("mixengine.projects.deleteMessage")}
          danger
          onCancel={() => setDeleting(null)}
          onConfirm={() => void confirmDelete(deleting)}
        />
      )}
    </div>
  );
}
