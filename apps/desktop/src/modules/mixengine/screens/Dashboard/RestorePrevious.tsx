import { useCallback, useEffect, useState } from "react";

import Button from "../../../../components/Button";
import ConfirmDialog from "../../../../components/ConfirmDialog";
import ErrorBanner from "../../../../components/ErrorBanner";
import NoticeBanner from "../../../../components/NoticeBanner";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import type { HomeRestoreReport } from "@mixengine/api";
import * as api from "../../api";
import { offerFrom, type RestoreOffer } from "../../previousCopy";
import styles from "./RestorePrevious.module.css";

/**
 * A copy of an earlier install's state, found in the folders this home keeps — roadmap task T182h.
 *
 * Drawn only while `home.previous` answers a copy, which the daemon does only while this home has
 * nothing of its own yet; gone once restored. The report stays until the card is left, so what was
 * skipped and what still needs a hand are read rather than guessed.
 *
 * Read again each time Dashboard comes forward, for `PathNudge`'s reason. A failed read is silent.
 */
export default function RestorePrevious({
  active,
  onRestored,
}: {
  active: boolean;
  /** Called after a restore, so the Dashboard picks up the services it brought back. */
  onRestored: () => void;
}) {
  const [offer, setOffer] = useState<RestoreOffer | null>(null);
  const [asking, setAsking] = useState(false);
  const [restoring, setRestoring] = useState(false);
  const [report, setReport] = useState<HomeRestoreReport | null>(null);
  const [error, setError] = useState("");
  const { t } = useTranslation();

  const read = useCallback(() => {
    api
      .homePrevious()
      .then((previous) => setOffer(offerFrom(previous)))
      .catch(() => {
        // Keep what was there: not knowing changes nothing.
      });
  }, []);

  useEffect(() => {
    if (active) read();
  }, [active, read]);

  async function restore() {
    setAsking(false);
    setRestoring(true);
    setError("");
    try {
      setReport(await api.homeRestore());
      onRestored();
      read();
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setRestoring(false);
    }
  }

  if (offer === null && report === null) return null;

  return (
    <section className={styles.card}>
      <h3 className={styles.title}>{t("mixengine.restorePrevious.title")}</h3>
      {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}

      {report !== null ? (
        <>
          <p className={styles.intro}>
            {t("mixengine.restorePrevious.done", {
              projects: Number(report.projects),
              sites: Number(report.sites),
              services: Number(report.services),
            })}
          </p>
          {[...report.skipped, ...report.problems].map((line) => (
            <NoticeBanner key={line} message={line} />
          ))}
        </>
      ) : (
        offer !== null && (
          <div className={styles.row}>
            <p className={styles.intro}>
              {offer.restorable
                ? t("mixengine.restorePrevious.intro", {
                    projects: offer.projects,
                    sites: offer.sites,
                    services: offer.services,
                  })
                : t("mixengine.restorePrevious.newer")}
            </p>
            {offer.restorable && (
              <Button
                variant="soft"
                busy={restoring ? t("mixengine.restorePrevious.restoring") : undefined}
                onClick={() => setAsking(true)}
              >
                {t("mixengine.restorePrevious.restore")}
              </Button>
            )}
          </div>
        )
      )}

      {asking && (
        <ConfirmDialog
          title={t("mixengine.restorePrevious.confirmTitle")}
          message={t("mixengine.restorePrevious.confirmMessage")}
          confirmLabel={t("mixengine.restorePrevious.restore")}
          onConfirm={() => void restore()}
          onCancel={() => setAsking(false)}
        />
      )}
    </section>
  );
}
