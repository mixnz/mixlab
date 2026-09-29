import { useCallback, useEffect, useState } from "react";

import Button from "../../../../components/Button";
import Card from "../../../../components/Card";
import LoadingState from "../../../../components/LoadingState";
import StatusPill, { type StatusTone } from "../../../../components/StatusPill";
import Table from "../../../../components/Table";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import { buildCertRows, type CertRow } from "../../certTable";
import styles from "./CertTable.module.css";

type Translate = ReturnType<typeof useTranslation>["t"];

function outcomeLabel(row: CertRow, t: Translate): string {
  switch (row.outcome.outcome) {
    case "issued":
      return t("mixengine.domains.certs.outcome.issued");
    case "reused":
      return t("mixengine.domains.certs.outcome.reused");
    case "not_wanted":
      return t("mixengine.domains.certs.outcome.notWanted", { reason: row.outcome.because });
    case "refused":
      return t("mixengine.domains.certs.outcome.refused", { reason: row.outcome.because });
  }
}

/** The outcome in one word for the pill; the sentence with the daemon's reason is its tooltip. */
function outcomeWord(row: CertRow, t: Translate): string {
  switch (row.outcome.outcome) {
    case "issued":
      return t("mixengine.domains.certs.outcome.issued");
    case "reused":
      return t("mixengine.domains.certs.outcome.reused");
    case "not_wanted":
      return t("mixengine.domains.certs.outcome.notWantedShort");
    case "refused":
      return t("mixengine.domains.certs.outcome.refusedShort");
  }
}

function outcomeTone(row: CertRow): StatusTone {
  switch (row.outcome.outcome) {
    case "issued":
    case "reused":
      return "success";
    case "not_wanted":
      return "neutral";
    case "refused":
      return "danger";
  }
}

/**
 * Each site's certificate — T2.7.
 *
 * **One `cert.issue` call without `site` draws the whole table.** Reissuing one row calls that same
 * method again with `{ site }` — idempotent, raises no prompt.
 */
export default function CertTable({
  revision,
  onError,
}: {
  /** A change means reread — `Domains` bumps it when a job ends or when the screen is reopened. */
  revision: number;
  onError: (message: string) => void;
}) {
  const { t } = useTranslation();
  const [rows, setRows] = useState<CertRow[]>([]);
  /** False until the first read has answered: the table is drawn empty until then. */
  const [loaded, setLoaded] = useState(false);
  const [reissuing, setReissuing] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setRows(buildCertRows(await api.certs()));
    } catch (e) {
      onError(errorMessage(t, e));
    } finally {
      setLoaded(true);
    }
  }, [onError, t]);

  useEffect(() => {
    void reload();
  }, [reload, revision]);

  async function reissue(domain: string) {
    setReissuing(domain);
    try {
      const [row] = buildCertRows(await api.certs(domain));
      if (row) setRows((current) => current.map((r) => (r.domain === domain ? row : r)));
    } catch (e) {
      onError(errorMessage(t, e));
    } finally {
      setReissuing(null);
    }
  }

  return (
    <Card title={t("mixengine.domains.certs.title")} count={loaded ? rows.length : undefined} flush>
      <Table aria-label={t("mixengine.domains.certs.title")}>
        <thead>
          <tr>
            <th>{t("mixengine.domains.certs.columnDomain")}</th>
            <th>{t("mixengine.domains.certs.columnNames")}</th>
            <th data-align="end">{t("mixengine.domains.certs.columnDaysLeft")}</th>
            <th>{t("mixengine.domains.certs.columnStatus")}</th>
            <th data-align="end">{t("mixengine.domains.certs.columnActions")}</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={row.domain}>
              <td data-nowrap className={styles.domain}>
                {row.domain}
              </td>
              <td className={row.sans.length > 0 ? undefined : styles.none}>
                {row.sans.length > 0 ? (
                  <span className={styles.names}>
                    {row.sans.map((name) => (
                      <span key={name} className={styles.name}>
                        {name}
                      </span>
                    ))}
                  </span>
                ) : (
                  "—"
                )}
              </td>
              <td
                data-align="end"
                className={
                  row.daysLeft === null
                    ? styles.none
                    : row.daysLeft < 7
                      ? styles.expiring
                      : styles.days
                }
              >
                {row.daysLeft ?? "—"}
              </td>
              <td>
                <StatusPill tone={outcomeTone(row)} title={outcomeLabel(row, t)}>
                  {outcomeWord(row, t)}
                </StatusPill>
              </td>
              <td data-align="end" data-nowrap>
                <Button
                  size="small"
                  onClick={() => void reissue(row.domain)}
                  busy={reissuing === row.domain ? t("mixengine.domains.certs.reissuing") : undefined}
                >
                  {t("mixengine.domains.certs.reissue")}
                </Button>
              </td>
            </tr>
          ))}
        </tbody>
      </Table>
      {!loaded && <LoadingState />}
    </Card>
  );
}
