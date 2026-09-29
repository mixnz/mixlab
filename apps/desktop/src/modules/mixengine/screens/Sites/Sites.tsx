import { openUrl } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useState } from "react";

import Button from "../../../../components/Button";
import Card from "../../../../components/Card";
import EmptyState from "../../../../components/EmptyState";
import ErrorBanner from "../../../../components/ErrorBanner";
import PageHeader from "../../../../components/PageHeader";
import Select from "../../../../components/Select";
import StatusPill from "../../../../components/StatusPill";
import Table from "../../../../components/Table";
import { FolderIcon, GlobeIcon, LockIcon, PlusIcon } from "../../../../icons";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { SiteDetail } from "@mixengine/api";
import type { SiteSharing } from "@mixengine/api";
import { subscribeDaemonWatch } from "../../daemonWatch";
import {
  applySharingChange,
  canEditSite,
  formatRemaining,
  siteVisit,
  type SiteRow,
} from "../../siteState";
import { takePendingSitesFilter } from "../../sitesNavigation";
import ShareDialog from "./ShareDialog";
import SiteForm from "./SiteForm";
import styles from "./Sites.module.css";

/** The share cell: a Share button when not shared, a live countdown + an Unshare button when it
 *  is. */
function SharingCell({
  sharing,
  onShare,
  onUnshare,
}: {
  sharing: SiteSharing | null | undefined;
  onShare: () => void;
  onUnshare: () => void;
}) {
  const { t } = useTranslation();
  const [, forceTick] = useState(0);

  useEffect(() => {
    if (!sharing?.until) return;
    const id = window.setInterval(() => forceTick((n) => n + 1), 1000);
    return () => window.clearInterval(id);
  }, [sharing?.until]);

  if (!sharing) {
    return (
      <Button onClick={onShare} size="small" variant="soft">
        {t("mixengine.sites.share.share")}
      </Button>
    );
  }

  return (
    <span className={styles.sharing}>
      {sharing.until ? formatRemaining(sharing.until) : t("mixengine.sites.sharingIndefinite")}
      <Button onClick={onUnshare} size="small">
        {t("mixengine.sites.share.unshare")}
      </Button>
    </span>
  );
}

/**
 * Every site in the home, and their LAN sharing state.
 *
 * **Sharing comes from the stream, not from guessing** — `site_sharing_changed` is what the roadmap
 * calls "the only place `mix` is the weaker client": in the CLI the reason sits in a log nobody
 * reads; here it has to be a line visible the moment it arrives.
 */
