import { useEffect, useState } from "react";

import Button from "../../../../components/Button";
import ErrorBanner from "../../../../components/ErrorBanner";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { PathReport } from "@mixengine/api";
import { shouldOfferPathInstall } from "../../pathState";
import styles from "./PathNudge.module.css";

/**
 * A reminder to put `<root>/bin` on PATH, for someone who just installed the app and has never
 * typed `mix path install`.
 *
 * **Only drawn when `on_path: false`, and it goes away by getting the job done** — like the Quick
 * Start card, there is no "Hide" button or saved flag. After installing, the card stays with an
 * "open a new terminal" line, because that is what the user needs to know next; the next time the
 * tab opens it is gone.
 *
 * A failed `path.status` read stays silent: a Dashboard turned red because of a reminder is a
 * Dashboard turned red over a decorative sentence. The full on/off, with error reporting, lives in
 * Settings.
 *
 * **Read again each time Dashboard comes back to the front**, not once at mount: Dashboard stays
 * mounted after its first visit (`mountedScreens`), so a switch flipped in Settings, or a
 * `mix path install` in a terminal, would otherwise never reach this card. The "open a new
 * terminal" line goes with the same reading, which is what "gone the next time" means here.
 */
export default function PathNudge({ active }: { active: boolean }) {
  const [report, setReport] = useState<PathReport | null>(null);
  const [installing, setInstalling] = useState(false);
  const [done, setDone] = useState(false);
  const [error, setError] = useState("");
  const { t } = useTranslation();

  useEffect(() => {
    if (!active) return;
    let live = true;
    api
      .pathStatus()
      .then((next) => {
        if (!live) return;
        setReport(next);
        setDone(false);
      })
      .catch(() => {
        // Keep the old report: not knowing changes nothing.
      });
    return () => {
      live = false;
    };
  }, [active]);

  async function install() {
    setInstalling(true);
    try {
      await api.pathInstall();
      setDone(true);
      setError("");
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setInstalling(false);
    }
  }

  if (!done && !shouldOfferPathInstall(report)) return null;

  return (
    <section className={styles.card}>
      {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}
      <h3 className={styles.title}>{t("mixengine.pathNudge.title")}</h3>
      {done ? (
        <p className={styles.intro}>{t("mixengine.pathNudge.done")}</p>
      ) : (
        <div className={styles.row}>
          <p className={styles.intro}>{t("mixengine.pathNudge.intro")}</p>
          <Button
            variant="soft"
            busy={installing ? t("mixengine.pathNudge.installing") : undefined}
            onClick={() => void install()}
          >
            {t("mixengine.pathNudge.install")}
          </Button>
        </div>
      )}
    </section>
  );
}
