import { Fragment, useCallback, useEffect, useState, type MouseEvent } from "react";

import ActionBar from "../../../../components/ActionBar";
import Button from "../../../../components/Button";
import Card from "../../../../components/Card";
import ConfirmDialog from "../../../../components/ConfirmDialog";
import ContextMenu from "../../../../components/ContextMenu";
import EmptyState from "../../../../components/EmptyState";
import ErrorBanner from "../../../../components/ErrorBanner";
import IconTile from "../../../../components/IconTile";
import LoadingState from "../../../../components/LoadingState";
import NoticeBanner from "../../../../components/NoticeBanner";
import PageHeader from "../../../../components/PageHeader";
import Table from "../../../../components/Table";
import { copyText } from "../../../../core/clipboard";
import { errorMessage } from "../../../../core/errors";
import {
  ChevronDownIcon,
  CopyIcon,
  FolderIcon,
  GlobeIcon,
  MoreIcon,
  PencilIcon,
  PlusIcon,
  TrashIcon,
  UploadIcon,
} from "../../../../icons";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { ProjectDetail } from "@mixengine/api";
import type { ProjectSummary } from "@mixengine/api";
import { takePendingProjectDetail } from "../../projectsNavigation";
import { siteAddresses, type SiteAddresses } from "../../siteGroups";
import ProjectDetailPanel from "./ProjectDetailPanel";
import ProjectForm from "./ProjectForm";
import styles from "./Projects.module.css";

interface Props {
  active: boolean;
  /** Over to the Sites screen, filtered to this project — see `sitesNavigation.ts`. */
  onOpenSites: (project: string) => void;
  /** Over to the Dashboard, where a database's sign-in details are shown — T205, D8. */
  onOpenDashboard: () => void;
  /** Whether this window draws the Terminal module, for the steps panel — T205. */
  terminalVisible: boolean;
}

