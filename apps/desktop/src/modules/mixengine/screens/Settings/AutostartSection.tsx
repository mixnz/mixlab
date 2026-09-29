import { useCallback, useEffect, useState } from "react";

import Checkbox from "../../../../components/Checkbox";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { AutostartReport } from "@mixengine/api";
import { autostartPresentation } from "../../settingsState";
import SectionLoading from "./SectionLoading";
import styles from "./Settings.module.css";

/**
 * The autostart switch — T85b.
 *
 * **`enabled && !for_this_home` must read differently from `enabled && for_this_home`.** A
 * registered entry belonging to another home is still `enabled: true` — the switch is on, but on
 * for a home that is not this one. Pressing "Enable" here is still valid (it *replaces* the entry,
 * since there is only one entry per user), but the sentence shown must not say "enabled" as if
 * there were nothing more to know.
 */
export default function AutostartSection({ onError }: { onError: (message: string) => void }) {
  const [report, setReport] = useState<AutostartReport | null>(null);
  /** False until the first read has answered, failed or not — a failure is the banner's to say. */
  const [loaded, setLoaded] = useState(false);
  const [busy, setBusy] = useState(false);
  const { t } = useTranslation();

  const reload = useCallback(async () => {
    try {
      setReport(await api.autostartStatus());
    } catch (e) {
      onError(errorMessage(t, e));
    } finally {
      setLoaded(true);
    }
  }, [t, onError]);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function toggle() {
    if (report === null) return;
    setBusy(true);
    try {
      setReport(report.enabled ? await api.autostartDisable() : await api.autostartEnable());
    } catch (e) {
      onError(errorMessage(t, e));
    } finally {
      setBusy(false);
    }
  }

  if (report === null) return loaded ? null : <SectionLoading title={t("mixengine.settings.autostart.title")} />;
  const presentation = autostartPresentation(report);

  return (
    <section className={styles.section}>
      <h3 className={styles.sectionTitle}>{t("mixengine.settings.autostart.title")}</h3>

      {presentation === "unsupported" ? (
        <p className={styles.muted}>
          {t("mixengine.settings.autostart.unsupported", { location: report.location })}
        </p>
      ) : (
        <>
          <Checkbox
            className={styles.row}
            label={t("mixengine.settings.autostart.toggle")}
            checked={report.enabled}
            disabled={busy}
            onChange={() => void toggle()}
          />
          {presentation === "enabledOtherHome" && (
            <p className={styles.warn}>{t("mixengine.settings.autostart.otherHome")}</p>
          )}
          <p className={styles.muted}>
            {t("mixengine.settings.autostart.location", { location: report.location })}
          </p>
        </>
      )}
    </section>
  );
}
