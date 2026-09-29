import { useCallback, useEffect, useState } from "react";

import Checkbox from "../../../components/Checkbox";
import ErrorBanner from "../../../components/ErrorBanner";
import { errorMessage } from "../../../core/errors";
import { loginItemStatus, setLoginItem, type LoginItem } from "../../../core/window";
import { useTranslation } from "../../../i18n";
import styles from "./SettingsModal.module.css";

/**
 * MixLab-wide behaviour that is not how it looks — T192. For now, MixLab at login (ADR 0042), here
 * rather than in the MixEngine pane since ADR 0058: starting in the tray is MixLab's, whatever
 * modules are visible. The daemon's own switch stays in the MixEngine pane.
 *
 * **Read on every mount.** The entry can be removed from System Settings, Task Manager or a
 * desktop's startup list without MixLab knowing, so the switch shows what is there now.
 */
function GeneralSection() {
  const [item, setItem] = useState<LoginItem | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const { t } = useTranslation();

  const reload = useCallback(async () => {
    try {
      setItem(await loginItemStatus());
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }, [t]);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function toggle() {
    if (item === null) return;
    setBusy(true);
    try {
      setItem(await setLoginItem(!item.enabled));
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className={styles.section}>
      {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}
      <span className={styles.sectionLabel}>{t("loginItem.title")}</span>
      {item === null ? null : item.supported ? (
        <>
          <Checkbox
            label={t("loginItem.toggle")}
            checked={item.enabled}
            disabled={busy}
            onChange={() => void toggle()}
          />
          <p className={styles.hint}>{item.trayHost ? t("loginItem.about") : t("loginItem.noTray")}</p>
        </>
      ) : (
        <p className={styles.hint}>{t("loginItem.unsupported")}</p>
      )}
    </div>
  );
}

export default GeneralSection;