/** Every project registered in the home — create, edit (name/root/pins), delete. */
export default function Projects({ active, onOpenSites, onOpenDashboard, terminalVisible }: Props) {
  const [rows, setRows] = useState<ProjectSummary[]>([]);
  /** False until the first read has answered — until then an empty `rows` means "not known yet",
   *  not "no projects". */
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState("");
  const [creating, setCreating] = useState(false);
  const [editing, setEditing] = useState<ProjectDetail | null>(null);
  /** The project whose row is open, and what has been read of it — T205: a row opens in place,
   *  under itself, so what someone just clicked is where they are looking. */
  const [expanded, setExpanded] = useState<string | null>(null);
  const [detail, setDetail] = useState<ProjectDetail | null>(null);
  /** The shown project's site addresses, for the steps panel's `open` rows — T205, T204a. */
  const [detailAddresses, setDetailAddresses] = useState<SiteAddresses>({ first: null, byDomain: {} });
  const [deleting, setDeleting] = useState<string | null>(null);
  /** The row whose ⋮ menu is open, and where — the actions used less often live there, as on the
   *  Sites screen, so a row shows what is done every day and nothing else. */
  const [menu, setMenu] = useState<{ name: string; x: number; y: number } | null>(null);
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
      setExpanded((open) =>
        open !== null && list.projects.some((row) => row.name === open) ? open : null,
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

  /** The project's detail, and the address its steps open — read together. */
  async function readDetail(name: string) {
    const [shown, listed] = await Promise.all([api.projectShow(name), api.sites(name)]);
    setDetail(shown);
    setDetailAddresses(siteAddresses(listed.sites, null));
  }

  /** Opens `name`'s row, or closes it when it is the open one. One open at a time. */
  async function toggle(name: string) {
    if (expanded === name) {
      setExpanded(null);
      return;
    }
    setExpanded(name);
    setDetail(null);
    try {
      await readDetail(name);
    } catch (e) {
      setExpanded(null);
      setError(errorMessage(t, e));
    }
  }

  /* Another screen sent someone to one project — the Sites screen's *What to run*, an apply that
     has just finished: its row is opened, once (`projectsNavigation.ts`). */
  useEffect(() => {
    if (!active) return;
    const wanted = takePendingProjectDetail();
    if (wanted === null) return;
    setExpanded(wanted);
    setDetail(null);
    readDetail(wanted).catch((e: unknown) => {
      setExpanded(null);
      setError(errorMessage(t, e));
    });
  }, [active, t]);

  /** A click anywhere on a row opens it, except on the row's own buttons. */
  function rowClick(e: MouseEvent<HTMLTableRowElement>, name: string) {
    if ((e.target as HTMLElement).closest("button, a, input")) return;
    void toggle(name);
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
      await readDetail(project);
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

      <Card flush demoFocus="projects">
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
              {rows.map((row) => {
                const open = expanded === row.name;
                return (
                  <Fragment key={row.name}>
                    <tr
                      className={open ? `${styles.row} ${styles.open}` : styles.row}
                      onClick={(e) => rowClick(e, row.name)}
                      // Which row a clip's step means: the hooks below are on every row alike.
                      data-demo-key={row.name}
                    >
                      {/* The open row's name and root, with its pins below, are what pin-runtime ends framed on:
                          measured by their words, and closer than the camera's usual 2×. */}
                      <td
                        data-demo-focus={open ? "project-open" : undefined}
                        data-demo-fit={open ? "text" : undefined}
                        data-demo-zoom-max={open ? "3.3" : undefined}
                      >
                        <span className={styles.name}>
                          <IconTile tone="coral">
                            <FolderIcon size={16} />
                          </IconTile>
                          {row.name}
                        </span>
                      </td>
                      <td data-demo-focus={open ? "project-open" : undefined} data-demo-fit={open ? "text" : undefined}>
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
                          {/* At the head of the row's buttons rather than before its name, so a
                              row here is laid out as a row on the Sites screen is. */}
                          <Button
                            size="small"
                            className={open ? `${styles.details} ${styles.detailsOpen}` : styles.details}
                            aria-expanded={open}
                            onClick={() => void toggle(row.name)}
                            data-demo="project-details"
                          >
                            {t("mixengine.projects.details")}
                            <ChevronDownIcon size={14} />
                          </Button>
                          <Button size="small" onClick={() => onOpenSites(row.name)}>
                            <GlobeIcon size={14} />
                            {t("mixengine.projects.openSites")}
                          </Button>
                          <ActionBar
                            actions={[
                              {
                                key: "menu",
                                icon: MoreIcon,
                                label: t("mixengine.projects.rowMenu"),
                                demo: "project-menu",
                                onClick: (event) => {
                                  const at = event.currentTarget.getBoundingClientRect();
                                  setMenu({ name: row.name, x: at.left, y: at.bottom });
                                },
                              },
                            ]}
                          />
                        </span>
                      </td>
                    </tr>
                    {open && (
                      <tr className={styles.detailRow}>
                        <td colSpan={4}>
                          {detail?.project.name === row.name ? (
                            <ProjectDetailPanel
                              detail={detail}
                              addresses={detailAddresses}
                              terminalVisible={terminalVisible}
                              onOpenDashboard={onOpenDashboard}
                              adopting={adopting}
                              onAdopt={(domain) => void adoptSite(row.name, domain)}
                            />
                          ) : (
                            <LoadingState compact />
                          )}
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

      {menu !== null && (
        <ContextMenu x={menu.x} y={menu.y} onClose={() => setMenu(null)}>
          <button
            type="button"
            data-demo="project-edit"
            onClick={() => {
              const name = menu.name;
              setMenu(null);
              void edit(name);
            }}
          >
            <PencilIcon size={14} />
            {t("mixengine.projects.edit")}
          </button>
          <button
            type="button"
            disabled={exporting !== null}
            onClick={() => {
              const name = menu.name;
              setMenu(null);
              void exportManifest(name);
            }}
          >
            <UploadIcon size={14} />
            {exporting === menu.name ? t("mixengine.projects.exporting") : t("mixengine.projects.exportManifest")}
          </button>

          <div className="context-menu-separator" />

          <button
            type="button"
            className="context-menu-delete"
            onClick={() => {
              const name = menu.name;
              setMenu(null);
              setDeleting(name);
            }}
          >
            <TrashIcon size={14} />
            {t("mixengine.projects.delete")}
          </button>
        </ContextMenu>
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
            if (expanded === editing.project.name) setExpanded(null);
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
