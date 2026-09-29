import { useCallback, useEffect, useState } from "react";

import Card from "../../../../components/Card";
import ErrorBanner from "../../../../components/ErrorBanner";
import Switch from "../../../../components/Switch";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import styles from "./AutostartPanel.module.css";

/**
 * Whether this service starts along with MixEngine — T114.
 *
 * **A switch, no Save button**, unlike `IdlePanel` right next to it: idle has three states and a
 * number to type, so it needs a confirmation; this is a boolean column, and a switch that needs an
 * extra "Save" press is a switch people think they have turned on.
 *
 * **Placed next to idle, with a line between the two.** The two settings answer two different
 * questions — "what is running when I sit down" and "what keeps running when I am not using it" —
 * and a service with both on will start at login and then be stopped when nobody uses it. That is
 * correct, and it is also exactly what people will read as a bug, so the explanation sits right
 * here rather than in the documentation.
 */
export default function AutostartPanel({ service }: { service: string }) {
  const [autostart, setAutostart] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const { t } = useTranslation();

  // Read from `service.list` rather than a separate read method: `ServiceSummary` already carries
  // this column (T112), so adding another backend command just to ask about one service would add
  // another place to drift.
  const reload = useCallback(async () => {
    try {
      const list = await api.services();
      const mine = list.services.find((summary) => summary.id === service);
      setAutostart(mine?.autostart ?? null);
      setError("");
    } catch (e) {
      setError(errorMessage(t, e));
    }
  }, [service, t]);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function toggle(wanted: boolean) {
    setBusy(true);
    setError("");
    try {
      const summary = await api.serviceSetAutostart({ service, autostart: wanted });
      // What the daemon returns, not what was just clicked: a switch that lies about the column in
      // the database is worse than a slow switch — the same rule the Dashboard's service table
      // follows.
      setAutostart(summary.autostart);
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card
      headingLevel={3}
      title={t("mixengine.servicesDetail.autostart.title")}
      description={t("mixengine.servicesDetail.autostart.toggle")}
      actions={
        <Switch
          aria-label={t("mixengine.servicesDetail.autostart.toggle")}
          checked={autostart === true}
          disabled={busy || autostart === null}
          onChange={(next) => void toggle(next)}
        />
      }
    >
      {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}
      <div className={styles.note}>
        <p>{t("mixengine.servicesDetail.autostart.dependencies")}</p>
        <p>{t("mixengine.servicesDetail.autostart.versusIdle")}</p>
      </div>
    </Card>
  );
}