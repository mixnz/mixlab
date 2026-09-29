import { useCallback, useEffect, useId, useState } from "react";

import Switch from "../../../../components/Switch";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import SectionLoading from "./SectionLoading";
import styles from "./Settings.module.css";

/**
 * "Save battery" — T167b, ADR 0041.
 *
 * **Off unless the user turns it on.** When off, MixEngine stops no service just because it is
 * idle: a running site just keeps running. When on, PHP pools idle for half an hour and
 * databases/caches idle for an hour are stopped, and the next request that needs them starts them
 * again — the sentence under the switch says exactly what the user will notice: the first load may
 * be a beat slower.
 *
 * `Switch` rather than `Checkbox` like the Autostart section next to it: this is a setting that
 * takes effect the moment it is clicked, exactly what `Switch` is for.
 */
export default function SaveResourcesSection({ onError }: { onError: (message: string) => void }) {
  const [on, setOn] = useState<boolean | null>(null);
  /** False until the first read has answered, failed or not — a failure is the banner's to say. */
  const [loaded, setLoaded] = useState(false);
  const [busy, setBusy] = useState(false);
  const labelId = useId();
  const { t } = useTranslation();

  const reload = useCallback(async () => {
    try {
      setOn((await api.saveResources()).on);
    } catch (e) {
      onError(errorMessage(t, e));
    } finally {
      setLoaded(true);
    }
  }, [t, onError]);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function change(next: boolean) {
    setBusy(true);
    try {
      setOn((await api.setSaveResources(next)).on);
    } catch (e) {
      onError(errorMessage(t, e));
    } finally {
      setBusy(false);
    }
  }

  if (on === null) return loaded ? null : <SectionLoading title={t("mixengine.settings.saveResources.title")} />;

  return (
    <section className={styles.section}>
      <h3 className={styles.sectionTitle}>{t("mixengine.settings.saveResources.title")}</h3>
      <div className={styles.row}>
        <Switch
          checked={on}
          disabled={busy}
          aria-labelledby={labelId}
          onChange={(next) => void change(next)}
        />
        <span id={labelId}>{t("mixengine.settings.saveResources.toggle")}</span>
      </div>
      <p className={styles.muted}>
        {on ? t("mixengine.settings.saveResources.on") : t("mixengine.settings.saveResources.off")}
      </p>
      {on && <p className={styles.muted}>{t("mixengine.settings.saveResources.keepWarm")}</p>}
    </section>
  );
}
