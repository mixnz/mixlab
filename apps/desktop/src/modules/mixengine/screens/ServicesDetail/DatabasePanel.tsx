import { useEffect, useState } from "react";

import Button from "../../../../components/Button";
import Card from "../../../../components/Card";
import ErrorBanner from "../../../../components/ErrorBanner";
import Input from "../../../../components/Input";
import { copyText } from "../../../../core/clipboard";
import { errorMessage } from "../../../../core/errors";
import { CopyIcon, LockIcon } from "../../../../icons";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { DatabaseClientReport } from "@mixengine/api";
import { createsDatabases, opensADatabase } from "./openChoices";
import styles from "./DatabasePanel.module.css";

/**
 * What the Services screen says about a database — and **says nothing at all about a service that
 * is not a database**.
 *
 * *Open* is no longer here: it is an action on the service itself, so it lives in that row's
 * three-dot menu on the Dashboard. Standing next to the Create button, the two buttons answered two
 * unrelated questions with the same shape, and the "Database name" field between them belonged to
 * exactly one of the two.
 */
export default function DatabasePanel({ service }: { service: string }) {
  const [report, setReport] = useState<DatabaseClientReport | null>(null);
  const [dbName, setDbName] = useState("");
  const [userName, setUserName] = useState("");
  const [createdMessage, setCreatedMessage] = useState("");
  const [busy, setBusy] = useState(false);
  /** The error of an action in the panel. The panel is not hidden — a failed Create does not change
   *  the fact that this service is still a database. */
  const [error, setError] = useState("");
  /** The error of the `database.client` read itself. The panel does not know whether it should
   *  exist, so it shows exactly that sentence and nothing else. */
  const [loadError, setLoadError] = useState("");
  const { t } = useTranslation();

  /**
   * Reads `database.client` for the selected service.
   *
   * **Clears the old answer before asking the new question, and ignores answers that arrive
   * late.** Without the first, a failed read leaves the *previous* service's `report` in place —
   * exactly how the panel once showed `postgres@main`'s credential address under the name
   * `php-fpm@8.4.24`. Without the second, clicking quickly through two services lets the old one's
   * response land later and win, the same race `readOrder.ts` describes for the Dashboard table.
   */
  useEffect(() => {
    let live = true;
    setReport(null);
    setError("");
    setLoadError("");
    setCreatedMessage("");
    setDbName("");
    setUserName("");

    void (async () => {
      try {
        const answer = await api.databaseClient(service);
        if (live) setReport(answer);
      } catch (e) {
        if (live) setLoadError(errorMessage(t, e));
      }
    })();

    return () => {
      live = false;
    };
  }, [service, t]);

  async function create() {
    setBusy(true);
    setError("");
    setCreatedMessage("");
    try {
      const account = await api.databaseCreate({
        service,
        database: dbName,
        user: userName.trim() === "" ? undefined : userName,
      });
      setCreatedMessage(
        account.made.database === "created"
          ? t("mixengine.servicesDetail.database.createdNew")
          : t("mixengine.servicesDetail.database.createdExisting"),
      );
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setBusy(false);
    }
  }

  // Asked and got nothing: say so. Staying silent here is a panel disappearing because the daemon
  // could not answer, and the reader concludes this service is not a database — something nobody
  // said.
  if (loadError !== "") {
    return <ErrorBanner message={loadError} onDismiss={() => setLoadError("")} />;
  }

  // Not finished reading: nothing to say yet.
  if (report === null) return null;

  // **Not a database means no panel at all.** `protocol: null` is a state the daemon answers for
  // nginx, caddy and every php-fpm pool — a line of text explaining there is nothing here is still
  // a block taking up space saying there is.
  if (!opensADatabase(report)) return null;

  return (
    <Card headingLevel={3} title={t("mixengine.servicesDetail.database.title")}>
      {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}

      <div className={styles.body}>
        {report.secret && (
          <div className={styles.secret}>
            <LockIcon size={14} className={styles.secretIcon} />
            <span className={styles.secretText}>
              {t("mixengine.servicesDetail.database.secretLine", { key: report.secret.key })}
            </span>
            <Button
              size="small"
              variant="ghost"
              aria-label={t("mixengine.servicesDetail.database.copyKey")}
              title={t("mixengine.servicesDetail.database.copyKey")}
              onClick={() => void copyText(report.secret?.key ?? "")}
            >
              <CopyIcon size={14} />
            </Button>
          </div>
        )}

        {/* Redis and MongoDB do not create databases this way: the daemon says so, and a form that
            can only be refused is not drawn (T155). */}
        {createsDatabases(report) && (
          <>
            <h4 className={styles.groupTitle}>{t("mixengine.servicesDetail.database.createTitle")}</h4>
            <div className={styles.fields}>
              <label className={styles.field}>
                {t("mixengine.servicesDetail.database.databaseName")}
                <Input mono value={dbName} disabled={busy} onChange={(e) => setDbName(e.target.value)} />
              </label>
              <label className={styles.field}>
                {t("mixengine.servicesDetail.database.userName")}
                {/* The daemon names the account after the database when none is given; the
                    placeholder says so as the database name is typed. */}
                <Input
                  mono
                  value={userName}
                  placeholder={dbName}
                  disabled={busy}
                  onChange={(e) => setUserName(e.target.value)}
                />
              </label>
              <Button
                variant="primary"
                className={styles.create}
                onClick={() => void create()}
                disabled={busy || dbName.trim() === ""}
              >
                {t("mixengine.servicesDetail.database.create")}
              </Button>
            </div>

            {createdMessage !== "" && <p className={styles.created}>{createdMessage}</p>}
          </>
        )}
      </div>
    </Card>
  );
}