export default function Sites({ active }: { active: boolean }) {
  const [rows, setRows] = useState<SiteRow[]>([]);
  const [projectNames, setProjectNames] = useState<string[]>([]);
  const [projectFilter, setProjectFilter] = useState("");
  const [error, setError] = useState("");
  const [creating, setCreating] = useState(false);
  const [editing, setEditing] = useState<SiteDetail | null>(null);
  const [sharing, setSharing] = useState<string | null>(null);
  /**
   * The domain waiting for its services to start; it locks **the whole table**, not just that row.
   *
   * One variable holds one domain, so two overlapping runs would trample each other: the one that
   * finishes first also clears the waiting mark of the one still running. Locking the whole table
   * is the cheapest way to keep that from happening, and the cost — a few seconds unable to click
   * another row — is smaller than a table that misstates what it is doing.
   */
  const [opening, setOpening] = useState<string | null>(null);
  const { t } = useTranslation();

  // `filterOverride` is the way out of `setState`'s one-beat delay: the effect below that refreshes
  // the project list computes a valid filter and then needs to read sites *right away* with that
  // value, not with the old `projectFilter` still sitting in the closure until the next render.
  const reload = useCallback(
    async (filterOverride?: string) => {
      const filter = filterOverride ?? projectFilter;
      try {
        const list = await api.sites(filter === "" ? undefined : filter);
        setRows(list.sites);
        setError("");
      } catch (e) {
        setError(errorMessage(t, e));
      }
    },
    [projectFilter, t],
  );

  /**
   * Rereads the project list every time we come back to this screen — a project may have just been
   * added/edited/deleted on the Projects screen while this one was hidden, and the list used to be
   * read only once on mount, so the Select sat still with stale data.
   *
   * **Validate `projectFilter` before calling `site.list`, not after.** The selected filter may
   * point to a project that was just deleted — calling `site.list` with a project that no longer
   * exists is refused outright by the daemon ("no such project: …") rather than returning an empty
   * list, so the filter has to be reset to "all" *before* reading sites, not by catching the error
   * and retrying.
   *
   * Also where the navigation request from `sitesNavigation.ts` is read (Projects → "open Sites,
   * filtered by project X") — merged together because both decide which filter is right before
   * calling `site.list`.
   */
  useEffect(() => {
    if (!active) return;
    let live = true;
    void (async () => {
      const requested = takePendingSitesFilter();
      try {
        const list = await api.projects();
        if (!live) return;
        const names = list.projects.map((p) => p.name);
        setProjectNames(names);
        const wanted = requested ?? projectFilter;
        const valid = wanted !== "" && names.includes(wanted) ? wanted : "";
        if (valid !== projectFilter) setProjectFilter(valid);
        await reload(valid);
      } catch (e) {
        if (live) setError(errorMessage(t, e));
      }
    })();
    return () => {
      live = false;
    };
    // `reload`/`projectFilter` are deliberately read from the closure at the time the effect runs,
    // not from deps: this is the refresh for a new `active` turn, not an effect that should rerun
    // every time `projectFilter` changes by itself (SiteForm already has its own `reload()` for
    // that once it saves).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active]);

  useEffect(() => {
    return subscribeDaemonWatch((raw) => {
      setRows((current) => applySharingChange(current, raw));
    });
  }, []);

  async function edit(domain: string) {
    try {
      setEditing(await api.site(domain));
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }

  /**
   * Clicking a domain: start the services this site needs, then open it.
   *
   * **Does not wait for the services to report healthy, only for `service.start` to return** — the
   * same rule `AfterApply` follows. Real health is a question for the Services screen; waiting any
   * longer here only holds the user back in front of a browser that retries on its own anyway.
   *
   * If start fails, it does **not** open: an error page cannot say that the service is what broke,
   * while `ErrorBanner` says exactly what the daemon returned.
   */
  async function visit(row: SiteRow) {
    const { startProject, url } = siteVisit(row);
    setOpening(row.domain);
    try {
      if (startProject !== null) await api.serviceStartProject(startProject);
      await openUrl(url);
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setOpening(null);
    }
  }

  /**
   * Unshares. Updates the row as soon as the call returns, without waiting for
   * `site_sharing_changed` — that event is for when it changes **by itself** (time runs out, the
   * network drops), not for when the user has just clicked.
   */
  async function unshare(domain: string) {
    try {
      await api.siteUnshare(domain);
      setRows((current) =>
        current.map((row) => (row.domain === domain ? { ...row, sharing: null } : row)),
      );
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }

  return (
    <div className={`mixengine-page ${styles.sites}`}>
      {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}

      <PageHeader
        title={t("mixengine.sidebar.sites")}
        description={t("mixengine.sites.about")}
        actions={
          <>
            <Select
              className={styles.filter}
              size="large"
              value={projectFilter}
              onChange={(value) => {
                // The refresh effect above only runs for a new `active` turn, so while this screen is
                // already open a filter change has to read the sites itself — with `value`, not with
                // the `projectFilter` that stays one beat stale in the closure.
                setProjectFilter(value);
                void reload(value);
              }}
              searchable
              options={[
                { value: "", label: t("mixengine.sites.filterAllProjects") },
                ...projectNames.map((name) => ({ value: name, label: name })),
              ]}
            />
            <Button size="large" variant="primary" onClick={() => setCreating(true)}>
              <PlusIcon size={15} />
              {t("mixengine.sites.newSite")}
            </Button>
          </>
        }
      />

      <Card flush>
        {rows.length === 0 ? (
          <EmptyState title={t("mixengine.sites.empty")} />
        ) : (
          <Table aria-label={t("mixengine.sidebar.sites")}>
            <thead>
              <tr>
                <th>{t("mixengine.sites.columnDomain")}</th>
                <th>{t("mixengine.sites.columnOwner")}</th>
                <th>{t("mixengine.sites.columnKind")}</th>
                <th>{t("mixengine.sites.columnRoutes")}</th>
                <th>{t("mixengine.sites.columnHttps")}</th>
                <th>{t("mixengine.sites.columnState")}</th>
                <th>{t("mixengine.sites.columnSharing")}</th>
                <th data-align="end">{t("mixengine.sites.columnActions")}</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => {
                const routes = (row.routes ?? []).length; // T135. `?? []` for a daemon from before the field.
                return (
                  <tr key={row.domain}>
                    <td>
                      <span className={styles.domain}>
                        <span className={row.https ? styles.lockOn : styles.lockOff} aria-hidden="true">
                          <LockIcon size={14} />
                        </span>
                        <Button
                          variant="link"
                          disabled={opening !== null}
                          onClick={() => void visit(row)}
                          title={t("mixengine.sites.openHint", { url: siteVisit(row).url })}
                        >
                          {row.domain}
                        </Button>
                        {opening === row.domain && (
                          <span className={styles.opening}>{t("mixengine.sites.opening")}</span>
                        )}
                      </span>
                    </td>
                    <td>
                      <span className={styles.owner}>
                        {row.owner.type === "project" ? (
                          <>
                            <FolderIcon size={14} className={styles.ownerIcon} />
                            {row.owner.name}
                          </>
                        ) : (
                          t("mixengine.sites.ownerExtension", { id: row.owner.id })
                        )}
                        {!canEditSite(row.owner) && (
                          <span className={styles.readOnly} title={t("mixengine.sites.editDisabledHint")}>
                            <LockIcon size={12} />
                          </span>
                        )}
                      </span>
                    </td>
                    <td>
                      <span className={styles.kind}>{row.kind.kind}</span>
                    </td>
                    <td className={routes === 0 ? styles.none : undefined}>
                      {routes === 0 ? t("mixengine.sites.routesNone") : routes}
                    </td>
                    <td data-nowrap>
                      <span className={row.https ? styles.httpsOn : styles.none}>
                        {row.https ? t("mixengine.sites.httpsOn") : t("mixengine.sites.httpsOff")}
                      </span>
                      {/* T98: the site forces HTTPS — `?? false` for a daemon built before this
                          field existed. */}
                      {row.https && (row.https_redirect ?? false) && (
                        <span className={styles.redirect}> {t("mixengine.sites.redirect")}</span>
                      )}
                    </td>
                    <td>
                      <StatusPill tone={row.state === "enabled" ? "success" : "neutral"}>
                        {row.state === "enabled"
                          ? t("mixengine.sites.stateEnabled")
                          : t("mixengine.sites.stateDisabled")}
                      </StatusPill>
                    </td>
                    <td>
                      <SharingCell
                        sharing={row.sharing}
                        onShare={() => setSharing(row.domain)}
                        onUnshare={() => void unshare(row.domain)}
                      />
                    </td>
                    <td data-align="end" data-nowrap>
                      <span className={styles.rowActions}>
                        <Button size="small" disabled={opening !== null} onClick={() => void visit(row)}>
                          <GlobeIcon size={14} />
                          {t("mixengine.sites.open")}
                        </Button>
                        <Button
                          size="small"
                          onClick={() => void edit(row.domain)}
                          disabled={!canEditSite(row.owner)}
                          title={canEditSite(row.owner) ? undefined : t("mixengine.sites.editDisabledHint")}
                        >
                          {t("mixengine.sites.edit")}
                        </Button>
                      </span>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </Table>
        )}
      </Card>

      {creating && (
        <SiteForm
          defaultProject={projectFilter === "" ? undefined : projectFilter}
          onCancel={() => setCreating(false)}
          onSaved={() => {
            setCreating(false);
            void reload();
          }}
        />
      )}

      {editing && (
        <SiteForm
          initial={editing}
          onCancel={() => setEditing(null)}
          onSaved={() => {
            setEditing(null);
            void reload();
          }}
        />
      )}

      {sharing && (
        <ShareDialog
          domain={sharing}
          onCancel={() => setSharing(null)}
          onShared={(next) => {
            setRows((current) =>
              current.map((row) => (row.domain === sharing ? { ...row, sharing: next } : row)),
            );
            setSharing(null);
          }}
        />
      )}
    </div>
  );
}
