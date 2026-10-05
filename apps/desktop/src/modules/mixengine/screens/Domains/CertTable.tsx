import { useCallback, useEffect, useState } from "react";

import Button from "../../../../components/Button";
import Card from "../../../../components/Card";
import LoadingState from "../../../../components/LoadingState";
import StatusPill, { type StatusTone } from "../../../../components/StatusPill";
import Table from "../../../../components/Table";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { SiteCertStatus } from "@mixengine/api";
import {
  buildCertRows,
  servedByDomain,
  servedCell,
  type CertRow,
  type ServedCell,
} from "../../certTable";
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

/** The Served pill's word — one explicit branch per word, no key built from a string. */
function servedWord(word: Exclude<ServedCell["word"], "unchecked">, t: Translate): string {
  switch (word) {
    case "served":
      return t("mixengine.domains.certs.served.served");
    case "no_certificate":
      return t("mixengine.domains.certs.served.no_certificate");
    case "names_differ":
      return t("mixengine.domains.certs.served.names_differ");
    case "not_served":
      return t("mixengine.domains.certs.served.not_served");
    case "served_certificate_differs":
      return t("mixengine.domains.certs.served.served_certificate_differs");
    case "not_trusted":
      return t("mixengine.domains.certs.served.not_trusted");
    case "expiring":
      return t("mixengine.domains.certs.served.expiring");
  }
}

/** The Served cell: a dash until the row has been checked, a pill naming what is wrong after. */
function ServedPill({ cell }: { cell: ServedCell }) {
  const { t } = useTranslation();
  if (cell.word === "unchecked") return <span className={styles.none}>—</span>;

  const hint =
    cell.word === "served_certificate_differs"
      ? t("mixengine.domains.certs.servedDiffersHint")
      : (cell.because ?? undefined);
  return (
    <StatusPill tone={cell.tone} title={hint}>
      {servedWord(cell.word, t)}
    </StatusPill>
  );
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
  /** The answer of `cert.status`, or `null` before anybody asked. Back to `null` after anything that
   *  may have changed what is served (a job finished, the screen reopened): what was checked then
   *  is not known to hold now. */
  const [served, setServed] = useState<Record<string, SiteCertStatus> | null>(null);
  const [checking, setChecking] = useState(false);

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

  useEffect(() => {
    setServed(null);
  }, [revision]);

  async function checkServed() {
    setChecking(true);
    try {
      setServed(servedByDomain(await api.certStatus()));
    } catch (e) {
      onError(errorMessage(t, e));
    } finally {
      setChecking(false);
    }
  }

  async function reissue(domain: string) {
    setReissuing(domain);
    try {
      const [row] = buildCertRows(await api.certs(domain));
      if (row) setRows((current) => current.map((r) => (r.domain === domain ? row : r)));
      // What was checked is no longer what is on disk.
      setServed((current) => {
        if (current === null || !(domain in current)) return current;
        const next = { ...current };
        delete next[domain];
        return next;
      });
    } catch (e) {
      onError(errorMessage(t, e));
    } finally {
      setReissuing(null);
    }
  }

  return (
    <Card
      title={t("mixengine.domains.certs.title")}
      count={loaded ? rows.length : undefined}
      flush
      actions={
        <Button
          size="small"
          onClick={() => void checkServed()}
          busy={checking ? t("mixengine.domains.certs.checkingServed") : undefined}
        >
          {t("mixengine.domains.certs.checkServed")}
        </Button>
      }
    >
      <Table aria-label={t("mixengine.domains.certs.title")}>
        <thead>
          <tr>
            <th>{t("mixengine.domains.certs.columnDomain")}</th>
            <th>{t("mixengine.domains.certs.columnNames")}</th>
            <th data-align="end">{t("mixengine.domains.certs.columnDaysLeft")}</th>
            <th>{t("mixengine.domains.certs.columnStatus")}</th>
            <th>{t("mixengine.domains.certs.columnServed")}</th>
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
              <td>
                <ServedPill cell={servedCell(served?.[row.domain])} />
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
