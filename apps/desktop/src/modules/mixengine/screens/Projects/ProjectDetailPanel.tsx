import Button from "../../../../components/Button";
import StatusPill from "../../../../components/StatusPill";
import Table from "../../../../components/Table";
import { useTranslation } from "../../../../i18n";
import type { ProjectDetail } from "@mixengine/api";
import NextStepsPanel from "../../components/NextStepsPanel";
import { declaredSiteRows } from "../../declaredSites";
import { formatPins } from "../../projectPins";
import type { SiteAddresses } from "../../siteGroups";
import styles from "./Projects.module.css";

interface Props {
  detail: ProjectDetail;
  /** Where the steps' `open` rows go: each step's own site, else the project's first — T204a. */
  addresses: SiteAddresses;
  terminalVisible: boolean;
  onOpenDashboard: () => void;
  /** The declared site being created, while it is. */
  adopting: string | null;
  onAdopt: (domain: string) => void;
}

/**
 * One project, opened in place under its row — T205. Its runtimes, the sites its `mixengine.toml`
 * declares and what its blueprint says is left to run, in that order, so everything about the row
 * someone just opened is where they are looking rather than at the foot of a long list.
 */
export default function ProjectDetailPanel({
  detail,
  addresses,
  terminalVisible,
  onOpenDashboard,
  adopting,
  onAdopt,
}: Props) {
  const { t } = useTranslation();
  const declared = declaredSiteRows(detail.declared_sites);

  return (
    <div className={styles.detail}>
      <section className={styles.section} data-demo="project-pin-list" data-demo-focus="project-open" data-demo-fit="text">
        <h4>{t("mixengine.projects.detail.pinsTitle")}</h4>
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
      </section>

      {declared.length > 0 && (
        <section className={styles.section}>
          <h4>{t("mixengine.projects.detail.declaredTitle")}</h4>
          <p className={styles.sectionHint}>{t("mixengine.projects.detail.declaredDescription")}</p>
          <Table aria-label={t("mixengine.projects.detail.declaredTitle")}>
            <thead>
              <tr>
                <th>{t("mixengine.projects.detail.declaredColumnDomain")}</th>
                <th>{t("mixengine.projects.detail.declaredColumnState")}</th>
                <th data-align="end">{t("mixengine.sites.columnActions")}</th>
              </tr>
            </thead>
            <tbody>
              {declared.map((site) => (
                <tr key={site.domain}>
                  <td>
                    <code>{site.domain}</code>
                  </td>
                  <td>
                    {site.status === "here" && (
                      <StatusPill tone="success">{t("mixengine.projects.detail.declaredHere")}</StatusPill>
                    )}
                    {site.status === "missing" && (
                      <StatusPill tone="neutral">{t("mixengine.projects.detail.declaredMissing")}</StatusPill>
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
                        onClick={() => onAdopt(site.domain)}
                        busy={adopting === site.domain ? t("mixengine.projects.detail.declaredAdding") : undefined}
                      >
                        {t("mixengine.projects.detail.declaredAdd")}
                      </Button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </Table>
        </section>
      )}

      {detail.next_steps && (
        <section className={styles.section}>
          <h4>{t("mixengine.projects.detail.stepsTitle")}</h4>
          <NextStepsPanel
            project={detail.project.name}
            root={detail.project.root}
            addresses={addresses}
            steps={detail.next_steps}
            terminalVisible={terminalVisible}
            onShowDatabase={onOpenDashboard}
            titled={false}
          />
        </section>
      )}
    </div>
  );
}
