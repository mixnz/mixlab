import { open as openDialog } from "@tauri-apps/plugin-dialog";

import Button from "../../../../components/Button";
import Checkbox from "../../../../components/Checkbox";
import Input, { Textarea } from "../../../../components/Input";
import Select from "../../../../components/Select";
import SwitchTile from "../../../../components/SwitchTile";
import { PlusIcon } from "../../../../icons";
import { useTranslation } from "../../../../i18n";
import { joinDocRoot, relativeToRoot } from "../../siteState";
import { emptyRoute, needsRiskyTldConsent, type RouteRow, type SiteDraft, type Target } from "./siteDraft";
import styles from "./SiteFields.module.css";

interface Props {
  value: SiteDraft;
  onChange: (next: SiteDraft) => void;
  /** Every service id, for the pool pickers and the services list. */
  serviceIds: string[];
  /** The owning project's root: shown as the full path under Doc root, and where Browse opens.
   *  `""` when not known — the doc root is then taken as given. */
  projectRoot: string;
  disabled?: boolean;
  /** The Enabled switch — only for a site that already exists. */
  showEnabled?: boolean;
}

/**
 * The fields of a site, from Domains to the HTTPS switches.
 *
 * **One component for every form that makes or edits a site** — `SiteForm` and the "also create a
 * site" section of `ProjectForm` — so the two cannot drift: a field added here is in both. Each
 * caller keeps only what is its own (the project picker, the dialog, the call to the daemon), and
 * builds its payload from `siteCreateFields`/`siteUpdateFields` beside this file.
 */
