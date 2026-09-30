import { useCallback, useEffect, useState } from "react";

import ErrorBanner from "../../../../components/ErrorBanner";
import PageHeader from "../../../../components/PageHeader";
import { errorMessage } from "../../../../core/errors";
import { useTranslation } from "../../../../i18n";
import * as api from "../../api";
import type { DaemonStatus } from "@mixengine/api";
import AutostartSection from "./AutostartSection";
import DiagnosticsSection from "./DiagnosticsSection";
import DoctorSection from "./DoctorSection";
import FrontEndSection from "./FrontEndSection";
import PathSection from "./PathSection";
import styles from "./Settings.module.css";
import SaveResourcesSection from "./SaveResourcesSection";
import CredentialsSection from "./CredentialsSection";
import SectionLoading from "./SectionLoading";

/**
 * Root directory, managed TLD, autostart, doctor, diagnostics — T4.6–T4.8. Updates are not here but
 * in MixLab's Settings → Updates (T187, ADR 0056).
 *
 * **Root/TLD call no new command** — both are read from `daemon.status()` (`home`,
 * `dns?.wildcards`), the call the Dashboard already makes on every `reload()`. Settings makes its
 * own separate call, which is cheaper than sharing state with another screen.
 *
 * **"Default web server" is a section of its own (`FrontEndSection`)** — `service.set_front_end`
 * and `ServiceSummary.role` (T97) exist since bindings `v0.0.6`; on an older daemon that section
 * shows a "this version does not support it yet" line instead of disappearing.
 */
export default function Settings({ active }: { active: boolean }) {
  const [status, setStatus] = useState<DaemonStatus | null>(null);
  /** False until the first read has answered, failed or not — a failure is the banner's to say. */
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState("");
  const { t } = useTranslation();

  const reload = useCallback(async () => {
    try {
      setStatus(await api.status());
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setLoaded(true);
    }
  }, [t]);

  useEffect(() => {
    if (active) void reload();
  }, [active, reload]);

  return (
    <div className={`mixengine-page ${styles.settings}`}>
      {error !== "" && <ErrorBanner message={error} onDismiss={() => setError("")} />}

      <PageHeader title={t("mixengine.sidebar.settings")} description={t("mixengine.settings.about")} />
      {!loaded && <SectionLoading title={t("mixengine.settings.general.title")} />}
      {status && (
        <section className={styles.section}>
          <h3 className={styles.sectionTitle}>{t("mixengine.settings.general.title")}</h3>
          <p className={styles.muted}>{t("mixengine.settings.general.root", { path: status.home })}</p>
          <p className={styles.muted}>
            {status.dns && status.dns.wildcards.length > 0
              ? t("mixengine.settings.general.tlds", { tlds: status.dns.wildcards.join(", ") })
              : t("mixengine.settings.general.noTlds")}
          </p>
        </section>
      )}

      <FrontEndSection active={active} onError={setError} />

      <AutostartSection onError={setError} />
      <CredentialsSection status={status} onError={setError} />
      <SaveResourcesSection onError={setError} />
      <PathSection active={active} onError={setError} />
      <DoctorSection active={active} onError={setError} />
      <DiagnosticsSection onError={setError} />
    </div>
  );
}
