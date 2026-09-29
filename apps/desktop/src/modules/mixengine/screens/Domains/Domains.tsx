import { useCallback, useEffect, useState } from "react";

import Button from "../../../../components/Button";
import Card from "../../../../components/Card";
import EmptyState from "../../../../components/EmptyState";
import ErrorBanner from "../../../../components/ErrorBanner";
import PageHeader from "../../../../components/PageHeader";
import Table from "../../../../components/Table";
import { CheckIcon, GlobeIcon, PlusIcon } from "../../../../icons";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { DomainStatus } from "@mixengine/api";
import { isJobFinished, needsResync } from "../../daemonState";
import { subscribeDaemonWatch } from "../../daemonWatch";
import AddDomainDialog from "./AddDomainDialog";
import CaBlock from "./CaBlock";
import CertTable from "./CertTable";
import styles from "./Domains.module.css";

/** One of the four yes-or-no facts about a domain: a tick in the success tone, or a dash. */
function Fact({ on }: { on: boolean }) {
  return on ? <CheckIcon size={15} className={styles.yes} /> : <span className={styles.none}>—</span>;
}

/**
 * The domain diagnosis table — T2.5.
 *
 * **Four independent facts, not one verdict.** `hosts_entry`, `wildcard`, `server_answers` and
 * `resolves_to` each answer a different question; `because` is the only sentence saying what is
 * wrong, drawn verbatim — not translated, because the daemon writes it.
 *
 * **All three blocks (CA, domain table, certificate table) reread when a job ends**, not only on
 * mount or on returning to the screen. A finished `elevation.grant` (from "Fix browser trust"
 * right here, from the Dashboard, or from the CLI) changes both each domain's `because` and the
 * CA's state, and the daemon emits no event of its own for that — `job_finished` is the signal
 * (see `isJobFinished`). `revision` is how this screen tells the two child blocks to reread
 * without making each of them subscribe to the event channel.
 */
export default function Domains({ active }: { active: boolean }) {
  const [rows, setRows] = useState<DomainStatus[]>([]);
  const [error, setError] = useState("");
  const [adding, setAdding] = useState(false);
  /** Bumped every time there is a reason for every block to reread — `CaBlock`/`CertTable` reread
   *  when it changes. */
  const [revision, setRevision] = useState(0);
  const { t } = useTranslation();

  const reload = useCallback(async () => {
    try {
      const report = await api.domains();
      setRows(report.domains);
      setError("");
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }, [t]);

  // Reread on mount and every time we come back to this screen — the same reason as
  // `Dashboard.tsx`.
  useEffect(() => {
    if (active) {
      void reload();
      setRevision((n) => n + 1);
    }
  }, [active, reload]);

  useEffect(() => {
    return subscribeDaemonWatch((raw) => {
      if (isJobFinished(raw) || needsResync(raw)) {
        void reload();
        setRevision((n) => n + 1);
      }
    });
  }, [reload]);

  async function remove(domain: string) {
    try {
      await api.domainRemove(domain);
      await reload();
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }

  return (
    <div className={`mixengine-page ${styles.domains}`}>
      {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}

      <PageHeader
        title={t("mixengine.sidebar.domains")}
        description={t("mixengine.domains.about")}
        actions={
          <Button size="large" variant="primary" onClick={() => setAdding(true)}>
            <PlusIcon size={15} />
            {t("mixengine.domains.addDomain")}
          </Button>
        }
      />

      <CaBlock revision={revision} onError={setError} />

      <Card title={t("mixengine.domains.title")} count={rows.length} flush>
        {rows.length === 0 ? (
          <EmptyState title={t("mixengine.domains.empty")} />
        ) : (
          <Table aria-label={t("mixengine.domains.title")}>
            <thead>
              <tr>
                <th>{t("mixengine.domains.columnDomain")}</th>
                <th>{t("mixengine.domains.columnSite")}</th>
                <th>{t("mixengine.domains.columnHosts")}</th>
                <th>{t("mixengine.domains.columnWildcard")}</th>
                <th>{t("mixengine.domains.columnServer")}</th>
                <th>{t("mixengine.domains.columnResolves")}</th>
                <th className={styles.reason}>{t("mixengine.domains.columnReason")}</th>
                <th data-align="end">{t("mixengine.domains.columnActions")}</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <tr key={row.domain}>
                  <td data-nowrap>
                    <span className={styles.domain}>
                      <span className={row.because == null ? styles.globeOk : styles.globeBad} aria-hidden="true">
                        <GlobeIcon size={14} />
                      </span>
                      {row.domain}
                    </span>
                  </td>
                  <td className={row.site == null ? styles.none : undefined}>{row.site ?? "—"}</td>
                  <td>
                    <Fact on={row.hosts_entry} />
                  </td>
                  <td>
                    <Fact on={row.wildcard} />
                  </td>
                  <td className={row.server_answers == null ? styles.none : styles.mono}>
                    {row.server_answers ?? "—"}
                  </td>
                  <td className={row.resolves_to.length === 0 ? styles.none : styles.mono}>
                    {row.resolves_to.length > 0 ? (
                      <span className={styles.resolves}>
                        {row.resolves_to.join(", ")}
                        {/* The machine resolves the name to what this daemon's server answers. */}
                        {row.server_answers != null && row.resolves_to.includes(row.server_answers) && (
                          <CheckIcon size={14} className={styles.yes} />
                        )}
                      </span>
                    ) : (
                      "—"
                    )}
                  </td>
                  {/* `because` is "a sentence saying what is wrong, or None when nothing is" (the
                      `DomainStatus` doc comment) — text means an error, so the red reads before
                      the sentence does. */}
                  <td className={`${styles.reason} ${styles.bad}`}>{row.because ?? ""}</td>
                  <td data-align="end" data-nowrap>
                    <Button size="small" variant="danger" onClick={() => void remove(row.domain)}>
                      {t("mixengine.domains.remove")}
                    </Button>
                  </td>
                </tr>
              ))}
            </tbody>
          </Table>
        )}
      </Card>

      <CertTable revision={revision} onError={setError} />

      {adding && (
        <AddDomainDialog
          onCancel={() => setAdding(false)}
          onAdded={() => {
            setAdding(false);
            void reload();
          }}
        />
      )}
    </div>
  );
}