export default function SiteFields({
  value,
  onChange,
  serviceIds,
  projectRoot,
  disabled = false,
  showEnabled = false,
}: Props) {
  const { t } = useTranslation();

  function set(change: Partial<SiteDraft>) {
    onChange({ ...value, ...change });
  }

  function toggleService(service: string) {
    const next = new Set(value.services);
    if (next.has(service)) next.delete(service);
    else next.add(service);
    set({ services: next });
  }

  function updateRoute(index: number, change: Partial<RouteRow>) {
    set({ routes: value.routes.map((route, at) => (at === index ? { ...route, ...change } : route)) });
  }

  /**
   * The dialog always returns an absolute path — strip the project root before storing it in
   * state, because that is the real shape `SiteSummary.doc_root` holds ("Relative to the project's
   * root, as stored"). Without stripping, this field shows an absolute path right after choosing
   * but only the remainder after saving and reopening — two different displays for the same site.
   *
   * `defaultPath` opens right at the current choice (root, or the current root/doc_root) so that
   * pressing Browse goes straight into the project, rather than feeling around from the drive root.
   */
  async function browseDocRoot() {
    const picked = await openDialog({
      directory: true,
      multiple: false,
      defaultPath: projectRoot === "" ? undefined : joinDocRoot(projectRoot, value.docRoot),
    });
    if (typeof picked !== "string") return;
    set({ docRoot: projectRoot === "" ? picked : relativeToRoot(projectRoot, picked) });
  }

  const poolOptions = [
    { value: "", label: t("mixengine.sites.form.poolAuto") },
    ...serviceIds.filter((s) => s.startsWith("php-fpm@")).map((s) => ({ value: s, label: s })),
  ];

  return (
    <>
      <label className={styles.field}>
        {t("mixengine.sites.form.domains")}
        <Textarea
          mono
          maxRows={6}
          value={value.domainsText}
          disabled={disabled}
          onChange={(e) => set({ domainsText: e.target.value })}
          placeholder={t("mixengine.sites.form.domainsPlaceholder")}
        />
      </label>

      {needsRiskyTldConsent(value) && (
        <Checkbox
          className={styles.checkbox}
          label={t("mixengine.sites.form.acceptRiskyTld")}
          checked={value.acceptRiskyTld}
          disabled={disabled}
          onChange={(e) => set({ acceptRiskyTld: e.target.checked })}
        />
      )}

      <label className={styles.field}>
        {t("mixengine.sites.form.docRoot")}
        <div className={styles.docRoot}>
          <Input
            mono
            value={value.docRoot}
            disabled={disabled}
            onChange={(e) => set({ docRoot: e.target.value })}
          />
          <Button onClick={() => void browseDocRoot()} disabled={disabled}>
            {t("common.browse")}
          </Button>
        </div>
        {/* The field above holds only the remainder after the root (exactly what the daemon
            stores) — this line is the only place the user sees the project's root and what the
            full path really is. */}
        {projectRoot !== "" && (
          <p className={styles.hint}>
            {t("mixengine.sites.form.docRootFull", { path: joinDocRoot(projectRoot, value.docRoot) })}
          </p>
        )}
      </label>

      {/* The kind beside the one field that kind needs; `static` needs none and has the row. */}
      <div className={styles.pair}>
        <label className={styles.field}>
          {t("mixengine.sites.form.kind")}
          <Select
            value={value.kind}
            disabled={disabled}
            onChange={(kind) => set({ kind })}
            options={[
              { value: "php-fpm", label: "php-fpm" },
              { value: "static", label: "static" },
              { value: "reverse-proxy", label: "reverse-proxy" },
              { value: "node-app", label: "node-app" },
            ]}
          />
        </label>

        {value.kind === "php-fpm" && (
          <label className={styles.field}>
            {t("mixengine.sites.form.pool")}
            <Select
              value={value.pool}
              disabled={disabled}
              onChange={(pool) => set({ pool })}
              placeholder={t("mixengine.sites.form.poolAuto")}
              options={poolOptions}
            />
          </label>
        )}

        {value.kind === "reverse-proxy" && (
          <label className={styles.field}>
            {t("mixengine.sites.form.upstream")}
            <Input
              mono
              value={value.upstream}
              disabled={disabled}
              onChange={(e) => set({ upstream: e.target.value })}
              placeholder="http://127.0.0.1:3000"
            />
          </label>
        )}

        {value.kind === "node-app" && (
          <label className={styles.field}>
            {t("mixengine.sites.form.port")}
            <Input
              type="number"
              value={value.port}
              disabled={disabled}
              onChange={(e) => set({ port: e.target.value })}
            />
          </label>
        )}
      </div>

      {/* T135. This block validates nothing — a wrong path comes back with exactly what the daemon
          says, like everything else in this form. */}
      <div className={styles.field}>
        {t("mixengine.sites.form.routes")}
        <p className={styles.hint}>{t("mixengine.sites.form.routesHint")}</p>

        {value.routes.length === 0 ? (
          <p className={styles.hint}>{t("mixengine.sites.form.routesEmpty")}</p>
        ) : (
          <div className={styles.routeList}>
            {value.routes.map((route, index) => (
              <div key={index} className={styles.route}>
                <Input
                  value={route.path}
                  disabled={disabled}
                  placeholder="/api"
                  aria-label={t("mixengine.sites.form.routePath")}
                  onChange={(e) => updateRoute(index, { path: e.target.value })}
                />
                <Select
                  value={route.target}
                  disabled={disabled}
                  onChange={(target) => updateRoute(index, { target: target as Target })}
                  options={[
                    { value: "proxy", label: "proxy" },
                    { value: "php-fpm", label: "php-fpm" },
                    { value: "static", label: "static" },
                  ]}
                />
                {route.target === "proxy" && (
                  <Input
                    value={route.upstream}
                    disabled={disabled}
                    placeholder="http://127.0.0.1:3003/xyz"
                    aria-label={t("mixengine.sites.form.routeUpstream")}
                    onChange={(e) => updateRoute(index, { upstream: e.target.value })}
                  />
                )}
                {route.target === "php-fpm" && (
                  <Select
                    value={route.pool}
                    disabled={disabled}
                    onChange={(pool) => updateRoute(index, { pool })}
                    placeholder={t("mixengine.sites.form.poolAuto")}
                    options={poolOptions}
                  />
                )}
                {route.target === "static" && (
                  <Input
                    value={route.root}
                    disabled={disabled}
                    placeholder="dist"
                    aria-label={t("mixengine.sites.form.routeRoot")}
                    onChange={(e) => updateRoute(index, { root: e.target.value })}
                  />
                )}
                <Button
                  variant="danger"
                  disabled={disabled}
                  aria-label={t("mixengine.sites.form.routesRemove")}
                  onClick={() => set({ routes: value.routes.filter((_, at) => at !== index) })}
                >
                  {t("common.delete")}
                </Button>
              </div>
            ))}
          </div>
        )}

        <Button
          className={styles.addRoute}
          disabled={disabled}
          onClick={() => set({ routes: [...value.routes, emptyRoute()] })}
        >
          <PlusIcon size={14} />
          {t("mixengine.sites.form.routesAdd")}
        </Button>
      </div>

      <div className={styles.field}>
        <span className={styles.fieldHead}>
          {t("mixengine.sites.form.services")}
          <span className={styles.count}>
            {t("mixengine.sites.form.servicesSelected", {
              count: value.services.size,
              total: serviceIds.length,
            })}
          </span>
        </span>
        <div className={styles.serviceList}>
          {serviceIds.map((service) => (
            <SwitchTile
              key={service}
              mono
              label={service}
              checked={value.services.has(service)}
              disabled={disabled}
              onChange={() => toggleService(service)}
            />
          ))}
        </div>
      </div>

      {/* Settings that take effect as the site is saved. The same tiles as the services above, so a
          heading of their own is what keeps them from reading as more services. */}
      <div className={styles.field}>
        {t("mixengine.sites.form.options")}
        <div className={styles.tiles}>
          <SwitchTile
            label={t("mixengine.sites.form.https")}
            checked={value.https}
            disabled={disabled}
            // Dropping HTTPS drops the redirect too: there is no HTTPS address to redirect to.
            onChange={(https) => set(https ? { https } : { https, httpsRedirect: false })}
          />
          <SwitchTile
            label={t("mixengine.sites.form.httpsRedirect")}
            checked={value.https && value.httpsRedirect}
            disabled={disabled || !value.https}
            onChange={(httpsRedirect) => set({ httpsRedirect })}
          />
          {showEnabled && (
            <SwitchTile
              label={t("mixengine.sites.form.enabled")}
              checked={value.enabled}
              disabled={disabled}
              onChange={(enabled) => set({ enabled })}
            />
          )}
        </div>
      </div>
    </>
  );
}
