import { useState } from "react";

import type { CredentialStore, DaemonStatus } from "@mixengine/api";
import Button from "../../../../components/Button";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import { credentialsPresentation } from "../../settingsState";
import styles from "./Settings.module.css";

/**
 * Where this home keeps its passwords — T194, ADR 0059.
 *
 * Drawn from `daemon.status`, which the screen already reads: the store, and — only where the daemon
 * says it would accept one — a switch. A switch applies at the next start, so success is a sentence
 * saying that rather than a changed label. A daemon that predates the member draws nothing.
 */
export default function CredentialsSection({
  status,
  onError,
}: {
  status: DaemonStatus | null;
  onError: (message: string) => void;
}) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState(false);

  const shown = credentialsPresentation(status?.credentials);
  if (!shown) return null;
  const switchTo = shown.switchTo;

  async function change(to: CredentialStore) {
    setBusy(true);
    try {
      await api.setCredentialStore(to);
      setSaved(true);
    } catch (e) {
      onError(errorMessage(t, e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className={styles.section}>
      <h3 className={styles.sectionTitle}>{t("mixengine.settings.credentials.title")}</h3>
      <p className={styles.muted}>
        {shown.store === "home" ? t("mixengine.settings.credentials.home") : t("mixengine.settings.credentials.os")}
      </p>
      {switchTo && !saved && (
        <div className={styles.row}>
          <Button busy={busy ? t("mixengine.settings.credentials.saving") : undefined} onClick={() => void change(switchTo)}>
            {switchTo === "home" ? t("mixengine.settings.credentials.toFile") : t("mixengine.settings.credentials.toKeyring")}
          </Button>
        </div>
      )}
      {saved && <p className={styles.muted}>{t("mixengine.settings.credentials.nextStart")}</p>}
    </section>
  );
}
